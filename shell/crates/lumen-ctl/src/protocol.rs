//! The Lumen Remote protocol as lumen-ctl reads and writes it: a port of
//! `remote/ui/src/protocol.ts`, which is the source of truth. Every check of
//! `parseRequest` is here in the same order with the same messages, so a
//! request the mock refuses is refused here with the same code; the tests at
//! the bottom are the mock's own cases. Where the two can differ is listed in
//! the report (serde_json against `JSON.parse`).
//!
//! A request that passes comes out typed: its values are integers, booleans,
//! names from the tables below (`&'static str`, never the client's string) or
//! text that has been checked. Nothing else of the line goes further.

use serde_json::{Map, Value, json};

pub const PROTOCOL_VERSION: u32 = 1;
/// The longest line either side accepts, in bytes of UTF-8.
pub const MAX_LINE_BYTES: usize = 65_536;
pub const MAX_REQUEST_ID: u32 = 2_147_483_647;
/// The longest text `text.insert` takes at once, in code points.
pub const MAX_INSERT: usize = 256;
/// The most characters `text.delete` removes at once.
pub const MAX_DELETE: u16 = 256;
/// The longest `toast`, in code points.
pub const MAX_TOAST: usize = 120;
/// The longest name a phone gives itself when it pairs, in code points.
pub const MAX_DEVICE_NAME: usize = 40;

/// Keys `key` can press, in protocol.ts's order.
pub const KEY_NAMES: [&str; 31] = [
    "up",
    "down",
    "left",
    "right",
    "ok",
    "back",
    "home",
    "menu",
    "play",
    "pause",
    "stop",
    "rewind",
    "fastforward",
    "info",
    "exit",
    "channelup",
    "channeldown",
    "red",
    "green",
    "yellow",
    "blue",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
];

/// LG's power states in Lumen's words.
pub const POWER_STATES: [&str; 4] = ["active", "screen-off", "screensaver", "standby"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Enum(&'static [&'static str]),
    Int { min: u8, max: u8 },
}

/// One of the settings Remote may read and write (D7).
#[derive(Debug, PartialEq, Eq)]
pub struct Setting {
    pub key: &'static str,
    pub kind: Kind,
}

/// protocol.ts's `SETTINGS`, in its order.
pub static SETTINGS: [Setting; 4] = [
    Setting {
        key: "picture.mode",
        kind: Kind::Enum(&[
            "vivid",
            "standard",
            "eco",
            "cinema-home",
            "cinema",
            "sports",
            "game",
            "filmmaker",
            "expert-bright",
            "expert-dark",
        ]),
    },
    Setting {
        key: "picture.oledLight",
        kind: Kind::Int { min: 0, max: 100 },
    },
    Setting {
        key: "picture.energySaving",
        kind: Kind::Enum(&["auto", "off", "minimum", "medium", "maximum"]),
    },
    Setting {
        key: "sound.mode",
        kind: Kind::Enum(&[
            "ai",
            "standard",
            "cinema",
            "clear-voice",
            "sports",
            "music",
            "game",
        ]),
    },
];

pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// A setting's value, taken from the table when it is a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingValue {
    Name(&'static str),
    Int(u8),
}

impl SettingValue {
    pub fn to_json(self) -> Value {
        match self {
            Self::Name(name) => json!(name),
            Self::Int(n) => json!(n),
        }
    }
}

/// What a paired device may do (decision 12): `Remote` is power, volume,
/// inputs, apps, keys, typing and picture presets; `Owner` adds privacy,
/// toasts and every setting. Only the PC, over its SSH key, grants owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Remote,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "remote" => Some(Self::Remote),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

/// Who is at the other end of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The PC, through `authorized_keys`' forced command: the owner.
    Ssh,
    /// `lumen-remote`, for the paired phones: every request carries the
    /// device's token, checked here against `devices.json`.
    Web,
}

