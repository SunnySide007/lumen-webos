//! What the launch argument says. webOS passes a native app one JSON
//! argument, in one of two shapes (sam, 2018; the B9's argv for version 1):
//!
//! * lifecycle version 1: the launch parameters themselves, with SAM's
//!   `"nid"` and `"@system_native_app"` added;
//! * version 2: an envelope, `{"event":"launch","reason":…,
//!   "interfaceVersion":2,"interfaceMethod":"registerApp",
//!   "parameters":{…},"preload":…}`.
//!
//! `Launch::read` takes either apart, and `Params` reads the platform
//! layer's own parameters: which platform, an ES 3 context, the GPU probe,
//! the overlay trial's window type, a hidden start, a time limit and the
//! bench's screenshots. A value
//! that makes no sense is left out and said in the log; it never keeps the
//! TV's Home from starting.

use std::time::Duration;

use serde_json::{Map, Value};

use super::Platform;
use super::lifecycle::Method;

/// The longest `exitAfter`, in seconds: an hour, for a trial left running.
const MAX_EXIT_AFTER: f64 = 3600.0;
/// The longest window type name taken from the parameters.
const MAX_WINDOW_TYPE: usize = 48;
/// The most screenshots one launch asks for.
pub const MAX_SHOTS: usize = 8;

/// One launch argument, taken apart.
#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    /// The launch parameters: the whole argument under version 1, the
    /// envelope's `parameters` under version 2.
    pub params: Map<String, Value>,
    /// The registration SAM asked for in a version 2 envelope; `None` for
    /// version 1, whose registration is `registerNativeApp`.
    pub method: Option<Method>,
    /// SAM's `preload` (`"full"` or `"partial"`): the shell is to start
    /// hidden, until a relaunch shows it.
    pub preload: Option<String>,
    /// SAM's `reason` for a version 2 launch, for the log.
    pub reason: Option<String>,
}

impl Launch {
    /// Reads a launch argument that parsed as a JSON object.
    pub fn read(top: &Map<String, Value>) -> Self {
        let envelope = top.get("interfaceVersion").and_then(Value::as_u64) == Some(2)
            || (top.get("event").and_then(Value::as_str) == Some("launch")
                && top.get("parameters").is_some_and(Value::is_object));
        let preload = short_text(top.get("preload"));
        if !envelope {
            return Self {
                params: top.clone(),
                method: None,
                preload,
                reason: None,
            };
        }
        let params = match top.get("parameters") {
            Some(Value::Object(params)) => params.clone(),
            _ => Map::new(),
        };
        let method = top
            .get("interfaceMethod")
            .and_then(Value::as_str)
            .and_then(Method::from_name)
            .or(Some(Method::RegisterApp));
        Self {
            params,
            method,
            preload,
            reason: short_text(top.get("reason")),
        }
    }

    /// Whether SAM asked for a hidden start.
    pub fn preloaded(&self) -> bool {
        self.preload.as_deref().is_some_and(|p| !p.is_empty())
    }
}

/// A short, clean string from another program, or `None`.
fn short_text(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    let clean: String = text.chars().filter(|c| !c.is_control()).take(64).collect();
    Some(clean)
}

/// The platform layer's parameters, from the launch parameters.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Params {
    /// `"platform": "wayland"` or `"sdl"`.
    pub platform: Option<Platform>,
    /// `"gles": 3`: ask for an ES 3.0 context (ES 2 if refused).
    pub gles3: bool,
    /// `"probe": "gpu"`: measure the GPU plan's gate after start-up.
    pub probe: bool,
    /// `"window": "overlay"`: the overlay trial's `_WEBOS_WINDOW_TYPE`.
    pub window_type: Option<String>,
    /// `"exitAfter": 20`: leave after this long, a trial's safety net.
    pub exit_after: Option<Duration>,
    /// `"hidden": true`: start without showing the window, as a preload
    /// does, until a relaunch shows it.
    pub hidden: bool,
    /// `"shots": [6, 60]`: save the first settled frame at or after each of
    /// these times on the loop's clock (the bench's screenshots), sorted.
    pub shots: Vec<Duration>,
    /// `"saver": "hold"`: answer LG's screensaver requests `ack: false`
    /// for this launch (a bench run; LG's settings stay as they are).
    pub hold_saver: bool,
    /// What was said that makes no sense, one line each for the log.
    pub refused: Vec<String>,
}

