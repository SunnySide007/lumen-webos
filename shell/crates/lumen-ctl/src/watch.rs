//! LG's state that lumen-ctl pushes to its client: the power state, the
//! volume, the app in front, the inputs and the settings. Each comes from one
//! or two luna methods, through a subscription where the method honours one
//! and a poll where it doesn't; either way every message lands in `States`,
//! which keeps the last value and says whether it changed, so a client sees
//! each change once, whoever made it.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::lg::{self, Call, Input, LG_SETTINGS};
use crate::protocol::{self, SettingValue};

/// How often a polled state is read: the four state methods every 2 s, the
/// settings, which change only from LG's menus, every 10 s.
pub const POLL_STATE: Duration = Duration::from_secs(2);
pub const POLL_SETTINGS: Duration = Duration::from_secs(10);

/// One luna method that feeds a watched state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Power,
    Volume,
    Foreground,
    Inputs,
    Picture,
    Sound,
}

impl Source {
    pub const ALL: [Self; 6] = [
        Self::Power,
        Self::Volume,
        Self::Foreground,
        Self::Inputs,
        Self::Picture,
        Self::Sound,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The state's name, as its event and `hello`'s `polling` list say it.
    pub fn state(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Volume => "volume",
            Self::Foreground => "foreground",
            Self::Inputs => "inputs",
            Self::Picture | Self::Sound => "settings",
        }
    }

    /// The method and payload that read it, holding a subscription open
    /// when `subscribe`.
    pub fn call(self, subscribe: bool) -> Call {
        let plain = |uri| {
            let payload = if subscribe {
                json!({ "subscribe": true })
            } else {
                json!({})
            };
            Call { uri, payload }
        };
        match self {
            Self::Power => plain(lg::GET_POWER_STATE),
            Self::Volume => plain(lg::GET_VOLUME),
            Self::Foreground => plain(lg::GET_FOREGROUND_APP),
            Self::Inputs => plain(lg::GET_ALL_INPUT_STATUS),
            Self::Picture => lg::read_settings("picture", subscribe),
            Self::Sound => lg::read_settings("sound", subscribe),
        }
    }

    pub fn poll_every(self) -> Duration {
        match self {
            Self::Picture | Self::Sound => POLL_SETTINGS,
            _ => POLL_STATE,
        }
    }

    fn category(self) -> &'static str {
        match self {
            Self::Sound => "sound",
            _ => "picture",
        }
    }
}

/// Whether a subscription's first reply says the method will answer again.
/// Only an explicit `"subscribed": true` counts: a method that ignores
/// `subscribe` answers once, and is polled.
pub fn subscribed(first: &Value) -> bool {
    lg::check_message(first).is_ok()
        && first.get("subscribed").and_then(Value::as_bool) == Some(true)
}

/// The last value of every watched state, `None` until first read.
#[derive(Debug, Default)]
pub struct States {
    power: Option<Value>,
    level: Option<u8>,
    muted: Option<bool>,
    foreground: Option<Option<String>>,
    inputs: Option<Vec<Input>>,
    /// The settings in Lumen's words, and LG's word for any the table has no
    /// name for, keyed by Lumen's setting.
    values: BTreeMap<&'static str, SettingValue>,
    lg: BTreeMap<&'static str, String>,
    settings_read: bool,
}

fn settings_data(values: &BTreeMap<&str, SettingValue>, lg: &BTreeMap<&str, String>) -> Value {
    let values: Map<String, Value> = values
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.to_json()))
        .collect();
    let mut data = json!({ "values": values });
    if !lg.is_empty() {
        data["lg"] = json!(lg);
    }
    data
}

impl States {
    /// A message from `src`; the event line for what changed, if anything
    /// did. A message LG marks as failed, or one without the fields, changes
    /// nothing.
    pub fn update(&mut self, src: Source, msg: &Value) -> Option<String> {
        if lg::check_message(msg).is_err() {
            return None;
        }
        match src {
            Source::Power => {
                let data = lg::power(msg)?;
                (self.power.as_ref() != Some(&data)).then(|| {
                    self.power = Some(data.clone());
                    protocol::event("power", data)
                })
            }
            Source::Volume => {
                let (level, muted) = lg::volume(msg);
                if level.is_none() && muted.is_none() {
                    return None;
                }
                let new = (level.or(self.level), muted.or(self.muted));
                if new == (self.level, self.muted) {
                    return None;
                }
                (self.level, self.muted) = new;
                self.volume_event()
            }
            Source::Foreground => {
                let id = lg::foreground(msg)?;
                (self.foreground.as_ref() != Some(&id)).then(|| {
                    self.foreground = Some(id.clone());
                    protocol::event("foreground", json!({ "id": id }))
                })
            }
            Source::Inputs => {
                let inputs = lg::inputs(msg)?;
                (self.inputs.as_ref() != Some(&inputs)).then(|| {
                    let event = inputs_event(&inputs);
                    self.inputs = Some(inputs);
                    event
                })
            }
            Source::Picture | Source::Sound => self.settings(src.category(), msg),
        }
    }

