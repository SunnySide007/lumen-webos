//! The bus as the parts see it, and LG's methods lumend calls.
//!
//! Every method is a constant here, and every payload is built by lumend
//! from fixed values: nothing the shell sends reaches a payload. lumend
//! never calls `com.webos.service.acr`, nor any other name it holds down:
//! a call to a dynamic service is what starts it (b9.md, "Platform").

use std::time::Duration;

use serde_json::{Value, json};

use crate::paths::{APP_ID, LG_HOME};

/// How long one call may take before it counts as failed.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const AM: &str = "luna://com.webos.applicationManager";
pub const RUNNING: &str = "luna://com.webos.applicationManager/running";
pub const FOREGROUND: &str = "luna://com.webos.applicationManager/getForegroundAppInfo";
pub const CLOSE: &str = "luna://com.webos.applicationManager/closeByAppId";
pub const LAUNCH: &str = "luna://com.webos.applicationManager/launch";
pub const LIFE_EVENTS: &str = "luna://com.webos.applicationManager/getAppLifeEvents";
pub const LAUNCH_POINTS: &str = "luna://com.webos.applicationManager/listLaunchPoints";
pub const GET_SETTINGS: &str = "luna://com.webos.settingsservice/getSystemSettings";
pub const SCREEN_OFF: &str = "luna://com.webos.service.tvpower/power/turnOffScreen";
/// The hub's own method: tells when a service connects or goes, without
/// calling (and so starting) it.
pub const SERVER_STATUS: &str = "luna://com.palm.bus/signal/registerServerStatus";

/// Every method lumend calls, for `lumend groups`, which reads which of the
/// TV's permission groups hold them, so the role asks for those only.
pub const METHODS: [&str; 9] = [
    RUNNING,
    FOREGROUND,
    CLOSE,
    LAUNCH,
    LIFE_EVENTS,
    LAUNCH_POINTS,
    GET_SETTINGS,
    SCREEN_OFF,
    SERVER_STATUS,
];

/// One way to reach luna; the tests give a fake.
pub trait Bus: Send + Sync {
    /// One call and its reply. `Err` is the bus failing; LG's own refusals
    /// are replies with `returnValue` false.
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String>;

    /// Opens a subscription whose messages arrive as `Event::Luna` with
    /// `id`.
    fn subscribe(&self, id: u32, uri: &str, payload: &Value) -> Result<(), String>;
}

impl Bus for crate::sys::luna::Luna {
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        Self::call(self, uri, payload, CALL_TIMEOUT)
    }

    fn subscribe(&self, id: u32, uri: &str, payload: &Value) -> Result<(), String> {
        Self::subscribe(self, id, uri, payload)
    }
}

/// `returnValue: true`.
pub fn ok(reply: &Value) -> bool {
    reply.get("returnValue") == Some(&Value::Bool(true))
}

/// LG's refusal in a few words.
pub fn refusal(reply: &Value) -> String {
    let text = reply
        .get("errorText")
        .and_then(Value::as_str)
        .unwrap_or("no reason given");
    let code = reply
        .get("errorCode")
        .map(Value::to_string)
        .unwrap_or_default();
    crate::log::clean(&format!("{text} {code}"))
        .trim()
        .to_owned()
}

/// A call whose reply must say `returnValue: true`.
pub fn checked(bus: &dyn Bus, uri: &str, payload: &Value) -> Result<Value, String> {
    let reply = bus.call(uri, payload)?;
    if ok(&reply) {
        Ok(reply)
    } else {
        Err(refusal(&reply))
    }
}

/// The TV's default home (route D's setting), asked as `lumen-home` asks.
pub fn default_home(bus: &dyn Bus) -> Result<String, String> {
    let reply = checked(
        bus,
        GET_SETTINGS,
        &json!({ "category": "general", "key": "defaultApps" }),
    )?;
    ["/settings/defaultApps/home", "/defaultApps/home"]
        .iter()
        .find_map(|p| reply.pointer(p).and_then(Value::as_str))
        .map(crate::log::clean)
        .ok_or_else(|| "no defaultApps.home in the reply".to_owned())
}

/// The app in front, if SAM names one.
pub fn foreground(bus: &dyn Bus) -> Result<Option<String>, String> {
    let reply = checked(bus, FOREGROUND, &json!({}))?;
    Ok(reply
        .get("appId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned))
}

/// One app in SAM's running list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    pub id: String,
    pub pid: Option<String>,
}

/// SAM's running list.
pub fn running(bus: &dyn Bus) -> Result<Vec<Running>, String> {
    let reply = checked(bus, RUNNING, &json!({}))?;
    let apps = reply
        .get("running")
        .and_then(Value::as_array)
        .ok_or("no running list in the reply")?;
    Ok(apps
        .iter()
        .filter_map(|app| {
            let id = app.get("id")?.as_str()?.to_owned();
            let pid = match app.get("processid") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            };
            Some(Running { id, pid })
        })
        .collect())
}

pub fn close_lg_home() -> (&'static str, Value) {
    (CLOSE, json!({ "id": LG_HOME, "forceMode": true }))
}

/// The shell's start, hidden behind the app in front, as lane L's shell
/// takes it (lifecycle version 1): a launch through SAM with
/// `params.hidden`, which the shell reads to do its start-up and draw
/// nothing until the first relaunch (a Home press). A shell without lane
/// L's change ignores the parameter and comes to the front, which `lumend
/// try shell` reports; fact 10 (depth.md) is whether SAM keeps it resident.
///
/// It asks for the Wayland path, the only one that starts hidden: the SDL
/// path refuses and starts shown. On the B9 on 2026-11-10 a Home press then
/// showed Lumen 216 to 218 ms after the key went down (a 180 ms hold),
/// against 843 to 864 ms with the shell closed.
pub fn launch_shell_hidden() -> (&'static str, Value) {
    (
        LAUNCH,
        json!({ "id": APP_ID, "params": { "hidden": true, "platform": "wayland" } }),
    )
}