impl Params {
    pub fn read(params: &Map<String, Value>) -> Self {
        let mut out = Self::default();
        if let Some(said) = params.get("platform") {
            match said.as_str().and_then(Platform::from_name) {
                Some(platform) => out.platform = Some(platform),
                None => out.refuse("platform", said, "sdl or wayland"),
            }
        }
        if let Some(said) = params.get("gles") {
            match said.as_u64().or_else(|| said.as_str()?.trim().parse().ok()) {
                Some(3) => out.gles3 = true,
                Some(2) => {}
                _ => out.refuse("gles", said, "2 or 3"),
            }
        }
        if let Some(said) = params.get("probe") {
            match said.as_str() {
                Some("gpu") => out.probe = true,
                _ => out.refuse("probe", said, "\"gpu\""),
            }
        }
        if let Some(said) = params.get("window") {
            match said.as_str().and_then(window_type) {
                Some(kind) => out.window_type = Some(kind),
                None => out.refuse("window", said, "a window type such as \"overlay\""),
            }
        }
        if let Some(said) = params.get("exitAfter") {
            match said
                .as_f64()
                .filter(|s| s.is_finite() && *s > 0.0 && *s <= MAX_EXIT_AFTER)
            {
                Some(secs) => out.exit_after = Some(Duration::from_secs_f64(secs)),
                None => out.refuse("exitAfter", said, "seconds, up to an hour"),
            }
        }
        if let Some(said) = params.get("hidden") {
            match said.as_bool() {
                Some(hidden) => out.hidden = hidden,
                None => out.refuse("hidden", said, "true or false"),
            }
        }
        if let Some(said) = params.get("saver") {
            match said.as_str() {
                Some("hold") => out.hold_saver = true,
                Some("allow") => {}
                _ => out.refuse("saver", said, "\"hold\" or \"allow\""),
            }
        }
        if let Some(said) = params.get("shots") {
            match read_shots(said) {
                Some(shots) => out.shots = shots,
                None => out.refuse(
                    "shots",
                    said,
                    "a list of 1 to 8 times in seconds, up to an hour",
                ),
            }
        }
        out
    }

    fn refuse(&mut self, key: &str, said: &Value, wanted: &str) {
        let mut shown = said.to_string();
        if shown.len() > 64 {
            shown = shown.chars().take(64).collect::<String>() + "…";
        }
        self.refused.push(format!(
            "launch parameter {key:?}: {shown} is not {wanted}; left out"
        ));
    }
}

/// `"shots"`' times, sorted, or `None` unless it is a list of 1 to
/// `MAX_SHOTS` numbers from 0 to an hour.
fn read_shots(said: &Value) -> Option<Vec<Duration>> {
    let list = said
        .as_array()
        .filter(|l| (1..=MAX_SHOTS).contains(&l.len()))?;
    let mut shots = list
        .iter()
        .map(|v| {
            v.as_f64()
                .filter(|s| s.is_finite() && (0.0..=MAX_EXIT_AFTER).contains(s))
                .map(Duration::from_secs_f64)
        })
        .collect::<Option<Vec<_>>>()?;
    shots.sort();
    Some(shots)
}

