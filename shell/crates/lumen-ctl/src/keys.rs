//! The route for `key` (remote.md, risk 1), picked by `--keys`:
//!
//! * `none`: no route; `key` is left out of `hello` and refused.
//! * `keymanager`: LG's `createKeyEvent`, with a payload built from the
//!   template the PM writes into `/var/lib/lumen/remote/keys.json` once
//!   `lumen-ctl probe-keys` has shown what the method wants.
//! * `uinput` (not built): a virtual remote through `/dev/uinput`, the other
//!   route R0 found. It would be one more `KeyRoute`.
//!
//! `keys.json`:
//!
//! ```json
//! {
//!   "service": "keymanager",
//!   "press": [{"keyCode": "$code", "type": "down"}, {"keyCode": "$code", "type": "up"}],
//!   "codes": {"up": 38, "down": 40, "ok": 13}
//! }
//! ```
//!
//! `service` names one of the two fixed methods (`keymanager` or
//! `legacyinput`), never a URI. One press sends each `press` payload in turn,
//! with every string that is exactly `$code` replaced by the key's code. A
//! key with no code is refused as unavailable.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::bus::{self, Bus};
use crate::lg::{self, Call};
use crate::privacy;
use crate::protocol::{ErrorCode, KEY_NAMES, Refusal};

pub const KEYS_FILE: &str = "/var/lib/lumen/remote/keys.json";
const MAX_FILE: u64 = 16 << 10;
const MAX_PRESS: usize = 4;
const MAX_DEPTH: usize = 4;
const PLACEHOLDER: &str = "$code";

pub trait KeyRoute: Send + Sync {
    /// `--keys`' word for it, for `hello` and the log.
    fn name(&self) -> &'static str;
    /// Whether `key` is in `hello`'s verbs.
    fn any(&self) -> bool;
    fn press(&self, bus: &dyn Bus, key: &'static str) -> Result<(), Refusal>;
}

pub struct NoKeys;

impl KeyRoute for NoKeys {
    fn name(&self) -> &'static str {
        "none"
    }

    fn any(&self) -> bool {
        false
    }

    fn press(&self, _: &dyn Bus, _: &'static str) -> Result<(), Refusal> {
        Err(Refusal::new(
            ErrorCode::Unavailable,
            "this TV has no route for keys yet",
        ))
    }
}

/// `keys.json`, checked.
#[derive(Debug, PartialEq)]
pub struct Template {
    uri: &'static str,
    press: Vec<Value>,
    codes: BTreeMap<&'static str, Value>,
}

fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn fill(v: &Value, code: &Value) -> Value {
    match v {
        Value::String(s) if s == PLACEHOLDER => code.clone(),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, code)).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, x)| (k.clone(), fill(x, code))).collect())
        }
        other => other.clone(),
    }
}

impl Template {
    pub fn parse(text: &[u8]) -> Result<Self, String> {
        let v: Value =
            serde_json::from_slice(text).map_err(|e| format!("keys.json is not JSON: {e}"))?;
        let Value::Object(top) = v else {
            return Err("keys.json is not an object".to_owned());
        };
        if let Some(k) = top
            .keys()
            .find(|k| !["service", "press", "codes"].contains(&k.as_str()))
        {
            return Err(format!("keys.json: unexpected field {k:?}"));
        }
        let uri = match top.get("service").and_then(Value::as_str) {
            Some("keymanager") => lg::KEYMANAGER_KEY,
            Some("legacyinput") => lg::LEGACYINPUT_KEY,
            _ => return Err("keys.json: service must be keymanager or legacyinput".to_owned()),
        };
        let press = top
            .get("press")
            .and_then(Value::as_array)
            .filter(|p| (1..=MAX_PRESS).contains(&p.len()))
            .ok_or_else(|| {
                format!("keys.json: press must be a list of 1 to {MAX_PRESS} objects")
            })?;
        if press.iter().any(|p| !p.is_object() || depth(p) > MAX_DEPTH) {
            return Err(format!(
                "keys.json: each press is an object at most {MAX_DEPTH} deep"
            ));
        }
        let codes_in = top
            .get("codes")
            .and_then(Value::as_object)
            .ok_or_else(|| "keys.json: codes must be an object".to_owned())?;
        let mut codes = BTreeMap::new();
        for (name, code) in codes_in {
            let Some(&key) = KEY_NAMES.iter().find(|k| **k == name.as_str()) else {
                return Err(format!("keys.json: {name:?} is not a key name"));
            };
            let plain = match code {
                Value::Number(n) => n.as_u64().is_some_and(|n| n <= 0xFFFF),
                Value::String(s) => {
                    !s.is_empty()
                        && s.len() <= 32
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                }
                _ => false,
            };
            if !plain {
                return Err(format!(
                    "keys.json: the code for {name} must be a number up to 65535 or a short name"
                ));
            }
            codes.insert(key, code.clone());
        }
        if codes.is_empty() {
            return Err("keys.json: no codes".to_owned());
        }
        Ok(Self {
            uri,
            press: press.clone(),
            codes,
        })
    }