/// Every verb and its checked arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    PowerOff,
    ScreenOff,
    ScreenOn,
    VolumeUp,
    VolumeDown,
    VolumeSet(u8),
    MuteSet(bool),
    AppsList,
    Launch(String),
    Key(&'static str),
    TextInsert(String),
    TextDelete(u16),
    TextEnter,
    LumenOpen,
    AmbientStart,
    PrivacyState,
    /// The settings asked for, or `None` for all of them.
    SettingsGet(Option<Vec<&'static Setting>>),
    SettingsSet(&'static Setting, SettingValue),
    Toast(String),
    PairOpen,
    PairClose,
    DevicesList,
    DevicesRevoke(String),
    DevicesRole(String, Role),
    /// `lumen-remote`'s only, for a phone's code.
    PairComplete {
        code: String,
        name: String,
    },
    /// `lumen-remote`'s only: whose token the request carries.
    DeviceAuth,
}

/// Every verb's name, in protocol.ts's order (`VERBS`).
pub const VERBS: [&str; 24] = [
    "power.off",
    "screen.off",
    "screen.on",
    "volume.up",
    "volume.down",
    "volume.set",
    "mute.set",
    "apps.list",
    "launch",
    "key",
    "text.insert",
    "text.delete",
    "text.enter",
    "lumen.open",
    "ambient.start",
    "privacy.state",
    "settings.get",
    "settings.set",
    "toast",
    "pair.open",
    "pair.close",
    "devices.list",
    "devices.revoke",
    "devices.role",
];

/// Verbs only the PC's SSH session takes: pairing and the devices' rights.
pub const SSH_ONLY: [&str; 5] = [
    "pair.open",
    "pair.close",
    "devices.list",
    "devices.revoke",
    "devices.role",
];

/// Verbs only `lumen-remote` sends, never a client: not in protocol.ts.
pub const WEB_ONLY: [&str; 2] = ["pair.complete", "device.auth"];

/// The settings a remote-only device may change: the presets.
pub const REMOTE_SETTINGS: [&str; 2] = ["picture.mode", "sound.mode"];

/// Whether `verb` is in a link's list.
pub fn takes(link: Link, verb: &str) -> bool {
    match link {
        Link::Ssh => VERBS.contains(&verb),
        Link::Web => {
            (VERBS.contains(&verb) && !SSH_ONLY.contains(&verb)) || WEB_ONLY.contains(&verb)
        }
    }
}

/// The verbs a role may send through `lumen-remote`, by name: the list
/// lumen-ctl gives `lumen-remote` in its hello, so the two check from one
/// table. `settings.set` is in both, and a remote-only device's is held to
/// `REMOTE_SETTINGS`.
pub fn role_verbs(role: Role) -> Vec<&'static str> {
    VERBS
        .iter()
        .copied()
        .filter(|v| !SSH_ONLY.contains(v))
        .filter(|v| role == Role::Owner || !matches!(*v, "privacy.state" | "toast"))
        .collect()
}

impl Verb {
    /// Whether a remote-only device is refused this request.
    pub fn owner_only(&self) -> bool {
        match self {
            Self::PrivacyState | Self::Toast(_) => true,
            Self::SettingsSet(s, _) => !REMOTE_SETTINGS.contains(&s.key),
            Self::PairOpen
            | Self::PairClose
            | Self::DevicesList
            | Self::DevicesRevoke(_)
            | Self::DevicesRole(..) => true,
            _ => false,
        }
    }
}

impl Verb {
    pub fn name(&self) -> &'static str {
        match self {
            Self::PowerOff => "power.off",
            Self::ScreenOff => "screen.off",
            Self::ScreenOn => "screen.on",
            Self::VolumeUp => "volume.up",
            Self::VolumeDown => "volume.down",
            Self::VolumeSet(_) => "volume.set",
            Self::MuteSet(_) => "mute.set",
            Self::AppsList => "apps.list",
            Self::Launch(_) => "launch",
            Self::Key(_) => "key",
            Self::TextInsert(_) => "text.insert",
            Self::TextDelete(_) => "text.delete",
            Self::TextEnter => "text.enter",
            Self::LumenOpen => "lumen.open",
            Self::AmbientStart => "ambient.start",
            Self::PrivacyState => "privacy.state",
            Self::SettingsGet(_) => "settings.get",
            Self::SettingsSet(..) => "settings.set",
            Self::Toast(_) => "toast",
            Self::PairOpen => "pair.open",
            Self::PairClose => "pair.close",
            Self::DevicesList => "devices.list",
            Self::DevicesRevoke(_) => "devices.revoke",
            Self::DevicesRole(..) => "devices.role",
            Self::PairComplete { .. } => "pair.complete",
            Self::DeviceAuth => "device.auth",
        }
    }

    /// The arguments for the log, rebuilt from the typed values. Text the
    /// owner typed (`text.insert`, `toast`) is never shown.
    pub fn log_args(&self) -> String {
        match self {
            Self::VolumeSet(level) => json!({ "level": level }).to_string(),
            Self::MuteSet(muted) => json!({ "muted": muted }).to_string(),
            Self::Launch(id) => json!({ "id": id }).to_string(),
            Self::Key(name) => json!({ "name": name }).to_string(),
            Self::TextDelete(count) => json!({ "count": count }).to_string(),
            Self::TextInsert(_) | Self::Toast(_) => "(text withheld)".to_owned(),
            Self::PairComplete { .. } => "(code and name withheld)".to_owned(),
            Self::DevicesRevoke(id) => json!({ "id": id }).to_string(),
            Self::DevicesRole(id, role) => json!({ "id": id, "role": role.as_str() }).to_string(),
            Self::SettingsGet(Some(keys)) => {
                json!({ "keys": keys.iter().map(|s| s.key).collect::<Vec<_>>() }).to_string()
            }
            Self::SettingsSet(s, v) => json!({ "key": s.key, "value": v.to_json() }).to_string(),
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Not JSON, not an object, too long, or no usable id.
    BadLine,
    UnknownVerb,
    BadArgs,
    /// No app, input or setting by that name on the TV.
    NotFound,
    /// A known verb the TV can't do now (no route for keys, no such mode).
    Unavailable,
    /// LG's service refused or didn't answer.
    Failed,
    /// The device isn't paired, or its role doesn't allow the verb.
    Forbidden,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadLine => "bad-line",
            Self::UnknownVerb => "unknown-verb",
            Self::BadArgs => "bad-args",
            Self::NotFound => "not-found",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Forbidden => "forbidden",
        }
    }
}

/// Why a request got no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: ErrorCode,
    pub message: String,
}

impl Refusal {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u32,
    pub verb: Verb,
    /// The device's token, on the web link only (64 hex digits).
    pub token: Option<String>,
}

/// A line refused before it reached a verb; `id` is `None` when the line had
/// no usable one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub id: Option<u32>,
    pub refusal: Refusal,
}

fn refuse(id: Option<u32>, code: ErrorCode, message: impl Into<String>) -> Refused {
    Refused {
        id,
        refusal: Refusal::new(code, message),
    }
}