    fn volume_event(&self) -> Option<String> {
        let level = self.level?;
        Some(protocol::event(
            "volume",
            json!({ "level": level, "muted": self.muted.unwrap_or(false) }),
        ))
    }

    fn settings(&mut self, category: &str, msg: &Value) -> Option<String> {
        let settings = msg.get("settings")?.as_object()?;
        self.settings_read = true;
        let (mut values, mut unmapped) = (BTreeMap::new(), BTreeMap::new());
        for s in LG_SETTINGS.iter().filter(|s| s.category == category) {
            let Some(raw) = settings.get(s.key) else {
                continue;
            };
            let key = s.setting.key;
            match lg::to_lumen(s, raw) {
                Some(v) => {
                    self.lg.remove(key);
                    if self.values.insert(key, v) != Some(v) {
                        values.insert(key, v);
                    }
                }
                None => {
                    let word = lg::clean(raw.as_str().unwrap_or(&raw.to_string()), lg::MAX_TITLE);
                    self.values.remove(key);
                    if self.lg.insert(key, word.clone()).as_ref() != Some(&word) {
                        unmapped.insert(key, word);
                    }
                }
            }
        }
        if values.is_empty() && unmapped.is_empty() {
            return None;
        }
        Some(protocol::event(
            "settings",
            settings_data(&values, &unmapped),
        ))
    }