    /// `path`, if it is a regular file owned by `owner` (root on the TV).
    pub fn load(path: &Path, owner: Option<u32>) -> Result<Self, String> {
        let text = privacy::read_owned(path, owner, MAX_FILE)
            .ok_or_else(|| format!("no usable {}", path.display()))?;
        Self::parse(&text)
    }

    /// The calls one press of `key` makes, or `None` when it has no code.
    pub fn calls(&self, key: &str) -> Option<Vec<Call>> {
        let code = self.codes.get(key)?;
        Some(
            self.press
                .iter()
                .map(|p| Call {
                    uri: self.uri,
                    payload: fill(p, code),
                })
                .collect(),
        )
    }
}

impl KeyRoute for Template {
    fn name(&self) -> &'static str {
        "keymanager"
    }

    fn any(&self) -> bool {
        !self.codes.is_empty()
    }

    fn press(&self, bus: &dyn Bus, key: &'static str) -> Result<(), Refusal> {
        let calls = self.calls(key).ok_or_else(|| {
            Refusal::new(
                ErrorCode::Unavailable,
                format!("keys.json has no code for {key}"),
            )
        })?;
        for c in &calls {
            bus::checked(bus, c).map_err(|e| Refusal::new(ErrorCode::Failed, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const GOOD: &str = r#"{
        "service": "keymanager",
        "press": [{"keyCode": "$code", "type": "down", "extra": ["$code", 1]}, {"keyCode": "$code", "type": "up"}],
        "codes": {"up": 38, "ok": "ENTER"}
    }"#;

    #[test]
    fn a_template_fills_in_each_keys_code() {
        let t = Template::parse(GOOD.as_bytes()).unwrap();
        assert!(t.any());
        let calls = t.calls("up").unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|c| c.uri == lg::KEYMANAGER_KEY));
        assert_eq!(
            calls[0].payload,
            json!({ "keyCode": 38, "type": "down", "extra": [38, 1] })
        );
        assert_eq!(calls[1].payload, json!({ "keyCode": 38, "type": "up" }));
        assert_eq!(
            t.calls("ok").unwrap()[1].payload,
            json!({ "keyCode": "ENTER", "type": "up" })
        );
        assert_eq!(t.calls("menu"), None);
        let legacy = GOOD.replace("keymanager", "legacyinput");
        assert_eq!(
            Template::parse(legacy.as_bytes())
                .unwrap()
                .calls("up")
                .unwrap()[0]
                .uri,
            lg::LEGACYINPUT_KEY
        );
    }

    #[test]
    fn a_template_cannot_name_another_method_or_key() {
        for bad in [
            r#"{"service":"luna://com.webos.service.tvpower/power/powerOff","press":[{}],"codes":{"up":1}}"#,
            r#"{"service":"keymanager","press":[],"codes":{"up":1}}"#,
            r#"{"service":"keymanager","press":[{},{},{},{},{}],"codes":{"up":1}}"#,
            r#"{"service":"keymanager","press":["x"],"codes":{"up":1}}"#,
            r#"{"service":"keymanager","press":[{"a":{"b":{"c":{"d":[1]}}}}],"codes":{"up":1}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{"power":116}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{"up":70000}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{"up":"a b"}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{"up":[1]}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{}}"#,
            r#"{"service":"keymanager","press":[{}],"codes":{"up":1},"uri":"x"}"#,
            "[1]",
            "nope",
        ] {
            assert!(Template::parse(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn no_route_refuses_as_unavailable() {
        assert!(!NoKeys.any());
        let bus = crate::fake::FakeBus::new(false);
        assert_eq!(
            NoKeys.press(&bus, "ok").unwrap_err().code,
            ErrorCode::Unavailable
        );
        let t = Template::parse(GOOD.as_bytes()).unwrap();
        assert_eq!(
            t.press(&bus, "menu").unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert_eq!(t.press(&bus, "up"), Ok(()));
        assert_eq!(bus.keys_pressed(), vec![json!(38), json!(38)]);
    }
}