/// protocol.ts's `quote`: a value as the messages show it, cut to 40 UTF-16
/// units as JavaScript's `slice` counts them.
fn quote(v: Option<&Value>) -> String {
    let s = match v {
        None => "undefined".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let s = if s.encode_utf16().count() > 40 {
        let mut units = 0;
        let mut cut: String = s
            .chars()
            .take_while(|c| {
                units += c.len_utf16();
                units <= 40
            })
            .collect();
        cut.push('…');
        cut
    } else {
        s
    };
    Value::String(s).to_string()
}

/// JavaScript's `Number.isInteger(v) && v >= min && v <= max`: `1.0` and
/// `1e2` are integers there, so they are here.
fn int_in(v: Option<&Value>, min: i64, max: i64) -> Option<i64> {
    let Some(Value::Number(n)) = v else {
        return None;
    };
    if let Some(i) = n.as_i64() {
        return (min..=max).contains(&i).then_some(i);
    }
    if n.is_u64() {
        // Above i64::MAX, so above every range here.
        return None;
    }
    let f = n.as_f64()?;
    // Every bound here is far below 2^53, so the casts are exact.
    (f.fract() == 0.0 && f >= min as f64 && f <= max as f64).then_some(f as i64)
}

/// protocol.ts's `onlyKeys`: the first key not allowed, by the map's order.
fn only_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match args.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!(
            "unexpected argument {}",
            quote(Some(&Value::String(k.clone())))
        )),
        None => Ok(()),
    }
}

fn is_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
}

/// protocol.ts's `checkText`. A Rust string is always well-formed Unicode: a
/// lone surrogate fails earlier, in the JSON.
fn check_text(name: &str, v: Option<&Value>, max: usize) -> Result<String, String> {
    let Some(Value::String(s)) = v else {
        return Err(format!("{name} must be a string"));
    };
    let n = s.chars().count();
    if n < 1 || n > max {
        return Err(format!("{name} must be 1 to {max} characters"));
    }
    if s.chars().any(is_control) {
        return Err(format!("{name} must not contain control characters"));
    }
    Ok(s.clone())
}

/// App and input ids: `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`.
pub fn is_app_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// protocol.ts's `checkSetting`, giving the value from the table.
pub fn check_setting(
    key: Option<&Value>,
    value: Option<&Value>,
) -> Result<(&'static Setting, SettingValue), String> {
    let Some(spec) = key.and_then(Value::as_str).and_then(setting) else {
        return Err(format!("unknown setting {}", quote(key)));
    };
    match spec.kind {
        Kind::Int { min, max } => int_in(value, i64::from(min), i64::from(max))
            .map(|n| (spec, SettingValue::Int(n as u8)))
            .ok_or_else(|| format!("{} must be an integer from {min} to {max}", spec.key)),
        Kind::Enum(values) => value
            .and_then(Value::as_str)
            .and_then(|v| values.iter().find(|&&name| name == v))
            .map(|&name| (spec, SettingValue::Name(name)))
            .ok_or_else(|| format!("{} must be one of {}", spec.key, values.join(", "))),
    }
}

/// A device id: eight lower-case hex digits.
fn device_id(v: Option<&Value>) -> Result<String, String> {
    match v.and_then(Value::as_str) {
        Some(id) if crate::devices::is_device_id(id) => Ok(id.to_owned()),
        _ => Err("id must be a device id".to_owned()),
    }
}

