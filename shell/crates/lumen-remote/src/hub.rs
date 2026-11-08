//! The hub: many phones on one `lumen-ctl serve --link web`. It holds no
//! socket; the server hands it lines and connections, and does what it
//! answers (`Action`), so the tests drive it with plain strings.
//!
//! * **Requests:** each phone's request is checked against its role, with
//!   the verb lists lumen-ctl's hello gave (`roles`, `remoteSettings`),
//!   then sent on with a fresh id and the phone's token. lumen-ctl checks
//!   the token and the role again, and its reply goes back to that phone
//!   under the phone's own id.
//! * **Events:** each watched state's last value is kept, so a phone that
//!   connects gets its own hello (its role's verbs, its role and its name)
//!   and the current state, as lumen-ctl gives them on a fresh session.
//!   Every change and `alive` go to every phone.
//! * **Revocation:** lumen-ctl's `devices` event (a device paired, was
//!   revoked or changed role) empties the token cache and asks again for
//!   each open WebSocket's device: a revoked one is closed with 4401, one
//!   whose role changed with 4001, so the page reconnects with its new
//!   verbs.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

pub type ConnId = u64;

pub const MAX_REQUEST_ID: u64 = 2_147_483_647;
const MAX_LINE: usize = 65_536;
/// Requests one phone may have waiting at once.
const MAX_PENDING: usize = 32;
const MAX_CACHE: usize = 64;
/// The states a new phone is sent, in lumen-ctl's order.
const STATES: [&str; 4] = ["power", "volume", "foreground", "inputs"];

pub const CLOSE_REVOKED: u16 = 4401;
pub const CLOSE_ROLE: u16 = 4001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub role: String,
}

