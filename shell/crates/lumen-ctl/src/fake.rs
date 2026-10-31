//! `--bus fake`: an in-process TV that answers the luna methods lumen-ctl
//! calls the way the B9's replies are shaped (docs/device/b9.md, "Remote"),
//! with made-up apps, labels and devices. It keeps its own state, so a
//! `volume.set` changes what `getVolume` says and pushes it to the volume
//! subscription, and it has one script: 300 ms after the volume subscription
//! opens, the volume goes up by one, as if someone pressed the Magic
//! Remote's volume key. `getPowerState` ignores `subscribe`, as some of LG's
//! methods may, so the power state is polled.
//!
//! It reaches no TV, no luna and no network, and reads no file: it is for the
//! tests and for running the protocol on a PC.

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::bus::{Bus, Sink};
use crate::lg;

/// When the scripted volume press comes, after the volume subscription opens.
pub const SCRIPT_DELAY: Duration = Duration::from_millis(300);

struct Tv {
    power: &'static str,
    volume: u64,
    muted: bool,
    foreground: String,
    picture: Map<String, Value>,
    sound: Map<String, Value>,
    typed: String,
    keys: Vec<Value>,
    toasts: usize,
}

struct Subscription {
    uri: &'static str,
    payload: Value,
    index: usize,
    sink: Sink,
}

pub struct FakeBus {
    tv: Mutex<Tv>,
    subs: Mutex<Vec<Subscription>>,
    script: bool,
}

fn ok(mut reply: Value) -> Value {
    reply["returnValue"] = json!(true);
    reply
}

fn refused(code: i64, text: &str) -> Value {
    json!({ "returnValue": false, "errorCode": code, "errorText": text })
}

fn launch_points() -> Value {
    let point = |id: &str, title: &str, color: &str| json!({ "id": id, "title": title, "iconColor": color, "lptype": "default" });
    ok(json!({ "launchPoints": [
        point("netflix", "Netflix", "#e50914"),
        point("youtube.leanback.v4", "YouTube", "#ff0033"),
        point("amazon", "Prime Video", "#1a98ff"),
        point("cdp-30", "Plex", "#e5a00d"),
        point(lg::LUMEN_ID, "Lumen", "#7c8cff"),
        point("org.webosbrew.hbchannel", "Homebrew Channel", "#000000"),
        point("com.palm.app.settings", "Settings", "#555555"),
        { "id": "bookmark.1", "title": "A web page", "lptype": "bookmark" },
    ] }))
}

fn input_status() -> Value {
    let hdmi = |n: u8, label: &str, plugged: bool, sub: Value| {
        json!({
            "id": format!("HDMI_{n}"), "port": n, "appId": format!("com.webos.app.hdmi{n}"),
            "label": label, "connected": true, "hdmiPlugIn": plugged, "subList": sub,
            "iconPrefix": "/usr/palm/plugins/inputapps/assets/small/", "icon": format!("HDMI_{n}.png"),
        })
    };
    ok(json!({ "devices": [
        { "id": "AV_1", "port": 1, "appId": "com.webos.app.externalinput.av1", "label": "AV",
          "connected": false, "subList": [] },
        hdmi(1, "HDMI 1", false, json!([])),
        hdmi(2, "Sound bar", true, json!([{ "id": "URCU", "labelName": "Acme", "brandName": "Acme" }])),
        hdmi(3, "Streamer", true, json!([{ "id": "SIMPLINK", "osdName": "Streaming box" }])),
        hdmi(4, "HDMI 4", false, json!([])),
    ] }))
}

/// What LG's description of each setting lists, shaped as webOS's
/// settingsservice gives it: the picture modes have a list (Filmmaker among
/// them), the sound modes a plain one, energy saving none.
fn setting_desc(key: &str) -> Value {
    let ext = |values: &[&str]| {
        let list: Vec<Value> = values
            .iter()
            .map(|v| json!({ "value": v, "visible": true, "active": true }))
            .collect();
        json!({ "arrayExt": list })
    };
    let values = match key {
        "pictureMode" => ext(&[
            "vivid",
            "normal",
            "eco",
            "cinema",
            "sports",
            "game",
            "filmMaker",
            "expert1",
            "expert2",
            "hdrEffect",
        ]),
        "soundMode" => json!({ "array": ["standard", "movie", "news", "sports", "music", "game"] }),
        _ => return refused(-1, "no description"),
    };
    ok(json!({ "results": [{ "key": key, "values": values }] }))
}

impl FakeBus {
    /// `script`: whether the scripted volume press runs (not in unit tests).
    pub fn new(script: bool) -> Self {
        let mut picture = Map::new();
        picture.insert("pictureMode".into(), json!("cinema"));
        picture.insert("backlight".into(), json!("80"));
        picture.insert("energySaving".into(), json!("off"));
        let mut sound = Map::new();
        sound.insert("soundMode".into(), json!("standard"));
        Self {
            tv: Mutex::new(Tv {
                power: "Active",
                volume: 14,
                muted: false,
                foreground: "netflix".into(),
                picture,
                sound,
                typed: String::new(),
                keys: Vec::new(),
                toasts: 0,
            }),
            subs: Mutex::new(Vec::new()),
            script,
        }
    }