/// protocol.ts's `checkArgs` for one verb, giving it typed.
fn check_args(verb: &str, args: &Map<String, Value>) -> Result<Verb, String> {
    let none = |v: Verb| only_keys(args, &[]).map(|()| v);
    match verb {
        "power.off" => none(Verb::PowerOff),
        "screen.off" => none(Verb::ScreenOff),
        "screen.on" => none(Verb::ScreenOn),
        "volume.up" => none(Verb::VolumeUp),
        "volume.down" => none(Verb::VolumeDown),
        "volume.set" => {
            only_keys(args, &["level"])?;
            int_in(args.get("level"), 0, 100)
                .map(|n| Verb::VolumeSet(n as u8))
                .ok_or_else(|| "level must be an integer from 0 to 100".to_owned())
        }
        "mute.set" => {
            only_keys(args, &["muted"])?;
            args.get("muted")
                .and_then(Value::as_bool)
                .map(Verb::MuteSet)
                .ok_or_else(|| "muted must be true or false".to_owned())
        }
        "apps.list" => none(Verb::AppsList),
        "launch" => {
            only_keys(args, &["id"])?;
            match args.get("id").and_then(Value::as_str) {
                Some(id) if is_app_id(id) => Ok(Verb::Launch(id.to_owned())),
                _ => Err("id must be an app or input id".to_owned()),
            }
        }
        "key" => {
            only_keys(args, &["name"])?;
            args.get("name")
                .and_then(Value::as_str)
                .and_then(|n| KEY_NAMES.iter().find(|&&k| k == n))
                .map(|&k| Verb::Key(k))
                .ok_or_else(|| format!("name must be one of {}", KEY_NAMES.join(", ")))
        }
        "text.insert" => {
            only_keys(args, &["text"])?;
            check_text("text", args.get("text"), MAX_INSERT).map(Verb::TextInsert)
        }
        "text.delete" => {
            only_keys(args, &["count"])?;
            int_in(args.get("count"), 1, i64::from(MAX_DELETE))
                .map(|n| Verb::TextDelete(n as u16))
                .ok_or_else(|| format!("count must be an integer from 1 to {MAX_DELETE}"))
        }
        "text.enter" => none(Verb::TextEnter),
        "lumen.open" => none(Verb::LumenOpen),
        "ambient.start" => none(Verb::AmbientStart),
        "privacy.state" => none(Verb::PrivacyState),
        "settings.get" => {
            only_keys(args, &["keys"])?;
            let Some(keys) = args.get("keys") else {
                return Ok(Verb::SettingsGet(None));
            };
            let list = keys
                .as_array()
                .filter(|l| !l.is_empty() && l.len() <= SETTINGS.len())
                .ok_or_else(|| {
                    format!("keys must be a list of 1 to {} settings", SETTINGS.len())
                })?;
            list.iter()
                .map(|k| {
                    k.as_str()
                        .and_then(setting)
                        .ok_or_else(|| format!("unknown setting {}", quote(Some(k))))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|keys| Verb::SettingsGet(Some(keys)))
        }
        "settings.set" => {
            only_keys(args, &["key", "value"])?;
            check_setting(args.get("key"), args.get("value"))
                .map(|(spec, value)| Verb::SettingsSet(spec, value))
        }
        "toast" => {
            only_keys(args, &["text"])?;
            check_text("text", args.get("text"), MAX_TOAST).map(Verb::Toast)
        }
        "pair.open" => none(Verb::PairOpen),
        "pair.close" => none(Verb::PairClose),
        "devices.list" => none(Verb::DevicesList),
        "devices.revoke" => {
            only_keys(args, &["id"])?;
            device_id(args.get("id")).map(Verb::DevicesRevoke)
        }
        "devices.role" => {
            only_keys(args, &["id", "role"])?;
            let id = device_id(args.get("id"))?;
            args.get("role")
                .and_then(Value::as_str)
                .and_then(Role::parse)
                .map(|role| Verb::DevicesRole(id, role))
                .ok_or_else(|| "role must be remote or owner".to_owned())
        }
        "pair.complete" => {
            only_keys(args, &["code", "name"])?;
            let code = match args.get("code").and_then(Value::as_str) {
                Some(c) if c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()) => c.to_owned(),
                _ => return Err("code must be six digits".to_owned()),
            };
            let name = check_text("name", args.get("name"), MAX_DEVICE_NAME)?;
            Ok(Verb::PairComplete { code, name })
        }
        "device.auth" => none(Verb::DeviceAuth),
        _ => unreachable!("check_args is only called for a known verb"),
    }
}

/// protocol.ts's `parseRequest`: every field typed and in range, checked in
/// the same order. On the web link a request may carry `token`, and takes
/// `lumen-remote`'s two verbs instead of the PC's five.
pub fn parse_request(line: &[u8], link: Link) -> Result<Request, Refused> {
    use ErrorCode::*;
    if line.len() > MAX_LINE_BYTES {
        return Err(refuse(
            None,
            BadLine,
            format!("line is longer than {MAX_LINE_BYTES} bytes"),
        ));
    }
    let value: Value =
        serde_json::from_slice(line).map_err(|_| refuse(None, BadLine, "not JSON"))?;
    let Value::Object(obj) = value else {
        return Err(refuse(None, BadLine, "not a JSON object"));
    };
    let Some(id) = int_in(obj.get("id"), 1, i64::from(MAX_REQUEST_ID)) else {
        return Err(refuse(
            None,
            BadLine,
            format!("id must be an integer from 1 to {MAX_REQUEST_ID}"),
        ));
    };
    let id = id as u32;
    let fields: &[&str] = match link {
        Link::Ssh => &["id", "verb", "args"],
        Link::Web => &["id", "verb", "args", "token"],
    };
    if let Err(extra) = only_keys(&obj, fields) {
        return Err(refuse(
            Some(id),
            BadLine,
            extra.replacen("argument", "field", 1),
        ));
    }
    let token = match obj.get("token") {
        None => None,
        Some(Value::String(t)) if crate::devices::is_hex(t, crate::devices::TOKEN_HEX) => {
            Some(t.clone())
        }
        Some(_) => return Err(refuse(Some(id), BadLine, "token must be 64 hex digits")),
    };
    let verb = match obj.get("verb") {
        Some(Value::String(v)) if takes(link, v) => v.as_str(),
        other => {
            return Err(refuse(
                Some(id),
                UnknownVerb,
                format!("unknown verb {}", quote(other)),
            ));
        }
    };
    // `value.args ?? {}`: null counts as absent.
    let empty = Map::new();
    let args = match obj.get("args") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(args)) => args,
        Some(_) => return Err(refuse(Some(id), BadArgs, "args must be an object")),
    };
    let verb = check_args(verb, args).map_err(|problem| refuse(Some(id), BadArgs, problem))?;
    Ok(Request { id, verb, token })
}

/// `{"id": …, "ok": true, "result": …}`.
pub fn reply_ok(id: u32, result: Value) -> String {
    json!({ "id": id, "ok": true, "result": result }).to_string()
}

/// `{"id": … | null, "ok": false, "error": {"code": …, "message": …}}`.
pub fn reply_err(id: Option<u32>, refusal: &Refusal) -> String {
    json!({
        "id": id,
        "ok": false,
        "error": { "code": refusal.code.as_str(), "message": refusal.message },
    })
    .to_string()
}