impl Device {
    fn from(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let role = s("role").filter(|r| r == "remote" || r == "owner")?;
        Some(Self {
            id: s("id")?,
            name: s("name")?,
            role,
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "name": self.name, "role": self.role })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// A line for lumen-ctl.
    Ctl(String),
    /// A line for a phone's WebSocket.
    Send(ConnId, String),
    /// Close a phone's WebSocket with this code.
    Close(ConnId, u16, &'static str),
    /// The device a connection's cookie belongs to, if any.
    Authed(ConnId, Option<Device>),
    /// A pairing's outcome: the new device and its token, or why not.
    Paired(ConnId, Result<(Device, String), String>),
    Log(String),
}

#[derive(Debug)]
enum Pending {
    Request { conn: ConnId, id: u64 },
    Auth { conn: ConnId, token: String },
    Reauth { conn: ConnId },
    Pair { conn: ConnId },
}

struct Client {
    token: String,
    device: Device,
    pending: usize,
    ready: bool,
}

#[derive(Default)]
pub struct Hub {
    hello: Option<Map<String, Value>>,
    roles: HashMap<String, Vec<String>>,
    remote_settings: Vec<String>,
    states: HashMap<&'static str, Value>,
    settings: Map<String, Value>,
    next: u64,
    pending: HashMap<u64, Pending>,
    clients: HashMap<ConnId, Client>,
    cache: HashMap<String, Device>,
}

fn reply_err(id: Option<u64>, code: &str, message: &str) -> String {
    json!({ "id": id, "ok": false, "error": { "code": code, "message": message } }).to_string()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sends `verb` to lumen-ctl, remembering what its reply is for.
    fn ask(
        &mut self,
        verb: &str,
        args: Option<Value>,
        token: Option<&str>,
        why: Pending,
    ) -> Action {
        loop {
            self.next = self.next % MAX_REQUEST_ID + 1;
            if !self.pending.contains_key(&self.next) {
                break;
            }
        }
        let mut line = json!({ "id": self.next, "verb": verb });
        if let Some(args) = args {
            line["args"] = args;
        }
        if let Some(token) = token {
            line["token"] = json!(token);
        }
        self.pending.insert(self.next, why);
        Action::Ctl(line.to_string())
    }

    /// A connection's cookie: answered at once from the cache, else asked.
    pub fn auth(&mut self, conn: ConnId, token: &str) -> Vec<Action> {
        if let Some(d) = self.cache.get(token) {
            return vec![Action::Authed(conn, Some(d.clone()))];
        }
        vec![self.ask(
            "device.auth",
            None,
            Some(token),
            Pending::Auth {
                conn,
                token: token.to_owned(),
            },
        )]
    }

    pub fn pair(&mut self, conn: ConnId, code: &str, name: &str) -> Vec<Action> {
        vec![self.ask(
            "pair.complete",
            Some(json!({ "code": code, "name": name })),
            None,
            Pending::Pair { conn },
        )]
    }

    /// A paired device's WebSocket is open.
    pub fn ws_open(&mut self, conn: ConnId, token: &str, device: Device) -> Vec<Action> {
        self.clients.insert(
            conn,
            Client {
                token: token.to_owned(),
                device,
                pending: 0,
                ready: false,
            },
        );
        self.greet(conn)
    }

    pub fn closed(&mut self, conn: ConnId) {
        self.clients.remove(&conn);
    }

    #[cfg(test)]
    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// The phone's own hello and the current state, once lumen-ctl's hello
    /// is in.
    fn greet(&mut self, conn: ConnId) -> Vec<Action> {
        let Some(hello) = &self.hello else {
            return Vec::new();
        };
        let Some(client) = self.clients.get_mut(&conn) else {
            return Vec::new();
        };
        if client.ready {
            return Vec::new();
        }
        client.ready = true;
        let mut data = hello.clone();
        let verbs = self
            .roles
            .get(&client.device.role)
            .cloned()
            .unwrap_or_default();
        data.insert("verbs".into(), json!(verbs));
        data.remove("roles");
        data.remove("remoteSettings");
        let ctl = data
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("lumen-ctl")
            .to_owned();
        data.insert(
            "server".into(),
            json!(format!(
                "lumen-remote {} ({ctl})",
                env!("CARGO_PKG_VERSION")
            )),
        );
        data.insert("role".into(), json!(client.device.role));
        data.insert(
            "device".into(),
            json!({ "id": client.device.id, "name": client.device.name }),
        );
        let mut out = vec![Action::Send(
            conn,
            json!({ "event": "hello", "data": data }).to_string(),
        )];
        for name in STATES {
            if let Some(v) = self.states.get(name) {
                out.push(Action::Send(
                    conn,
                    json!({ "event": name, "data": v }).to_string(),
                ));
            }
        }
        if !self.settings.is_empty() {
            out.push(Action::Send(
                conn,
                json!({ "event": "settings", "data": { "values": self.settings } }).to_string(),
            ));
        }
        out
    }

    fn broadcast(&self, line: &str) -> Vec<Action> {
        self.clients
            .iter()
            .filter(|(_, c)| c.ready)
            .map(|(id, _)| Action::Send(*id, line.to_owned()))
            .collect()
    }

    /// One line from lumen-ctl.
    pub fn ctl_line(&mut self, line: &str) -> Vec<Action> {
        let Ok(Value::Object(msg)) = serde_json::from_str::<Value>(line) else {
            return vec![Action::Log(
                "lumen-ctl sent a line that isn't a JSON object".into(),
            )];
        };
        if let Some(name) = msg.get("event").and_then(Value::as_str) {
            let data = msg.get("data").cloned().unwrap_or(Value::Null);
            return self.event(name, data, line);
        }
        let Some(id) = msg.get("id").and_then(Value::as_u64) else {
            let why = msg
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str);
            return vec![Action::Log(format!(
                "lumen-ctl refused a line: {}",
                why.unwrap_or("?")
            ))];
        };
        let Some(pending) = self.pending.remove(&id) else {
            return Vec::new();
        };
        let ok = msg.get("ok") == Some(&Value::Bool(true));
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        let error = || {
            msg.get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("lumen-ctl refused it")
                .to_owned()
        };
        match pending {
            Pending::Request { conn, id: theirs } => {
                let Some(client) = self.clients.get_mut(&conn) else {
                    return Vec::new();
                };
                client.pending = client.pending.saturating_sub(1);
                let mut reply = msg.clone();
                reply.insert("id".into(), json!(theirs));
                vec![Action::Send(conn, Value::Object(reply).to_string())]
            }
            Pending::Auth { conn, token } => {
                let device = if ok { Device::from(&result) } else { None };
                if let Some(d) = &device {
                    if self.cache.len() >= MAX_CACHE {
                        self.cache.clear();
                    }
                    self.cache.insert(token, d.clone());
                }
                vec![Action::Authed(conn, device)]
            }
            Pending::Reauth { conn } => {
                let Some(client) = self.clients.get_mut(&conn) else {
                    return Vec::new();
                };
                match Device::from(&result).filter(|_| ok) {
                    Some(d) if d.role == client.device.role => {
                        client.device = d;
                        Vec::new()
                    }
                    Some(d) => {
                        let id = d.id.clone();
                        self.clients.remove(&conn);
                        vec![
                            Action::Close(conn, CLOSE_ROLE, "role changed"),
                            Action::Log(format!(
                                "device {id} is now {}: its page reconnects",
                                d.role
                            )),
                        ]
                    }
                    None => {
                        let id = client.device.id.clone();
                        self.clients.remove(&conn);
                        vec![
                            Action::Close(conn, CLOSE_REVOKED, "revoked"),
                            Action::Log(format!("device {id} was revoked: its WebSocket closed")),
                        ]
                    }
                }
            }
            Pending::Pair { conn } => {
                let paired = if ok {
                    Device::from(&result)
                        .zip(
                            result
                                .get("token")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        )
                        .ok_or_else(|| "lumen-ctl's answer was not a device".to_owned())
                } else {
                    Err(error())
                };
                vec![Action::Paired(conn, paired)]
            }
        }
    }

    fn event(&mut self, name: &str, data: Value, line: &str) -> Vec<Action> {
        match name {
            "hello" => {
                let Value::Object(hello) = data else {
                    return vec![Action::Log("lumen-ctl's hello has no data".into())];
                };
                self.roles = ["remote", "owner"]
                    .iter()
                    .map(|r| {
                        (
                            (*r).to_owned(),
                            strings(hello.get("roles").and_then(|x| x.get(*r))),
                        )
                    })
                    .collect();
                self.remote_settings = strings(hello.get("remoteSettings"));
                self.hello = Some(hello);
                let waiting: Vec<ConnId> = self.clients.keys().copied().collect();
                waiting.into_iter().flat_map(|c| self.greet(c)).collect()
            }
            "devices" => {
                self.cache.clear();
                let open: Vec<(ConnId, String)> = self
                    .clients
                    .iter()
                    .map(|(id, c)| (*id, c.token.clone()))
                    .collect();
                open.into_iter()
                    .map(|(conn, token)| {
                        self.ask("device.auth", None, Some(&token), Pending::Reauth { conn })
                    })
                    .collect()
            }
            "settings" => {
                if let Some(Value::Object(values)) = data.get("values") {
                    for (k, v) in values {
                        self.settings.insert(k.clone(), v.clone());
                    }
                }
                self.broadcast(line)
            }
            "alive" => self.broadcast(line),
            _ => match STATES.iter().find(|s| **s == name) {
                Some(state) => {
                    self.states.insert(state, data);
                    self.broadcast(line)
                }
                // Anything else stays here.
                None => Vec::new(),
            },
        }
    }

    /// A WebSocket message from a phone: one request per line.
    pub fn phone_message(&mut self, conn: ConnId, text: &str) -> Vec<Action> {
        let mut out = Vec::new();
        for line in text.split('\n').filter(|l| !l.trim().is_empty()) {
            match self.check(conn, line) {
                Ok(action) => out.push(action),
                Err(reply) => out.push(Action::Send(conn, reply)),
            }
        }
        out
    }

    /// A phone's request, checked against its role and sent on with its
    /// token, or the refusal to send back.
    fn check(&mut self, conn: ConnId, line: &str) -> Result<Action, String> {
        if line.len() > MAX_LINE {
            return Err(reply_err(
                None,
                "bad-line",
                "line is longer than 65536 bytes",
            ));
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Err(reply_err(None, "bad-line", "not JSON"));
        };
        let Value::Object(req) = value else {
            return Err(reply_err(None, "bad-line", "not a JSON object"));
        };
        let Some(id) = req
            .get("id")
            .and_then(Value::as_u64)
            .filter(|i| (1..=MAX_REQUEST_ID).contains(i))
        else {
            return Err(reply_err(
                None,
                "bad-line",
                "id must be an integer from 1 to 2147483647",
            ));
        };
        if let Some(extra) = req
            .keys()
            .find(|k| !matches!(k.as_str(), "id" | "verb" | "args"))
        {
            return Err(reply_err(
                Some(id),
                "bad-line",
                &format!("unexpected field {}", json!(extra)),
            ));
        }
        let verb = req.get("verb").and_then(Value::as_str).unwrap_or("");
        let Some(client) = self.clients.get(&conn) else {
            return Err(reply_err(
                Some(id),
                "unavailable",
                "this connection is closing",
            ));
        };
        if self.hello.is_none() {
            return Err(reply_err(
                Some(id),
                "unavailable",
                "the TV's lumen-ctl is starting",
            ));
        }
        let allowed = self
            .roles
            .get(&client.device.role)
            .is_some_and(|v| v.iter().any(|x| x == verb));
        if !allowed {
            let owners = self
                .roles
                .get("owner")
                .is_some_and(|v| v.iter().any(|x| x == verb));
            return Err(if owners {
                reply_err(
                    Some(id),
                    "forbidden",
                    &format!("{verb} needs owner rights, which only the PC grants"),
                )
            } else {
                let shown: String = verb.chars().take(40).collect();
                reply_err(
                    Some(id),
                    "unknown-verb",
                    &format!("unknown verb {}", json!(shown)),
                )
            });
        }
        if verb == "settings.set" && client.device.role != "owner" {
            let key = req
                .get("args")
                .and_then(|a| a.get("key"))
                .and_then(Value::as_str);
            if !key.is_some_and(|k| self.remote_settings.iter().any(|s| s == k)) {
                return Err(reply_err(
                    Some(id),
                    "forbidden",
                    "only the picture and sound modes can be changed without owner rights",
                ));
            }
        }
        if client.pending >= MAX_PENDING {
            return Err(reply_err(
                Some(id),
                "unavailable",
                "too many requests at once",
            ));
        }
        let token = client.token.clone();
        let args = req.get("args").cloned();
        let verb = verb.to_owned();
        if let Some(c) = self.clients.get_mut(&conn) {
            c.pending += 1;
        }
        Ok(self.ask(&verb, args, Some(&token), Pending::Request { conn, id }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn hello() -> String {
        json!({ "event": "hello", "data": {
            "protocol": 1, "server": "lumen-ctl 0.0.1",
            "verbs": ["volume.set", "settings.set", "privacy.state", "launch"],
            "roles": { "remote": ["volume.set", "settings.set", "launch"], "owner": ["volume.set", "settings.set", "privacy.state", "launch"] },
            "remoteSettings": ["picture.mode", "sound.mode"],
            "bus": "fake", "keys": "none",
        }})
        .to_string()
    }

    fn device(role: &str) -> Device {
        Device {
            id: "0a1b2c3d".into(),
            name: "Pixel".into(),
            role: role.into(),
        }
    }

    fn sent(actions: &[Action]) -> Vec<Value> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Send(_, l) => Some(serde_json::from_str(l).unwrap()),
                _ => None,
            })
            .collect()
    }

    fn to_ctl(actions: &[Action]) -> Vec<Value> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Ctl(l) => Some(serde_json::from_str(l).unwrap()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_phone_gets_its_own_hello_and_the_state() {
        let mut hub = Hub::new();
        // A phone that connects before lumen-ctl's hello waits for it.
        assert!(hub.ws_open(1, TOKEN, device("remote")).is_empty());
        hub.ctl_line(r#"{"event":"volume","data":{"level":3,"muted":false}}"#);
        let out = sent(&hub.ctl_line(&hello()));
        assert_eq!(out.len(), 2, "{out:?}");
        let h = &out[0]["data"];
        assert_eq!(h["verbs"], json!(["volume.set", "settings.set", "launch"]));
        assert_eq!(h["role"], json!("remote"));
        assert_eq!(h["device"], json!({ "id": "0a1b2c3d", "name": "Pixel" }));
        assert!(h.get("roles").is_none() && h.get("remoteSettings").is_none());
        assert!(h["server"].as_str().unwrap().starts_with("lumen-remote "));
        assert_eq!(out[1]["event"], json!("volume"));
        // Settings merge; a later phone gets them all.
        hub.ctl_line(r#"{"event":"settings","data":{"values":{"picture.mode":"cinema","sound.mode":"standard"}}}"#);
        hub.ctl_line(r#"{"event":"settings","data":{"values":{"picture.mode":"game"}}}"#);
        let out = sent(&hub.ws_open(2, TOKEN, device("owner")));
        assert_eq!(out[0]["data"]["role"], json!("owner"));
        assert_eq!(
            out.last().unwrap()["data"]["values"],
            json!({ "picture.mode": "game", "sound.mode": "standard" })
        );
        // Events go to both; unknown ones and devices stay.
        assert_eq!(hub.ctl_line(r#"{"event":"alive","data":{}}"#).len(), 2);
        assert!(hub.ctl_line(r#"{"event":"secret","data":{}}"#).is_empty());
    }

    #[test]
    fn requests_go_on_with_the_token_and_come_back_with_the_phones_id() {
        let mut hub = Hub::new();
        hub.ctl_line(&hello());
        hub.ws_open(1, TOKEN, device("remote"));
        let out = hub.phone_message(
            1,
            "{\"id\":7,\"verb\":\"volume.set\",\"args\":{\"level\":20}}\n",
        );
        let up = &to_ctl(&out)[0];
        assert_eq!(up["verb"], json!("volume.set"));
        assert_eq!(up["token"], json!(TOKEN));
        assert_eq!(up["args"], json!({ "level": 20 }));
        let upstream_id = up["id"].as_u64().unwrap();
        let back = hub.ctl_line(&format!(
            r#"{{"id":{upstream_id},"ok":true,"result":{{}}}}"#
        ));
        assert_eq!(sent(&back), [json!({ "id": 7, "ok": true, "result": {} })]);
        // A reply for a phone that left goes nowhere.
        let out = hub.phone_message(1, r#"{"id":8,"verb":"launch","args":{"id":"netflix"}}"#);
        let upstream_id = to_ctl(&out)[0]["id"].as_u64().unwrap();
        hub.closed(1);
        assert!(
            hub.ctl_line(&format!(
                r#"{{"id":{upstream_id},"ok":true,"result":{{}}}}"#
            ))
            .is_empty()
        );
    }

    #[test]
    fn a_remote_only_phone_is_held_to_its_role() {
        let mut hub = Hub::new();
        hub.ctl_line(&hello());
        hub.ws_open(1, TOKEN, device("remote"));
        let code = |hub: &mut Hub, line: &str| -> Value {
            let out = hub.phone_message(1, line);
            assert!(to_ctl(&out).is_empty(), "{line} went to lumen-ctl");
            sent(&out)[0]["error"]["code"].clone()
        };
        assert_eq!(
            code(&mut hub, r#"{"id":1,"verb":"privacy.state"}"#),
            json!("forbidden")
        );
        assert_eq!(
            code(
                &mut hub,
                r#"{"id":2,"verb":"settings.set","args":{"key":"picture.oledLight","value":1}}"#
            ),
            json!("forbidden")
        );
        // lumen-remote's own verbs are no phone's.
        for verb in [
            "pair.complete",
            "device.auth",
            "devices.revoke",
            "pair.open",
        ] {
            assert_eq!(
                code(&mut hub, &format!(r#"{{"id":3,"verb":"{verb}"}}"#)),
                json!("unknown-verb")
            );
        }
        // A phone can't bring its own token, or anything else.
        let t = format!(r#"{{"id":4,"verb":"volume.set","args":{{"level":1}},"token":"{TOKEN}"}}"#);
        assert_eq!(code(&mut hub, &t), json!("bad-line"));
        assert_eq!(code(&mut hub, "nope"), json!("bad-line"));
        assert_eq!(
            code(&mut hub, r#"{"id":0,"verb":"volume.set"}"#),
            json!("bad-line")
        );
        // The presets are allowed.
        let out = hub.phone_message(
            1,
            r#"{"id":5,"verb":"settings.set","args":{"key":"picture.mode","value":"game"}}"#,
        );
        assert_eq!(to_ctl(&out).len(), 1);
    }

    #[test]
    fn too_many_waiting_requests_are_refused() {
        let mut hub = Hub::new();
        hub.ctl_line(&hello());
        hub.ws_open(1, TOKEN, device("remote"));
        for i in 1..=32 {
            assert_eq!(
                to_ctl(&hub.phone_message(
                    1,
                    &format!(r#"{{"id":{i},"verb":"launch","args":{{"id":"x"}}}}"#)
                ))
                .len(),
                1
            );
        }
        let out = hub.phone_message(1, r#"{"id":99,"verb":"launch","args":{"id":"x"}}"#);
        assert_eq!(sent(&out)[0]["error"]["code"], json!("unavailable"));
    }

    #[test]
    fn revocation_and_role_changes_close_the_websocket() {
        let mut hub = Hub::new();
        hub.ctl_line(&hello());
        hub.ws_open(1, TOKEN, device("remote"));
        let other = "f".repeat(64);
        hub.ws_open(
            2,
            &other,
            Device {
                id: "11111111".into(),
                ..device("remote")
            },
        );
        // A cached cookie is answered at once...
        let up = to_ctl(&hub.auth(9, TOKEN))[0]["id"].as_u64().unwrap();
        let r = hub.ctl_line(&format!(r#"{{"id":{up},"ok":true,"result":{{"id":"0a1b2c3d","name":"Pixel","role":"remote","paired":"x"}}}}"#));
        assert_eq!(r, [Action::Authed(9, Some(device("remote")))]);
        assert_eq!(
            hub.auth(10, TOKEN),
            [Action::Authed(10, Some(device("remote")))]
        );
        // ...until devices change: then every socket is asked about again.
        let asks = to_ctl(&hub.ctl_line(r#"{"event":"devices","data":{}}"#));
        assert_eq!(asks.len(), 2);
        assert_eq!(to_ctl(&hub.auth(11, TOKEN)).len(), 1);
        for ask in asks {
            let id = ask["id"].as_u64().unwrap();
            let reply = if ask["token"] == json!(TOKEN) {
                format!(
                    r#"{{"id":{id},"ok":false,"error":{{"code":"forbidden","message":"revoked"}}}}"#
                )
            } else {
                format!(
                    r#"{{"id":{id},"ok":true,"result":{{"id":"11111111","name":"Tab","role":"owner","paired":"x"}}}}"#
                )
            };
            let out = hub.ctl_line(&reply);
            let close = out.iter().find_map(|a| match a {
                Action::Close(c, code, _) => Some((*c, *code)),
                _ => None,
            });
            if ask["token"] == json!(TOKEN) {
                assert_eq!(close, Some((1, CLOSE_REVOKED)));
            } else {
                assert_eq!(close, Some((2, CLOSE_ROLE)));
            }
        }
        assert_eq!(hub.clients(), 0);
    }

    #[test]
    fn pairing_answers_with_the_device_and_its_token() {
        let mut hub = Hub::new();
        let up = to_ctl(&hub.pair(3, "123456", "Pixel"))[0].clone();
        assert_eq!(up["args"], json!({ "code": "123456", "name": "Pixel" }));
        assert!(up.get("token").is_none());
        let id = up["id"].as_u64().unwrap();
        let out = hub.ctl_line(&format!(r#"{{"id":{id},"ok":true,"result":{{"id":"0a1b2c3d","name":"Pixel","role":"remote","token":"{TOKEN}"}}}}"#));
        assert_eq!(
            out,
            [Action::Paired(3, Ok((device("remote"), TOKEN.to_owned())))]
        );
        let id = to_ctl(&hub.pair(4, "000000", "x"))[0]["id"]
            .as_u64()
            .unwrap();
        let out = hub.ctl_line(&format!(r#"{{"id":{id},"ok":false,"error":{{"code":"forbidden","message":"That code is wrong."}}}}"#));
        assert_eq!(out, [Action::Paired(4, Err("That code is wrong.".into()))]);
    }
}