    fn tv(&self) -> MutexGuard<'_, Tv> {
        self.tv.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The codes `createKeyEvent` was given, for the tests.
    #[cfg(test)]
    pub fn keys_pressed(&self) -> Vec<Value> {
        self.tv().keys.clone()
    }

    #[cfg(test)]
    pub fn typed(&self) -> String {
        self.tv().typed.clone()
    }

    #[cfg(test)]
    pub fn toasts(&self) -> usize {
        self.tv().toasts
    }

    /// Someone at the TV changes the volume.
    pub fn press_volume_up(&self) {
        {
            let mut tv = self.tv();
            tv.volume = (tv.volume + 1).min(100);
        }
        self.notify(lg::GET_VOLUME);
    }

    /// Sends each subscription on `uri` what the method says now.
    fn notify(&self, uri: &str) {
        let subs: Vec<(usize, Value, Sink)> = self
            .subs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|s| s.uri == uri)
            .map(|s| (s.index, s.payload.clone(), Arc::clone(&s.sink)))
            .collect();
        for (index, payload, sink) in subs {
            let mut reply = self.answer(uri, &payload);
            reply["subscribed"] = json!(true);
            sink(index, Ok(reply));
        }
    }

    fn settings_of(tv: &Tv, payload: &Value) -> Option<Value> {
        let category = payload.get("category")?.as_str()?;
        let all = match category {
            "picture" => &tv.picture,
            "sound" => &tv.sound,
            _ => return None,
        };
        let keys: Vec<&str> = payload
            .get("keys")
            .and_then(Value::as_array)
            .map(|k| k.iter().filter_map(Value::as_str).collect())
            .unwrap_or_else(|| all.keys().map(String::as_str).collect());
        let settings: Map<String, Value> = keys
            .iter()
            .filter_map(|k| Some(((*k).to_owned(), all.get(*k)?.clone())))
            .collect();
        let mut reply = json!({ "category": category, "settings": settings, "subscribed": false });
        if category == "picture" {
            reply["dimension"] = json!({ "pictureMode": tv.picture["pictureMode"], "input": "default", "dynamicRange": "sdr" });
        }
        Some(ok(reply))
    }

    /// The method's reply, and what it changed for whom.
    fn answer(&self, uri: &str, payload: &Value) -> Value {
        let mut tv = self.tv();
        match uri {
            lg::GET_POWER_STATE => ok(json!({ "state": tv.power })),
            lg::GET_VOLUME => ok(json!({
                "scenario": "mastervolume_tv_speaker", "volume": tv.volume, "muted": tv.muted,
            })),
            lg::GET_FOREGROUND_APP => {
                ok(json!({ "appId": tv.foreground, "windowId": "", "processId": "" }))
            }
            lg::GET_ALL_INPUT_STATUS => input_status(),
            lg::LIST_LAUNCH_POINTS => launch_points(),
            lg::GET_SYSTEM_SETTINGS => {
                Self::settings_of(&tv, payload).unwrap_or_else(|| refused(-1, "unknown category"))
            }
            lg::GET_SYSTEM_SETTING_DESC => {
                let key = payload["keys"][0].as_str().unwrap_or("");
                setting_desc(key)
            }
            lg::SET_VOLUME => match payload.get("volume").and_then(Value::as_u64) {
                Some(v) if v <= 100 => {
                    tv.volume = v;
                    ok(json!({}))
                }
                _ => refused(-1, "volume is out of range"),
            },
            lg::VOLUME_UP => {
                tv.volume = (tv.volume + 1).min(100);
                ok(json!({}))
            }
            lg::VOLUME_DOWN => {
                tv.volume = tv.volume.saturating_sub(1);
                ok(json!({}))
            }
            lg::SET_MUTED => match payload.get("muted").and_then(Value::as_bool) {
                Some(m) => {
                    tv.muted = m;
                    ok(json!({}))
                }
                None => refused(-1, "muted is required"),
            },
            lg::TURN_OFF_SCREEN => {
                tv.power = "Screen Off";
                ok(json!({}))
            }
            lg::TURN_ON_SCREEN => {
                tv.power = "Active";
                ok(json!({}))
            }
            lg::POWER_OFF => {
                tv.power = "Suspend";
                ok(json!({}))
            }
            lg::LAUNCH => {
                let id = payload.get("id").and_then(Value::as_str).unwrap_or("");
                tv.foreground = id.to_owned();
                ok(json!({ "id": id }))
            }
            lg::INSERT_TEXT => {
                let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
                tv.typed.push_str(text);
                ok(json!({}))
            }
            lg::DELETE_CHARACTERS => {
                let n = payload.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
                let keep = tv.typed.chars().count().saturating_sub(n);
                tv.typed = tv.typed.chars().take(keep).collect();
                ok(json!({}))
            }
            lg::SEND_ENTER_KEY => {
                tv.typed.clear();
                ok(json!({}))
            }
            lg::CREATE_TOAST => {
                tv.toasts += 1;
                ok(json!({ "toastId": format!("lumen-{}", tv.toasts) }))
            }
            lg::SET_SYSTEM_SETTINGS => {
                let category = payload.get("category").and_then(Value::as_str);
                let changes = payload.get("settings").and_then(Value::as_object);
                let (Some(category), Some(changes)) = (category, changes) else {
                    return refused(-1, "category and settings are required");
                };
                let target = match category {
                    "picture" => &mut tv.picture,
                    "sound" => &mut tv.sound,
                    _ => return refused(-1, "unknown category"),
                };
                for (k, v) in changes {
                    target.insert(k.clone(), v.clone());
                }
                ok(json!({}))
            }
            lg::KEYMANAGER_KEY | lg::LEGACYINPUT_KEY => match payload.get("keyCode") {
                Some(code) => {
                    tv.keys.push(code.clone());
                    ok(json!({}))
                }
                None => refused(-1000, "keyCode is a required parameter"),
            },
            _ => refused(-1, "Unknown method"),
        }
    }

    /// The state each write changes, to push to its subscribers.
    fn changed(uri: &str) -> Option<&'static str> {
        match uri {
            lg::SET_VOLUME | lg::VOLUME_UP | lg::VOLUME_DOWN | lg::SET_MUTED => {
                Some(lg::GET_VOLUME)
            }
            lg::LAUNCH => Some(lg::GET_FOREGROUND_APP),
            lg::SET_SYSTEM_SETTINGS => Some(lg::GET_SYSTEM_SETTINGS),
            lg::TURN_OFF_SCREEN | lg::TURN_ON_SCREEN | lg::POWER_OFF => Some(lg::GET_POWER_STATE),
            _ => None,
        }
    }
}