/// SAM's own preload, as the B9's upstart task preloads LG Home: only for
/// `lumend try shell preload`, which answers depth.md's fact 10.
pub fn launch_shell_preload() -> (&'static str, Value) {
    (LAUNCH, json!({ "id": APP_ID, "preload": "full" }))
}

pub fn subscribe_life_events() -> (&'static str, Value) {
    (LIFE_EVENTS, json!({ "subscribe": true }))
}

pub fn server_status(service: &str) -> (&'static str, Value) {
    (SERVER_STATUS, json!({ "serviceName": service }))
}

pub fn screen_off() -> (&'static str, Value) {
    (SCREEN_OFF, json!({}))
}

pub fn launch_points() -> (&'static str, Value) {
    (LAUNCH_POINTS, json!({}))
}

/// A life event, as far as lumend reads one: whose, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeEvent {
    pub app: String,
    pub event: String,
    /// `homeKey` when SAM says so in `reason` or `launchType`.
    pub home_key: bool,
}

/// Reads a `getAppLifeEvents` message; `None` for the first reply and for
/// anything without an app and an event.
pub fn life_event(msg: &Value) -> Option<LifeEvent> {
    let app = msg
        .get("appId")
        .or_else(|| msg.get("id"))
        .and_then(Value::as_str)?;
    let event = msg
        .get("event")
        .or_else(|| msg.get("status"))
        .and_then(Value::as_str)?;
    let home_key = ["reason", "launchType"].iter().any(|k| {
        msg.get(*k).and_then(Value::as_str) == Some("homeKey")
            || msg
                .pointer(&format!("/parameters/{k}"))
                .and_then(Value::as_str)
                == Some("homeKey")
    });
    Some(LifeEvent {
        app: crate::log::clean(app),
        event: crate::log::clean(event),
        home_key,
    })
}

/// Whether `uri` is one of SAM's.
pub fn is_sam(uri: &str) -> bool {
    uri.starts_with(AM)
}

/// A bus that answers from a script and records what it was asked.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeBus {
        /// Replies by URI, in order; the last one repeats.
        pub replies: Mutex<HashMap<String, VecDeque<Result<Value, String>>>>,
        pub calls: Mutex<Vec<(String, Value)>>,
        pub subs: Mutex<Vec<(u32, String, Value)>>,
    }

    impl FakeBus {
        pub fn reply(&self, uri: &str, reply: Result<Value, String>) {
            self.replies
                .lock()
                .unwrap()
                .entry(uri.to_owned())
                .or_default()
                .push_back(reply);
        }

        pub fn calls_to(&self, uri: &str) -> Vec<Value> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(u, _)| u == uri)
                .map(|(_, p)| p.clone())
                .collect()
        }

        pub fn uris(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(u, _)| u.clone())
                .collect()
        }
    }

    impl Bus for FakeBus {
        fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
            self.calls
                .lock()
                .unwrap()
                .push((uri.to_owned(), payload.clone()));
            let mut replies = self.replies.lock().unwrap();
            let queue = replies
                .get_mut(uri)
                .ok_or_else(|| format!("no fake reply for {uri}"))?;
            if queue.len() > 1 {
                queue.pop_front().unwrap()
            } else {
                queue.front().cloned().unwrap()
            }
        }

        fn subscribe(&self, id: u32, uri: &str, payload: &Value) -> Result<(), String> {
            self.subs
                .lock()
                .unwrap()
                .push((id, uri.to_owned(), payload.clone()));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeBus;
    use super::*;

    #[test]
    fn no_method_lumend_calls_is_a_sender_it_holds_down() {
        for uri in METHODS {
            for sender in crate::senders::SENDERS {
                assert!(!uri.contains(sender.service), "{uri}");
            }
        }
    }

    #[test]
    fn reads_the_default_home_and_the_running_list() {
        let bus = FakeBus::default();
        bus.reply(
            GET_SETTINGS,
            Ok(json!({"returnValue": true, "settings": {"defaultApps": {"home": APP_ID}}})),
        );
        bus.reply(
            RUNNING,
            Ok(json!({"returnValue": true, "running": [
                {"id": LG_HOME, "processid": "1736"},
                {"id": "netflix", "processid": 2001},
                {"processid": "5"}
            ]})),
        );
        assert_eq!(default_home(&bus).unwrap(), APP_ID);
        let list = running(&bus).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].pid.as_deref(), Some("1736"));
        assert_eq!(list[1].pid.as_deref(), Some("2001"));
    }

    #[test]
    fn a_refusal_is_an_error_with_lgs_words() {
        let bus = FakeBus::default();
        bus.reply(
            FOREGROUND,
            Ok(json!({"returnValue": false, "errorCode": -1027, "errorText": "Invalid permissions\n"})),
        );
        assert_eq!(foreground(&bus).unwrap_err(), "Invalid permissions? -1027");
    }

    #[test]
    fn reads_life_events() {
        assert_eq!(
            life_event(&json!({"returnValue": true, "subscribed": true})),
            None
        );
        let e =
            life_event(&json!({"appId": APP_ID, "event": "launch", "reason": "homeKey"})).unwrap();
        assert!(e.home_key);
        assert_eq!(e.event, "launch");
        let e = life_event(&json!({"appId": LG_HOME, "event": "stop"})).unwrap();
        assert!(!e.home_key);
    }
}