    /// The current value of every state read so far, in the mock's order, as
    /// a new client gets it after `hello`.
    pub fn snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(power) = &self.power {
            out.push(protocol::event("power", power.clone()));
        }
        out.extend(self.volume_event());
        if let Some(id) = &self.foreground {
            out.push(protocol::event("foreground", json!({ "id": id })));
        }
        if let Some(inputs) = &self.inputs {
            out.push(inputs_event(inputs));
        }
        if self.settings_read {
            out.push(protocol::event(
                "settings",
                settings_data(&self.values, &self.lg),
            ));
        }
        out
    }

    /// The settings as `settings.get` answers: the values asked for, and
    /// LG's word for those without a name.
    pub fn read(&self, keys: &[&'static str]) -> Value {
        let values: BTreeMap<&str, SettingValue> = self
            .values
            .iter()
            .filter(|(k, _)| keys.contains(k))
            .map(|(k, v)| (*k, *v))
            .collect();
        let lg: BTreeMap<&str, String> = self
            .lg
            .iter()
            .filter(|(k, _)| keys.contains(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        settings_data(&values, &lg)
    }
}

fn inputs_event(inputs: &[Input]) -> String {
    let list: Vec<Value> = inputs.iter().map(Input::to_json).collect();
    protocol::event("inputs", json!({ "inputs": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    fn event(line: &str) -> Value {
        parse(line)
    }

    #[test]
    fn subscriptions_count_only_when_lg_says_so() {
        assert!(subscribed(&parse(
            r#"{"returnValue":true,"subscribed":true,"volume":3}"#
        )));
        assert!(!subscribed(&parse(r#"{"returnValue":true,"volume":3}"#)));
        assert!(!subscribed(&parse(
            r#"{"returnValue":true,"subscribed":false}"#
        )));
        assert!(!subscribed(&parse(
            r#"{"returnValue":false,"subscribed":true}"#
        )));
    }

    #[test]
    fn the_sources_read_what_the_verbs_watch() {
        assert_eq!(Source::Volume.call(true).uri, lg::GET_VOLUME);
        assert_eq!(
            Source::Volume.call(true).payload,
            json!({ "subscribe": true })
        );
        assert_eq!(Source::Power.call(false).payload, json!({}));
        assert_eq!(Source::Sound.call(true).payload["category"], json!("sound"));
        assert_eq!(Source::Picture.state(), "settings");
        assert_eq!(Source::Inputs.poll_every(), POLL_STATE);
        for (i, s) in Source::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn pushes_each_change_once() {
        let mut s = States::default();
        let power = parse(r#"{"state":"Active","returnValue":true}"#);
        assert_eq!(
            event(&s.update(Source::Power, &power).unwrap()),
            json!({ "event": "power", "data": { "state": "active", "lg": "Active" } })
        );
        assert_eq!(s.update(Source::Power, &power), None);
        let off = parse(r#"{"state":"Screen Off"}"#);
        assert!(s.update(Source::Power, &off).is_some());

        let v = parse(r#"{"volume":6,"muted":false,"returnValue":true}"#);
        assert_eq!(
            event(&s.update(Source::Volume, &v).unwrap()),
            json!({ "event": "volume", "data": { "level": 6, "muted": false } })
        );
        assert_eq!(s.update(Source::Volume, &v), None);
        // A message with mute alone keeps the level.
        assert_eq!(
            event(
                &s.update(Source::Volume, &parse(r#"{"muted":true}"#))
                    .unwrap()
            ),
            json!({ "event": "volume", "data": { "level": 6, "muted": true } })
        );
        // A failed message changes nothing.
        assert_eq!(
            s.update(
                Source::Volume,
                &parse(r#"{"returnValue":false,"volume":9}"#)
            ),
            None
        );

        let front = parse(r#"{"appId":"netflix","returnValue":true}"#);
        assert_eq!(
            event(&s.update(Source::Foreground, &front).unwrap()),
            json!({ "event": "foreground", "data": { "id": "netflix" } })
        );
        assert_eq!(s.update(Source::Foreground, &front), None);
        assert_eq!(
            event(
                &s.update(Source::Foreground, &parse(r#"{"appId":""}"#))
                    .unwrap()
            ),
            json!({ "event": "foreground", "data": { "id": null } })
        );
    }

    #[test]
    fn pushes_the_settings_that_changed() {
        let mut s = States::default();
        let picture = parse(
            r#"{"returnValue":true,"category":"picture","dimension":{"dynamicRange":"sdr"},
                "settings":{"pictureMode":"hdrEffect","backlight":"100","energySaving":"off"}}"#,
        );
        // A first read is news too: before `hello` the server drops it, and
        // after, a state that could not be read at first arrives this way.
        assert_eq!(
            event(&s.update(Source::Picture, &picture).unwrap())["data"],
            json!({
                "values": { "picture.oledLight": 100, "picture.energySaving": "off" },
                "lg": { "picture.mode": "hdrEffect" },
            })
        );
        assert_eq!(
            event(
                &s.update(
                    Source::Sound,
                    &parse(r#"{"settings":{"soundMode":"standard"}}"#)
                )
                .unwrap()
            )["data"],
            json!({ "values": { "sound.mode": "standard" } })
        );
        assert_eq!(
            event(&s.snapshot().pop().unwrap()),
            json!({ "event": "settings", "data": {
                "values": { "picture.oledLight": 100, "picture.energySaving": "off", "sound.mode": "standard" },
                "lg": { "picture.mode": "hdrEffect" },
            } })
        );
        assert_eq!(s.update(Source::Picture, &picture), None);
        // A subscription's message with one key: only that one goes out.
        assert_eq!(
            event(
                &s.update(
                    Source::Picture,
                    &parse(r#"{"settings":{"pictureMode":"cinema"}}"#)
                )
                .unwrap()
            ),
            json!({ "event": "settings", "data": { "values": { "picture.mode": "cinema" } } })
        );
        assert_eq!(
            event(
                &s.update(
                    Source::Picture,
                    &parse(r#"{"settings":{"backlight":"80"}}"#)
                )
                .unwrap()
            ),
            json!({ "event": "settings", "data": { "values": { "picture.oledLight": 80 } } })
        );
        assert_eq!(
            s.read(&["picture.mode", "sound.mode"]),
            json!({ "values": { "picture.mode": "cinema", "sound.mode": "standard" } })
        );
        // Back to a mode Lumen has no name for: out of the values, into lg.
        let back = s
            .update(
                Source::Picture,
                &parse(r#"{"settings":{"pictureMode":"hdrEffect"}}"#),
            )
            .unwrap();
        assert_eq!(
            event(&back),
            json!({ "event": "settings", "data": { "values": {}, "lg": { "picture.mode": "hdrEffect" } } })
        );
        assert_eq!(
            s.read(&["picture.mode"]),
            json!({ "values": {}, "lg": { "picture.mode": "hdrEffect" } })
        );
    }

    #[test]
    fn the_snapshot_has_what_was_read_in_the_mocks_order() {
        let mut s = States::default();
        assert!(s.snapshot().is_empty());
        s.update(Source::Volume, &parse(r#"{"volume":6,"muted":false}"#));
        s.update(Source::Power, &parse(r#"{"state":"Active"}"#));
        s.update(
            Source::Inputs,
            &parse(
                r#"{"devices":[{"appId":"com.webos.app.hdmi1","label":"PC","hdmiPlugIn":true}]}"#,
            ),
        );
        s.update(Source::Foreground, &parse(r#"{"appId":"netflix"}"#));
        s.update(
            Source::Sound,
            &parse(r#"{"settings":{"soundMode":"music"}}"#),
        );
        let names: Vec<String> = s
            .snapshot()
            .iter()
            .map(|l| event(l)["event"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            names,
            ["power", "volume", "foreground", "inputs", "settings"]
        );
        assert_eq!(
            event(&s.snapshot()[3])["data"],
            json!({ "inputs": [{ "id": "com.webos.app.hdmi1", "label": "PC", "device": null, "connected": true }] })
        );
    }

    #[test]
    fn a_mute_without_a_level_waits_for_the_level() {
        let mut s = States::default();
        assert_eq!(s.update(Source::Volume, &parse(r#"{"muted":true}"#)), None);
        assert!(s.snapshot().is_empty());
        assert_eq!(
            event(&s.update(Source::Volume, &parse(r#"{"volume":3}"#)).unwrap())["data"],
            json!({ "level": 3, "muted": true })
        );
    }
}