impl Bus for FakeBus {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        let reply = self.answer(uri, payload);
        if reply["returnValue"] == json!(true)
            && let Some(state) = Self::changed(uri)
        {
            self.notify(state);
        }
        Ok(reply)
    }

    fn watch(&self, subs: Vec<(&'static str, Value)>, sink: Sink) -> Option<Result<(), String>> {
        for (index, (uri, payload)) in subs.into_iter().enumerate() {
            let mut first = self.answer(uri, &payload);
            if uri == lg::GET_POWER_STATE {
                // Answers once and is polled.
                first["subscribed"] = json!(false);
                sink(index, Ok(first));
                continue;
            }
            first["subscribed"] = json!(true);
            self.subs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(Subscription {
                    uri,
                    payload,
                    index,
                    sink: Arc::clone(&sink),
                });
            sink(index, Ok(first));
        }
        Some(Ok(()))
    }
}

/// Starts the script on `bus`: the volume press, `SCRIPT_DELAY` from now.
pub fn start_script(bus: &Arc<FakeBus>) {
    if !bus.script {
        return;
    }
    let bus = Arc::clone(bus);
    let _ = thread::Builder::new()
        .name("fake-remote".to_owned())
        .spawn(move || {
            thread::sleep(SCRIPT_DELAY);
            bus.press_volume_up();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_and_pushes_like_a_tv() {
        let bus = FakeBus::new(false);
        let got: Arc<Mutex<Vec<(usize, Value)>>> = Arc::default();
        let into = Arc::clone(&got);
        let sink: Sink = Arc::new(move |i, m| into.lock().unwrap().push((i, m.unwrap())));
        bus.watch(
            vec![
                (lg::GET_POWER_STATE, json!({ "subscribe": true })),
                (lg::GET_VOLUME, json!({ "subscribe": true })),
                (
                    lg::GET_SYSTEM_SETTINGS,
                    lg::read_settings("sound", true).payload,
                ),
            ],
            sink,
        )
        .unwrap()
        .unwrap();
        {
            let first = got.lock().unwrap();
            assert_eq!(first.len(), 3);
            assert_eq!(first[0].1["subscribed"], json!(false));
            assert_eq!(first[1].1["volume"], json!(14));
            assert_eq!(first[2].1["settings"], json!({ "soundMode": "standard" }));
        }
        bus.call(lg::SET_VOLUME, &json!({ "volume": 30 })).unwrap();
        bus.press_volume_up();
        bus.call(
            lg::SET_SYSTEM_SETTINGS,
            &json!({ "category": "sound", "settings": { "soundMode": "music" } }),
        )
        .unwrap();
        let got = got.lock().unwrap();
        assert_eq!(got.len(), 6);
        assert_eq!((got[3].0, got[3].1["volume"].clone()), (1, json!(30)));
        assert_eq!(got[4].1["volume"], json!(31));
        assert_eq!(got[5].1["settings"], json!({ "soundMode": "music" }));
        // A refusal is a reply, as luna's are.
        assert_eq!(
            bus.call(lg::KEYMANAGER_KEY, &json!({})).unwrap()["errorText"],
            json!("keyCode is a required parameter")
        );
        assert_eq!(
            bus.call("luna://com.webos.service.tvpower/power/reboot", &json!({}))
                .unwrap()["returnValue"],
            json!(false)
        );
    }
}