/// The compositor's window type for a short name (`overlay` gives
/// `_WEBOS_WINDOW_TYPE_OVERLAY`) or a full one. Letters and underscores
/// only, so nothing else reaches the compositor's property.
pub fn window_type(name: &str) -> Option<String> {
    const PREFIX: &str = "_WEBOS_WINDOW_TYPE_";
    let name = name.trim();
    let upper = name.to_ascii_uppercase();
    let full = if upper.starts_with(PREFIX) {
        upper
    } else {
        format!("{PREFIX}{upper}")
    };
    let rest = &full[PREFIX.len()..];
    let fine = !rest.is_empty()
        && full.len() <= MAX_WINDOW_TYPE
        && rest.chars().all(|c| c.is_ascii_uppercase() || c == '_');
    fine.then_some(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn version_1_is_the_parameters_themselves() {
        // The B9's argv for a Home press, 2026-10-31 (b9.md, "Home").
        let top = object(json!({
            "@system_native_app": true, "launchType": "homeKey", "nid": "x", "look": "plain"
        }));
        let launch = Launch::read(&top);
        assert_eq!(launch.method, None);
        assert_eq!(launch.params.get("look"), Some(&json!("plain")));
        assert!(!launch.preloaded());
    }

    #[test]
    fn version_2_is_an_envelope_around_them() {
        let top = object(json!({
            "event": "launch", "reason": "homeKey", "appId": "x",
            "interfaceVersion": 2, "interfaceMethod": "registerApp",
            "parameters": {"look": "plain", "platform": "wayland"},
            "preload": "full"
        }));
        let launch = Launch::read(&top);
        assert_eq!(launch.method, Some(Method::RegisterApp));
        assert_eq!(launch.params.get("look"), Some(&json!("plain")));
        assert_eq!(launch.preload.as_deref(), Some("full"));
        assert!(launch.preloaded());
        assert_eq!(launch.reason.as_deref(), Some("homeKey"));
        // An envelope with no method named still means version 2.
        let bare = object(json!({"interfaceVersion": 2, "parameters": {}}));
        assert_eq!(Launch::read(&bare).method, Some(Method::RegisterApp));
        // An empty preload is no preload.
        let empty = object(json!({"interfaceVersion": 2, "preload": ""}));
        assert!(!Launch::read(&empty).preloaded());
    }

    #[test]
    fn the_platform_layers_parameters() {
        let params = Params::read(&object(json!({
            "platform": "wayland", "gles": 3, "probe": "gpu", "window": "overlay",
            "exitAfter": 20, "hidden": true, "shots": [60, 6.5], "saver": "hold"
        })));
        assert_eq!(
            params,
            Params {
                platform: Some(Platform::Wayland),
                gles3: true,
                probe: true,
                window_type: Some("_WEBOS_WINDOW_TYPE_OVERLAY".to_owned()),
                exit_after: Some(Duration::from_secs(20)),
                hidden: true,
                shots: vec![Duration::from_millis(6500), Duration::from_secs(60)],
                hold_saver: true,
                refused: Vec::new(),
            }
        );
        assert_eq!(Params::read(&Map::new()), Params::default());
        assert!(Params::read(&object(json!({"gles": "3"}))).gles3);
        assert!(!Params::read(&object(json!({"gles": 2}))).gles3);
    }

    #[test]
    fn nonsense_is_left_out_and_said() {
        let params = Params::read(&object(json!({
            "platform": "x11", "gles": 4, "probe": true, "window": "over lay;rm",
            "exitAfter": -1, "hidden": "yes", "shots": [1, -2], "saver": true
        })));
        assert_eq!(params.platform, None);
        assert!(!params.gles3 && !params.probe && !params.hidden);
        assert_eq!((params.window_type, params.exit_after), (None, None));
        assert!(params.shots.is_empty());
        assert!(!params.hold_saver);
        assert_eq!(params.refused.len(), 8, "{:?}", params.refused);
        for bad in [
            json!([]),
            json!(3),
            json!([1, 2, 3, 4, 5, 6, 7, 8, 9]),
            json!([4000]),
        ] {
            let p = Params::read(&object(json!({ "shots": bad })));
            assert!(p.shots.is_empty() && p.refused.len() == 1, "{bad}");
        }
        assert!(params.refused[0].starts_with("launch parameter \"platform\": \"x11\""));
    }

    #[test]
    fn window_types_are_letters_and_underscores_only() {
        assert_eq!(
            window_type("popup").as_deref(),
            Some("_WEBOS_WINDOW_TYPE_POPUP")
        );
        assert_eq!(
            window_type("_WEBOS_WINDOW_TYPE_SYSTEM_UI").as_deref(),
            Some("_WEBOS_WINDOW_TYPE_SYSTEM_UI")
        );
        for bad in [
            "",
            "_WEBOS_WINDOW_TYPE_",
            "over lay",
            "a\"b",
            "x1",
            &"A".repeat(40),
        ] {
            assert_eq!(window_type(bad), None, "{bad:?}");
        }
    }
}