/// `{"event": …, "data": …}`.
pub fn event(name: &str, data: Value) -> String {
    json!({ "event": name, "data": data }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(verb: &str, args: &Value) -> Result<Verb, String> {
        let Value::Object(map) = args else {
            return Err("args must be an object".to_owned());
        };
        check_args(verb, map)
    }

    /// protocol.test.ts's GOOD: one good set of arguments per verb.
    const GOOD: [(&str, &str); 24] = [
        ("power.off", "{}"),
        ("screen.off", "{}"),
        ("screen.on", "{}"),
        ("volume.up", "{}"),
        ("volume.down", "{}"),
        ("volume.set", r#"{"level":20}"#),
        ("mute.set", r#"{"muted":true}"#),
        ("apps.list", "{}"),
        ("launch", r#"{"id":"com.webos.app.hdmi1"}"#),
        ("key", r#"{"name":"ok"}"#),
        ("text.insert", r#"{"text":"héllo 👋"}"#),
        ("text.delete", r#"{"count":3}"#),
        ("text.enter", "{}"),
        ("lumen.open", "{}"),
        ("ambient.start", "{}"),
        ("privacy.state", "{}"),
        ("settings.get", r#"{"keys":["picture.mode"]}"#),
        ("settings.set", r#"{"key":"picture.oledLight","value":80}"#),
        ("toast", r#"{"text":"Volume 20"}"#),
        ("pair.open", "{}"),
        ("pair.close", "{}"),
        ("devices.list", "{}"),
        ("devices.revoke", r#"{"id":"0a1b2c3d"}"#),
        ("devices.role", r#"{"id":"0a1b2c3d","role":"owner"}"#),
    ];

    /// protocol.test.ts's BAD, with the TypeScript's own cases for strings
    /// written as JSON (its "\ud800" is a lone surrogate, refused by
    /// serde_json as a whole line: see `a_lone_surrogate_is_refused`).
    const BAD: &[(&str, &str)] = &[
        ("power.off", r#"{"now":true}"#),
        ("volume.set", "{}"),
        ("volume.set", r#"{"level":-1}"#),
        ("volume.set", r#"{"level":101}"#),
        ("volume.set", r#"{"level":2.5}"#),
        ("volume.set", r#"{"level":"20"}"#),
        ("volume.set", r#"{"level":20,"extra":1}"#),
        ("mute.set", "{}"),
        ("mute.set", r#"{"muted":"yes"}"#),
        ("mute.set", r#"{"muted":1}"#),
        ("launch", "{}"),
        ("launch", r#"{"id":""}"#),
        ("launch", r#"{"id":"../etc/passwd"}"#),
        ("launch", r#"{"id":"a b"}"#),
        ("launch", r#"{"id":5}"#),
        ("key", r#"{"name":"power"}"#),
        ("key", r#"{"name":"OK"}"#),
        ("key", "{}"),
        ("text.insert", r#"{"text":""}"#),
        ("text.insert", r#"{"text":"a\nb"}"#),
        ("text.insert", r#"{"text":"\t"}"#),
        ("text.insert", r#"{"text":1}"#),
        ("text.delete", r#"{"count":0}"#),
        ("text.delete", r#"{"count":257}"#),
        ("text.delete", "{}"),
        ("text.delete", r#"{"count":1.5}"#),
        ("settings.get", r#"{"keys":[]}"#),
        ("settings.get", r#"{"keys":["picture.mode","nope"]}"#),
        ("settings.get", r#"{"keys":"picture.mode"}"#),
        ("settings.set", r#"{"key":"picture.mode","value":"loud"}"#),
        ("settings.set", r#"{"key":"picture.oledLight","value":101}"#),
        (
            "settings.set",
            r#"{"key":"picture.oledLight","value":"80"}"#,
        ),
        ("settings.set", r#"{"key":"backlight","value":1}"#),
        ("settings.set", r#"{"key":"picture.mode"}"#),
        ("toast", r#"{"text":""}"#),
        ("pair.open", r#"{"code":"123456"}"#),
        ("devices.revoke", "{}"),
        ("devices.revoke", r#"{"id":"0A1B2C3D"}"#),
        ("devices.revoke", r#"{"id":"0a1b2c3"}"#),
        ("devices.role", r#"{"id":"0a1b2c3d","role":"admin"}"#),
        ("devices.role", r#"{"id":"0a1b2c3d"}"#),
        ("pair.complete", r#"{"code":"12345","name":"Pixel"}"#),
        ("pair.complete", r#"{"code":"12345a","name":"Pixel"}"#),
        ("pair.complete", r#"{"code":123456,"name":"Pixel"}"#),
        ("pair.complete", r#"{"code":"123456","name":""}"#),
        ("pair.complete", r#"{"code":"123456"}"#),
        ("device.auth", r#"{"token":"x"}"#),
    ];

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn the_verbs_are_exactly_remote_mds_table() {
        let mut verbs = VERBS.to_vec();
        verbs.sort_unstable();
        let mut want = vec![
            "power.off",
            "screen.off",
            "screen.on",
            "volume.up",
            "volume.down",
            "volume.set",
            "mute.set",
            "apps.list",
            "launch",
            "key",
            "text.insert",
            "text.enter",
            "text.delete",
            "lumen.open",
            "ambient.start",
            "privacy.state",
            "settings.get",
            "settings.set",
            "toast",
            "pair.open",
            "pair.close",
            "devices.list",
            "devices.revoke",
            "devices.role",
        ];
        want.sort_unstable();
        assert_eq!(verbs, want);
        // Each verb's typed form names itself.
        for (verb, args) in GOOD {
            assert_eq!(args_of(verb, &parse(args)).unwrap().name(), verb);
        }
    }

    #[test]
    fn every_verb_takes_its_good_arguments() {
        for (verb, args) in GOOD {
            assert!(args_of(verb, &parse(args)).is_ok(), "{verb} {args}");
        }
        assert_eq!(
            args_of("text.insert", &parse(r#"{"text":"héllo 👋"}"#)),
            Ok(Verb::TextInsert("héllo 👋".to_owned()))
        );
        assert_eq!(
            args_of(
                "settings.set",
                &parse(r#"{"key":"picture.oledLight","value":80}"#)
            ),
            Ok(Verb::SettingsSet(&SETTINGS[1], SettingValue::Int(80)))
        );
    }

    #[test]
    fn every_verb_refuses_its_bad_arguments() {
        for (verb, args) in BAD {
            assert!(args_of(verb, &parse(args)).is_err(), "{verb} took {args}");
        }
        // The long ones, built.
        let long_id = json!({ "id": "x".repeat(129) });
        assert!(args_of("launch", &long_id).is_err());
        let long_text = json!({ "text": "x".repeat(257) });
        assert!(args_of("text.insert", &long_text).is_err());
        let long_toast = json!({ "text": "x".repeat(121) });
        assert!(args_of("toast", &long_toast).is_err());
        // 256 code points of a four-byte character is still 256.
        let wide = json!({ "text": "👋".repeat(256) });
        assert!(args_of("text.insert", &wide).is_ok());
        // C1 controls too, and DEL.
        for c in ["\u{7f}", "\u{85}", "\u{9f}"] {
            assert!(
                args_of("text.insert", &json!({ "text": c })).is_err(),
                "{c:?}"
            );
        }
    }

    #[test]
    fn the_messages_are_protocol_ts_s() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "power.off",
                r#"{"now":true}"#,
                r#"unexpected argument "now""#,
            ),
            (
                "volume.set",
                r#"{"level":101}"#,
                "level must be an integer from 0 to 100",
            ),
            ("mute.set", "{}", "muted must be true or false"),
            ("launch", r#"{"id":"a b"}"#, "id must be an app or input id"),
            (
                "key",
                r#"{"name":"power"}"#,
                "name must be one of up, down, left, right, ok, back, home, menu, play, pause, stop, rewind, fastforward, info, exit, channelup, channeldown, red, green, yellow, blue, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9",
            ),
            ("text.insert", r#"{"text":1}"#, "text must be a string"),
            (
                "text.insert",
                r#"{"text":""}"#,
                "text must be 1 to 256 characters",
            ),
            (
                "text.insert",
                r#"{"text":"a\nb"}"#,
                "text must not contain control characters",
            ),
            (
                "text.delete",
                r#"{"count":0}"#,
                "count must be an integer from 1 to 256",
            ),
            (
                "settings.get",
                r#"{"keys":[]}"#,
                "keys must be a list of 1 to 4 settings",
            ),
            (
                "settings.get",
                r#"{"keys":["nope"]}"#,
                r#"unknown setting "nope""#,
            ),
            (
                "settings.set",
                r#"{"key":"backlight","value":1}"#,
                r#"unknown setting "backlight""#,
            ),
            (
                "settings.set",
                r#"{"value":1}"#,
                r#"unknown setting "undefined""#,
            ),
            (
                "settings.set",
                r#"{"key":"picture.oledLight","value":"80"}"#,
                "picture.oledLight must be an integer from 0 to 100",
            ),
            (
                "settings.set",
                r#"{"key":"picture.mode","value":"loud"}"#,
                "picture.mode must be one of vivid, standard, eco, cinema-home, cinema, sports, game, filmmaker, expert-bright, expert-dark",
            ),
            (
                "toast",
                r#"{"text":""}"#,
                "text must be 1 to 120 characters",
            ),
        ];
        for (verb, args, message) in cases {
            assert_eq!(
                args_of(verb, &parse(args)).unwrap_err(),
                *message,
                "{verb} {args}"
            );
        }
    }

    #[test]
    fn takes_every_key_name_and_every_value_of_every_setting() {
        for name in KEY_NAMES {
            assert_eq!(
                args_of("key", &json!({ "name": name })),
                Ok(Verb::Key(name))
            );
        }
        for s in &SETTINGS {
            let values: Vec<Value> = match s.kind {
                Kind::Enum(values) => values.iter().map(|v| json!(v)).collect(),
                Kind::Int { min, max } => vec![json!(min), json!(max)],
            };
            for value in values {
                let args = json!({ "key": s.key, "value": value });
                assert!(args_of("settings.set", &args).is_ok(), "{args}");
            }
        }
    }

    #[test]
    fn integers_are_javascripts() {
        assert_eq!(
            args_of("volume.set", &parse(r#"{"level":20.0}"#)),
            Ok(Verb::VolumeSet(20))
        );
        assert_eq!(
            args_of("volume.set", &parse(r#"{"level":2e1}"#)),
            Ok(Verb::VolumeSet(20))
        );
        assert_eq!(
            args_of("volume.set", &parse(r#"{"level":-0}"#)),
            Ok(Verb::VolumeSet(0))
        );
        assert!(args_of("volume.set", &parse(r#"{"level":18446744073709551615}"#)).is_err());
        assert!(args_of("volume.set", &parse(r#"{"level":true}"#)).is_err());
    }

    fn refused(line: &str) -> Refused {
        parse_request(line.as_bytes(), Link::Ssh).unwrap_err()
    }

    #[test]
    fn reads_a_request_with_or_without_args() {
        assert_eq!(
            parse_request(
                br#"{"id":1,"verb":"volume.set","args":{"level":7}}"#,
                Link::Ssh
            ),
            Ok(Request {
                id: 1,
                verb: Verb::VolumeSet(7),
                token: None
            })
        );
        assert_eq!(
            parse_request(br#"{"id":2,"verb":"volume.up"}"#, Link::Ssh),
            Ok(Request {
                id: 2,
                verb: Verb::VolumeUp,
                token: None
            })
        );
        // `args ?? {}`: null is no arguments.
        assert_eq!(
            parse_request(br#"{"id":3,"verb":"volume.up","args":null}"#, Link::Ssh),
            Ok(Request {
                id: 3,
                verb: Verb::VolumeUp,
                token: None
            })
        );
        // The largest id, and one written as a float.
        assert_eq!(
            parse_request(br#"{"id":2147483647,"verb":"apps.list"}"#, Link::Ssh).map(|r| r.id),
            Ok(MAX_REQUEST_ID)
        );
        assert_eq!(
            parse_request(br#"{"id":4.0,"verb":"apps.list"}"#, Link::Ssh).map(|r| r.id),
            Ok(4)
        );
    }

    #[test]
    fn refuses_bad_lines_with_the_mocks_codes() {
        use ErrorCode::*;
        let cases: &[(&str, Option<u32>, ErrorCode)] = &[
            ("not json", None, BadLine),
            ("[1,2]", None, BadLine),
            (r#"{"verb":"volume.up"}"#, None, BadLine),
            (r#"{"id":0,"verb":"volume.up"}"#, None, BadLine),
            (r#"{"id":1.5,"verb":"volume.up"}"#, None, BadLine),
            (r#"{"id":2147483648,"verb":"volume.up"}"#, None, BadLine),
            (r#"{"id":"1","verb":"volume.up"}"#, None, BadLine),
            (
                r#"{"id":3,"verb":"volume.up","sudo":true}"#,
                Some(3),
                BadLine,
            ),
            (r#"{"id":4,"verb":"rm -rf"}"#, Some(4), UnknownVerb),
            (r#"{"id":4,"verb":5}"#, Some(4), UnknownVerb),
            (r#"{"id":4}"#, Some(4), UnknownVerb),
            (
                r#"{"id":5,"verb":"volume.set","args":[50]}"#,
                Some(5),
                BadArgs,
            ),
            (
                r#"{"id":6,"verb":"volume.set","args":{"level":500}}"#,
                Some(6),
                BadArgs,
            ),
        ];
        for (line, id, code) in cases {
            let r = refused(line);
            assert_eq!((r.id, r.refusal.code), (*id, *code), "{line}");
        }
        assert_eq!(
            refused(r#"{"id":3,"verb":"volume.up","sudo":true}"#)
                .refusal
                .message,
            r#"unexpected field "sudo""#
        );
        assert_eq!(refused("not json").refusal.message, "not JSON");
        assert_eq!(refused("7").refusal.message, "not a JSON object");
        assert_eq!(
            refused(r#"{"id":4}"#).refusal.message,
            r#"unknown verb "undefined""#
        );
        assert_eq!(
            refused(r#"{"id":4,"verb":5}"#).refusal.message,
            r#"unknown verb "5""#
        );
        assert_eq!(
            refused(r#"{"id":5,"verb":"volume.set","args":[50]}"#)
                .refusal
                .message,
            "args must be an object"
        );
    }

    #[test]
    fn refuses_a_line_over_the_limit_before_parsing_it() {
        let line = format!(
            r#"{{"id":1,"verb":"toast","args":{{"text":"{}"}}}}"#,
            "é".repeat(MAX_LINE_BYTES / 2)
        );
        let r = refused(&line);
        assert_eq!((r.id, r.refusal.code), (None, ErrorCode::BadLine));
        assert_eq!(r.refusal.message, "line is longer than 65536 bytes");
        // Exactly at the limit is read (and then refused for its text).
        let pad = MAX_LINE_BYTES - r#"{"id":1,"verb":"nope","x":""}"#.len();
        let line = format!(r#"{{"id":1,"verb":"nope","x":"{}"}}"#, "a".repeat(pad));
        assert_eq!(line.len(), MAX_LINE_BYTES);
        assert_eq!(refused(&line).id, Some(1));
    }

    #[test]
    fn does_not_echo_a_long_verb_back_whole() {
        let line = json!({ "id": 1, "verb": "v".repeat(1000) }).to_string();
        let r = refused(&line);
        assert!(
            r.refusal.message.chars().count() < 80,
            "{}",
            r.refusal.message
        );
        assert_eq!(
            r.refusal.message,
            format!("unknown verb \"{}…\"", "v".repeat(40))
        );
        // Cut by UTF-16 units, as JavaScript's slice: 20 emoji are 40 units.
        let line = json!({ "id": 1, "verb": "👋".repeat(25) }).to_string();
        assert_eq!(
            refused(&line).refusal.message,
            format!("unknown verb \"{}…\"", "👋".repeat(20))
        );
    }

    #[test]
    fn a_lone_surrogate_is_refused() {
        // TypeScript refuses it as bad-args (not well-formed); serde_json
        // cannot hold one in a string, so here it is a bad line.
        let r = refused(r#"{"id":1,"verb":"text.insert","args":{"text":"\ud800"}}"#);
        assert_eq!(r.refusal.code, ErrorCode::BadLine);
        // Invalid UTF-8 likewise.
        assert_eq!(
            parse_request(
                b"{\"id\":1,\"verb\":\"toast\",\"args\":{\"text\":\"\xff\"}}",
                Link::Ssh
            )
            .unwrap_err()
            .refusal
            .code,
            ErrorCode::BadLine
        );
    }

    #[test]
    fn app_ids_are_protocol_ts_s_pattern() {
        for good in [
            "netflix",
            "com.webos.app.hdmi1",
            "youtube.leanback.v4",
            "cdp-30",
            "a",
            "A_b-c.d",
        ] {
            assert!(is_app_id(good), "{good}");
        }
        let long = "x".repeat(129);
        for bad in [
            "",
            ".hidden",
            "-x",
            "a b",
            "a/b",
            "../etc",
            "a;b",
            "é",
            long.as_str(),
        ] {
            assert!(!is_app_id(bad), "{bad}");
        }
        assert!(is_app_id(&"x".repeat(128)));
    }

    #[test]
    fn replies_and_events_have_the_protocols_shape() {
        assert_eq!(
            parse(&reply_ok(7, json!({}))),
            json!({ "id": 7, "ok": true, "result": {} })
        );
        assert_eq!(
            parse(&reply_err(
                None,
                &Refusal::new(ErrorCode::BadLine, "not JSON")
            )),
            json!({ "id": null, "ok": false, "error": { "code": "bad-line", "message": "not JSON" } })
        );
        assert_eq!(
            parse(&event("volume", json!({ "level": 20, "muted": false }))),
            json!({ "event": "volume", "data": { "level": 20, "muted": false } })
        );
    }

    #[test]
    fn the_log_never_shows_typed_text() {
        assert_eq!(
            Verb::TextInsert("my password".to_owned()).log_args(),
            "(text withheld)"
        );
        assert_eq!(Verb::Toast("hi".to_owned()).log_args(), "(text withheld)");
        assert_eq!(Verb::VolumeSet(20).log_args(), r#"{"level":20}"#);
        assert_eq!(
            Verb::SettingsSet(&SETTINGS[0], SettingValue::Name("cinema")).log_args(),
            r#"{"key":"picture.mode","value":"cinema"}"#
        );
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn the_web_link_takes_a_token_and_lumen_remotes_verbs() {
        let line = format!(r#"{{"id":1,"verb":"volume.up","token":"{TOKEN}"}}"#);
        assert_eq!(
            parse_request(line.as_bytes(), Link::Web),
            Ok(Request {
                id: 1,
                verb: Verb::VolumeUp,
                token: Some(TOKEN.to_owned())
            })
        );
        // The PC's session takes no token field.
        let r = parse_request(line.as_bytes(), Link::Ssh).unwrap_err();
        assert_eq!(r.refusal.message, r#"unexpected field "token""#);
        // A token is 64 lower-case hex digits.
        for bad in [
            "\"short\"",
            "1",
            "null",
            &format!("\"{}\"", TOKEN.to_uppercase()),
        ] {
            let line = format!(r#"{{"id":1,"verb":"volume.up","token":{bad}}}"#);
            let r = parse_request(line.as_bytes(), Link::Web).unwrap_err();
            assert_eq!(r.refusal.message, "token must be 64 hex digits", "{bad}");
        }
        // Pairing and the devices' rights are the PC's; pair.complete and
        // device.auth are lumen-remote's.
        for verb in SSH_ONLY {
            let line = format!(r#"{{"id":2,"verb":"{verb}"}}"#);
            assert!(
                parse_request(line.as_bytes(), Link::Ssh).is_ok() || verb.starts_with("devices.")
            );
            let r = parse_request(line.as_bytes(), Link::Web).unwrap_err();
            assert_eq!(r.refusal.code, ErrorCode::UnknownVerb, "{verb}");
        }
        for verb in WEB_ONLY {
            let line = format!(r#"{{"id":3,"verb":"{verb}"}}"#);
            let r = parse_request(line.as_bytes(), Link::Ssh).unwrap_err();
            assert_eq!(r.refusal.code, ErrorCode::UnknownVerb, "{verb}");
        }
        assert_eq!(
            parse_request(
                br#"{"id":4,"verb":"pair.complete","args":{"code":"012345","name":"Pixel 9"}}"#,
                Link::Web
            )
            .map(|r| r.verb),
            Ok(Verb::PairComplete {
                code: "012345".to_owned(),
                name: "Pixel 9".to_owned()
            })
        );
    }

    #[test]
    fn remote_only_devices_get_decision_12s_verbs() {
        let remote = role_verbs(Role::Remote);
        let owner = role_verbs(Role::Owner);
        for verb in [
            "power.off",
            "volume.set",
            "launch",
            "key",
            "text.insert",
            "settings.set",
        ] {
            assert!(remote.contains(&verb), "{verb}");
        }
        for verb in ["privacy.state", "toast"] {
            assert!(!remote.contains(&verb) && owner.contains(&verb), "{verb}");
        }
        for verb in SSH_ONLY.iter().chain(WEB_ONLY.iter()) {
            assert!(!owner.contains(verb), "{verb}");
        }
        let set = |key: &str, value: Value| {
            check_setting(Some(&json!(key)), Some(&value))
                .map(|(s, v)| Verb::SettingsSet(s, v))
                .unwrap()
        };
        assert!(!set("picture.mode", json!("cinema")).owner_only());
        assert!(!set("sound.mode", json!("music")).owner_only());
        assert!(set("picture.oledLight", json!(40)).owner_only());
        assert!(set("picture.energySaving", json!("off")).owner_only());
        assert!(Verb::PrivacyState.owner_only() && Verb::PairOpen.owner_only());
        assert!(!Verb::VolumeUp.owner_only() && !Verb::DeviceAuth.owner_only());
    }
}
