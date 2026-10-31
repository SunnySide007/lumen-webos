//! LG's side of each verb: the luna methods R0 read on the B9
//! (docs/device/b9.md, "Remote"), the payloads lumen-ctl sends them, and how
//! their replies become the protocol's values.
//!
//! Every payload is built here from typed values the request check let
//! through; no string from a client reaches luna unless it is an app id
//! that LG's own list named, or text that `text.insert` or `toast` checked.
//! Strings from LG (titles, labels, error texts) are cleaned and capped
//! before they go out.

use serde_json::{Map, Value, json};

use crate::protocol::{self, Kind, Setting, SettingValue};

/// Lumen's own app, which `lumen.open` launches.
pub const LUMEN_ID: &str = "io.github.sunnyside007.lumen.shell";

pub const GET_POWER_STATE: &str = "luna://com.webos.service.tvpower/power/getPowerState";
pub const POWER_OFF: &str = "luna://com.webos.service.tvpower/power/powerOff";
pub const TURN_OFF_SCREEN: &str = "luna://com.webos.service.tvpower/power/turnOffScreen";
pub const TURN_ON_SCREEN: &str = "luna://com.webos.service.tvpower/power/turnOnScreen";
pub const GET_VOLUME: &str = "luna://com.webos.audio/getVolume";
pub const SET_VOLUME: &str = "luna://com.webos.audio/setVolume";
pub const VOLUME_UP: &str = "luna://com.webos.audio/volumeUp";
pub const VOLUME_DOWN: &str = "luna://com.webos.audio/volumeDown";
pub const SET_MUTED: &str = "luna://com.webos.audio/setMuted";
pub const LIST_LAUNCH_POINTS: &str = "luna://com.webos.applicationManager/listLaunchPoints";
pub const LAUNCH: &str = "luna://com.webos.applicationManager/launch";
pub const GET_FOREGROUND_APP: &str = "luna://com.webos.applicationManager/getForegroundAppInfo";
pub const GET_ALL_INPUT_STATUS: &str = "luna://com.webos.service.eim/getAllInputStatus";
pub const INSERT_TEXT: &str = "luna://com.webos.service.ime/insertText";
pub const DELETE_CHARACTERS: &str = "luna://com.webos.service.ime/deleteCharacters";
pub const SEND_ENTER_KEY: &str = "luna://com.webos.service.ime/sendEnterKey";
pub const CREATE_TOAST: &str = "luna://com.webos.notification/createToast";
pub const GET_SYSTEM_SETTINGS: &str = "luna://com.webos.settingsservice/getSystemSettings";
pub const SET_SYSTEM_SETTINGS: &str = "luna://com.webos.settingsservice/setSystemSettings";
pub const GET_SYSTEM_SETTING_DESC: &str = "luna://com.webos.settingsservice/getSystemSettingDesc";
pub const KEYMANAGER_KEY: &str = "luna://com.webos.service.tv.keymanager/createKeyEvent";
pub const LEGACYINPUT_KEY: &str = "luna://com.webos.service.tv.legacyinput/createKeyEvent";

/// The longest string from LG that goes out: titles, labels, states, reasons.
pub const MAX_TEXT: usize = 200;
/// The longest title or label in `apps.list` and `inputs`.
pub const MAX_TITLE: usize = 64;

/// One call: a fixed method and the payload the code built.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub uri: &'static str,
    pub payload: Value,
}

fn call(uri: &'static str, payload: Value) -> Call {
    Call { uri, payload }
}

/// `powerOff` takes a reason on webOS 4; `remoteKey` is the power key's.
/// Unconfirmed on the B9: LG's error says so if it wants another.
pub fn power_off() -> Call {
    call(POWER_OFF, json!({ "reason": "remoteKey" }))
}

pub fn screen(on: bool) -> Call {
    call(if on { TURN_ON_SCREEN } else { TURN_OFF_SCREEN }, json!({}))
}

pub fn volume_step(up: bool) -> Call {
    call(if up { VOLUME_UP } else { VOLUME_DOWN }, json!({}))
}

pub fn set_volume(level: u8) -> Call {
    call(SET_VOLUME, json!({ "volume": level }))
}

pub fn set_muted(muted: bool) -> Call {
    call(SET_MUTED, json!({ "muted": muted }))
}

pub fn list_launch_points() -> Call {
    call(LIST_LAUNCH_POINTS, json!({}))
}

pub fn list_inputs() -> Call {
    call(GET_ALL_INPUT_STATUS, json!({}))
}

/// `id` is one LG's own list named (or Lumen's, for `lumen.open`).
pub fn launch(id: &str) -> Call {
    call(LAUNCH, json!({ "id": id }))
}

pub fn insert_text(text: &str) -> Call {
    call(INSERT_TEXT, json!({ "text": text }))
}

pub fn delete_characters(count: u16) -> Call {
    call(DELETE_CHARACTERS, json!({ "count": count }))
}

pub fn send_enter_key() -> Call {
    call(SEND_ENTER_KEY, json!({}))
}

/// A toast in Lumen's name. Unconfirmed on the B9: `sourceId` is what a
/// caller that is not an app passes on webOS 4.
pub fn toast(text: &str) -> Call {
    call(
        CREATE_TOAST,
        json!({ "sourceId": LUMEN_ID, "message": text }),
    )
}

/// A reply's error text, unless it says it succeeded.
pub fn check(reply: &Value) -> Result<(), String> {
    if reply.get("returnValue").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    Err(error_text(reply))
}

/// A subscription's later messages may leave `returnValue` out; only an
/// explicit false is a failure.
pub fn check_message(reply: &Value) -> Result<(), String> {
    if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
        Err(error_text(reply))
    } else {
        Ok(())
    }
}

fn error_text(reply: &Value) -> String {
    let text = reply
        .get("errorText")
        .and_then(Value::as_str)
        .map(|t| clean(t, MAX_TEXT))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "no reason given".to_owned());
    match reply.get("errorCode").and_then(Value::as_i64) {
        Some(code) => format!("{text} (error {code})"),
        None => text,
    }
}

/// Another service's string, fit for one line: control characters and line
/// or paragraph separators made spaces, trimmed, and cut to `max` characters.
pub fn clean(text: &str, max: usize) -> String {
    text.trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn list<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

// ---------------------------------------------------------------- power

/// LG's `getPowerState` state as one of `POWER_STATES`. "Screen Saver" was
/// read on the B9; "Active", "Screen Off" and "Suspend" are the likely names
/// of the others, to confirm on the TV. Anything else counts as on, and the
/// event carries LG's word beside it.
pub fn power_state(lg: &str) -> &'static str {
    match lg {
        "Screen Off" => "screen-off",
        "Screen Saver" => "screensaver",
        "Suspend" => "standby",
        _ => "active",
    }
}

/// The `power` event's data from a `getPowerState` reply, or `None` when it
/// names no state.
pub fn power(reply: &Value) -> Option<Value> {
    let lg = clean(str_field(reply, "state")?, MAX_TEXT);
    if lg.is_empty() {
        return None;
    }
    let state = power_state(&lg);
    debug_assert!(protocol::POWER_STATES.contains(&state));
    Some(json!({ "state": state, "lg": lg }))
}

// ---------------------------------------------------------------- volume

/// `getVolume`'s level and mute, each if present: webOS 4 puts both at the
/// top (`volume`, `muted`, as read on the B9); later versions nest them in
/// `volumeStatus` (`volume`, `muteStatus`).
pub fn volume(reply: &Value) -> (Option<u8>, Option<bool>) {
    let nested = reply.get("volumeStatus");
    let level = reply
        .get("volume")
        .or_else(|| nested.and_then(|n| n.get("volume")))
        .and_then(Value::as_u64)
        .filter(|v| *v <= 100)
        .map(|v| v as u8);
    let muted = reply
        .get("muted")
        .or_else(|| nested.and_then(|n| n.get("muteStatus")))
        .and_then(Value::as_bool);
    (level, muted)
}

// ---------------------------------------------------------------- the app in front

/// An app id from LG, kept only if it is one the protocol could launch.
fn app_id(v: Option<&str>) -> Option<String> {
    v.filter(|id| protocol::is_app_id(id)).map(str::to_owned)
}

/// `getForegroundAppInfo`'s `appId`: `Some(None)` when LG names none (an
/// empty id, between two apps), `None` when the message carries no id.
pub fn foreground(reply: &Value) -> Option<Option<String>> {
    let id = str_field(reply, "appId")?;
    Some(app_id(Some(id)))
}

// ---------------------------------------------------------------- inputs and apps

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub id: String,
    pub label: String,
    pub device: Option<String>,
    pub connected: bool,
}

impl Input {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "device": self.device,
            "connected": self.connected,
        })
    }
}

/// `getAllInputStatus`'s devices, or `None` when the reply has no list.
///
/// * `id` is the input's app (`com.webos.app.hdmi1`), which `launch` takes.
/// * `label` is the name the owner gave it.
/// * `device` is what CEC or LG's set-top list says is plugged in: the first
///   sub-device's `osdName`, `labelName` or `brandName`.
/// * `connected` is `hdmiPlugIn` where LG gives it (the cable): on the B9
///   `connected` is true for every HDMI input, cable or not.
pub fn inputs(reply: &Value) -> Option<Vec<Input>> {
    let devices = reply.get("devices")?.as_array()?;
    let mut out: Vec<Input> = Vec::new();
    for d in devices {
        let Some(id) = app_id(str_field(d, "appId")) else {
            continue;
        };
        if out.iter().any(|i| i.id == id) {
            continue;
        }
        let label = str_field(d, "label")
            .map(|l| clean(l, MAX_TITLE))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| id.clone());
        let device = list(d, "subList").next().and_then(|sub| {
            ["osdName", "labelName", "brandName"]
                .iter()
                .filter_map(|k| str_field(sub, k))
                .map(|name| clean(name, MAX_TITLE))
                .find(|name| !name.is_empty())
        });
        let connected = d
            .get("hdmiPlugIn")
            .and_then(Value::as_bool)
            .or_else(|| d.get("connected").and_then(Value::as_bool))
            .unwrap_or(false);
        out.push(Input {
            id,
            label,
            device,
            connected,
        });
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub title: String,
    pub input: bool,
    /// `#rrggbb`, lower case.
    pub color: Option<String>,
}

impl App {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "kind": if self.input { "input" } else { "app" },
            "iconColor": self.color,
        })
    }
}

fn color(text: &str) -> Option<String> {
    let hex = text.strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", hex.to_ascii_lowercase()))
}

/// `apps.list`: LG's launch points in LG's order (apps only: `lptype`
/// `default`, as the shell lists them; bookmarks and channels are left out),
/// then the inputs. Ids the protocol could not launch are left out.
pub fn apps(points: &Value, inputs: Option<&[Input]>) -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for p in list(points, "launchPoints") {
        let Some(id) = app_id(str_field(p, "id")) else {
            continue;
        };
        if str_field(p, "lptype").unwrap_or("default") != "default"
            || out.iter().any(|a| a.id == id)
        {
            continue;
        }
        let title = str_field(p, "title")
            .map(|t| clean(t, MAX_TITLE))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.clone());
        out.push(App {
            title,
            color: str_field(p, "iconColor").and_then(color),
            id,
            input: false,
        });
    }
    for i in inputs.unwrap_or_default() {
        if !out.iter().any(|a| a.id == i.id) {
            out.push(App {
                id: i.id.clone(),
                title: i.label.clone(),
                input: true,
                color: None,
            });
        }
    }
    out
}

// ---------------------------------------------------------------- settings

/// The picture's dynamic range, from `dimension.dynamicRange`: LG keeps a
/// picture mode per range, under different names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Sdr,
    Hdr,
    Dolby,
}

impl Range {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sdr => "SDR",
            Self::Hdr => "HDR",
            Self::Dolby => "Dolby Vision",
        }
    }
}

/// `sdr` was read on the B9; the HDR names are likely, not read. A range
/// lumen-ctl cannot place (Technicolor's, or a new name) is `Err`, and no
/// picture mode is written for it.
pub fn range(reply: &Value) -> Result<Range, String> {
    let raw = reply
        .get("dimension")
        .and_then(|d| d.get("dynamicRange"))
        .and_then(Value::as_str)
        .ok_or_else(|| "LG's reply names no dynamic range".to_owned())?;
    if raw == "sdr" {
        Ok(Range::Sdr)
    } else if raw.starts_with("dolby") {
        Ok(Range::Dolby)
    } else if raw.starts_with("hdr") || raw == "hlg" {
        Ok(Range::Hdr)
    } else {
        Err(format!(
            "an unknown dynamic range, {}",
            clean(raw, MAX_TITLE)
        ))
    }
}

/// One of Lumen's values and LG's names for it, most likely first, in SDR,
/// HDR and Dolby Vision. A setting that does not depend on the range has its
/// names in `sdr` only.
struct Choice {
    lumen: &'static str,
    sdr: &'static [&'static str],
    hdr: &'static [&'static str],
    dolby: &'static [&'static str],
}

const fn any(lumen: &'static str, names: &'static [&'static str]) -> Choice {
    Choice {
        lumen,
        sdr: names,
        hdr: &[],
        dolby: &[],
    }
}

/// How one of Lumen's settings is kept in LG's settingsservice.
pub struct LgSetting {
    pub setting: &'static Setting,
    pub category: &'static str,
    pub key: &'static str,
    pub by_range: bool,
    choices: &'static [Choice],
    /// LG's values for this key read on the B9 on 2026-10-31 (for
    /// `pictureMode`, the modes `pictureSettingModified` lists). Written
    /// only these when LG's description of the key can't be read.
    seen: &'static [&'static str],
}

/// The fixed mapping, in protocol.ts's order. The report lists it, with
/// which names the B9's dump confirmed.
pub static LG_SETTINGS: [LgSetting; 4] = [
    LgSetting {
        setting: &protocol::SETTINGS[0],
        category: "picture",
        key: "pictureMode",
        by_range: true,
        choices: &[
            Choice {
                lumen: "vivid",
                sdr: &["vivid"],
                hdr: &["hdrVivid"],
                dolby: &["dolbyHdrVivid"],
            },
            Choice {
                lumen: "standard",
                sdr: &["normal"],
                hdr: &["hdrStandard"],
                dolby: &["dolbyHdrStandard"],
            },
            Choice {
                lumen: "eco",
                sdr: &["eco"],
                hdr: &[],
                dolby: &[],
            },
            Choice {
                lumen: "cinema-home",
                sdr: &[],
                hdr: &["hdrCinemaBright"],
                dolby: &["dolbyHdrCinemaBright"],
            },
            Choice {
                lumen: "cinema",
                sdr: &["cinema"],
                hdr: &["hdrCinema"],
                dolby: &["dolbyHdrCinema"],
            },
            Choice {
                lumen: "sports",
                sdr: &["sports"],
                hdr: &[],
                dolby: &[],
            },
            Choice {
                lumen: "game",
                sdr: &["game"],
                hdr: &["hdrGame"],
                dolby: &["dolbyHdrGame"],
            },
            Choice {
                lumen: "filmmaker",
                sdr: &["filmMaker"],
                hdr: &["hdrFilmMaker"],
                dolby: &[],
            },
            Choice {
                lumen: "expert-bright",
                sdr: &["expert1"],
                hdr: &[],
                dolby: &[],
            },
            Choice {
                lumen: "expert-dark",
                sdr: &["expert2"],
                hdr: &[],
                dolby: &[],
            },
        ],
        seen: &[
            "vivid",
            "normal",
            "eco",
            "cinema",
            "sports",
            "game",
            "expert1",
            "expert2",
            "hdrVivid",
            "hdrStandard",
            "hdrCinemaBright",
            "hdrCinema",
            "hdrGame",
            "dolbyHdrVivid",
            "dolbyHdrStandard",
            "dolbyHdrCinemaBright",
            "dolbyHdrCinema",
            "dolbyHdrGame",
        ],
    },
    LgSetting {
        setting: &protocol::SETTINGS[1],
        category: "picture",
        key: "backlight",
        by_range: false,
        choices: &[],
        seen: &[],
    },
    LgSetting {
        setting: &protocol::SETTINGS[2],
        category: "picture",
        key: "energySaving",
        by_range: false,
        choices: &[
            any("auto", &["auto"]),
            any("off", &["off"]),
            any("minimum", &["min", "minimum"]),
            any("medium", &["med", "medium"]),
            any("maximum", &["max", "maximum"]),
        ],
        seen: &["off"],
    },
    LgSetting {
        setting: &protocol::SETTINGS[3],
        category: "sound",
        key: "soundMode",
        by_range: false,
        choices: &[
            any("ai", &["aiSoundPlus", "aiSound"]),
            any("standard", &["standard"]),
            any("cinema", &["movie", "cinema"]),
            any("clear-voice", &["news", "clearVoice"]),
            any("sports", &["sports", "sport"]),
            any("music", &["music"]),
            any("game", &["game"]),
        ],
        seen: &["standard"],
    },
];

pub fn lg_setting(setting: &Setting) -> &'static LgSetting {
    LG_SETTINGS
        .iter()
        .find(|s| s.setting.key == setting.key)
        .expect("every setting has an LG mapping")
}

/// The settings of one category that `getSystemSettings` reads.
pub fn read_settings(category: &str, subscribe: bool) -> Call {
    let keys: Vec<&str> = LG_SETTINGS
        .iter()
        .filter(|s| s.category == category)
        .map(|s| s.key)
        .collect();
    let mut payload = json!({ "category": category, "keys": keys });
    if subscribe {
        payload["subscribe"] = json!(true);
    }
    call(GET_SYSTEM_SETTINGS, payload)
}

/// The picture mode alone, for its `dimension`.
pub fn read_dimension() -> Call {
    call(
        GET_SYSTEM_SETTINGS,
        json!({ "category": "picture", "keys": ["pictureMode"] }),
    )
}

pub fn read_desc(s: &LgSetting) -> Call {
    call(
        GET_SYSTEM_SETTING_DESC,
        json!({ "category": s.category, "keys": [s.key] }),
    )
}

/// LG's value as one of Lumen's, or `None` when the table has no name for it
/// (the B9's "HDR Effect" picture mode, `hdrEffect`, say).
pub fn to_lumen(s: &LgSetting, lg: &Value) -> Option<SettingValue> {
    match s.setting.kind {
        Kind::Int { min, max } => {
            let n = match lg {
                Value::String(text) => text.trim().parse::<u8>().ok()?,
                Value::Number(n) => u8::try_from(n.as_u64()?).ok()?,
                _ => return None,
            };
            (min..=max).contains(&n).then_some(SettingValue::Int(n))
        }
        Kind::Enum(_) => {
            let lg = lg.as_str()?;
            s.choices
                .iter()
                .find(|c| {
                    [c.sdr, c.hdr, c.dolby]
                        .iter()
                        .any(|names| names.contains(&lg))
                })
                .map(|c| SettingValue::Name(c.lumen))
        }
    }
}

/// LG's names for one of Lumen's values in `range`, most likely first.
pub fn candidates(s: &LgSetting, value: &str, range: Range) -> &'static [&'static str] {
    let Some(c) = s.choices.iter().find(|c| c.lumen == value) else {
        return &[];
    };
    if !s.by_range {
        return c.sdr;
    }
    match range {
        Range::Sdr => c.sdr,
        Range::Hdr => c.hdr,
        Range::Dolby => c.dolby,
    }
}

/// The values LG's description of `key` lists as usable (`values.arrayExt`
/// entries not marked inactive or hidden, or `values.array`), or `None` when
/// the reply lists none. The shape is webOS's settingsservice's; the B9's is
/// unread, and `probe` prints it.
pub fn desc_values(reply: &Value, key: &str) -> Option<Vec<String>> {
    let entry = list(reply, "results").find(|r| str_field(r, "key") == Some(key))?;
    let values = entry.get("values")?;
    if let Some(ext) = values.get("arrayExt").and_then(Value::as_array) {
        return Some(
            ext.iter()
                .filter(|e| e.get("active") != Some(&Value::Bool(false)))
                .filter(|e| e.get("visible") != Some(&Value::Bool(false)))
                .filter_map(|e| str_field(e, "value").map(str::to_owned))
                .collect(),
        );
    }
    let plain = values.get("array").and_then(Value::as_array)?;
    Some(
        plain
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    )
}

/// LG's name to write for `value`: the first candidate LG's description lists,
/// or, when the description can't be read, the first the B9 was seen to use.
/// `Err` says why there is none.
pub fn pick(
    s: &LgSetting,
    value: &str,
    range: Range,
    desc: Option<&[String]>,
) -> Result<&'static str, String> {
    let names = candidates(s, value, range);
    let what = || {
        if s.by_range {
            format!("{} {value} in {}", s.setting.key, range.name())
        } else {
            format!("{} {value}", s.setting.key)
        }
    };
    if names.is_empty() {
        return Err(format!("LG has no {} on this TV", what()));
    }
    let found = match desc {
        Some(listed) => names.iter().find(|n| listed.iter().any(|l| l == *n)),
        None => names.iter().find(|n| s.seen.contains(n)),
    };
    found.copied().ok_or_else(|| match desc {
        Some(_) => format!("this TV's list has no {} ({})", what(), names.join(" or ")),
        None => format!(
            "{} ({}) is not confirmed on this TV, and LG's list of values could not be read",
            what(),
            names.join(" or ")
        ),
    })
}

/// The `setSystemSettings` call for one value, as LG keeps it: the picture's
/// numbers as strings (`backlight` read `"100"` on the B9).
pub fn write_setting(s: &LgSetting, lg_value: Value) -> Call {
    let mut settings = Map::new();
    settings.insert(s.key.to_owned(), lg_value);
    call(
        SET_SYSTEM_SETTINGS,
        json!({ "category": s.category, "settings": settings }),
    )
}

pub fn int_value(n: u8) -> Value {
    json!(n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn payloads_are_built_from_typed_values() {
        assert_eq!(set_volume(20).payload, json!({ "volume": 20 }));
        assert_eq!(set_volume(20).uri, "luna://com.webos.audio/setVolume");
        assert_eq!(set_muted(true).payload, json!({ "muted": true }));
        assert_eq!(volume_step(true).uri, VOLUME_UP);
        assert_eq!(volume_step(false).uri, VOLUME_DOWN);
        assert_eq!(screen(false).uri, TURN_OFF_SCREEN);
        assert_eq!(screen(true).uri, TURN_ON_SCREEN);
        assert_eq!(power_off().payload, json!({ "reason": "remoteKey" }));
        assert_eq!(launch("netflix").payload, json!({ "id": "netflix" }));
        assert_eq!(delete_characters(3).payload, json!({ "count": 3 }));
        // A quote or a brace in text stays inside the JSON string.
        let t = insert_text(r#"a"}, "x": {"#);
        assert_eq!(t.payload["text"], json!(r#"a"}, "x": {"#));
        assert_eq!(t.payload.as_object().unwrap().len(), 1);
        assert_eq!(
            toast("Volume 20").payload,
            json!({ "sourceId": LUMEN_ID, "message": "Volume 20" })
        );
    }

    #[test]
    fn every_uri_is_a_fixed_luna_method() {
        for uri in [
            GET_POWER_STATE,
            POWER_OFF,
            TURN_OFF_SCREEN,
            TURN_ON_SCREEN,
            GET_VOLUME,
            SET_VOLUME,
            VOLUME_UP,
            VOLUME_DOWN,
            SET_MUTED,
            LIST_LAUNCH_POINTS,
            LAUNCH,
            GET_FOREGROUND_APP,
            GET_ALL_INPUT_STATUS,
            INSERT_TEXT,
            DELETE_CHARACTERS,
            SEND_ENTER_KEY,
            CREATE_TOAST,
            GET_SYSTEM_SETTINGS,
            SET_SYSTEM_SETTINGS,
            GET_SYSTEM_SETTING_DESC,
            KEYMANAGER_KEY,
            LEGACYINPUT_KEY,
        ] {
            let rest = uri.strip_prefix("luna://com.webos.").unwrap();
            assert!(
                rest.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'/'),
                "{uri}"
            );
        }
    }

    #[test]
    fn checks_replies() {
        assert_eq!(check(&parse(r#"{"returnValue":true}"#)), Ok(()));
        assert_eq!(
            check(&parse(
                r#"{"returnValue":false,"errorCode":-1,"errorText":"Unknown method \"getStatus\" for category \"/\""}"#
            )),
            Err(r#"Unknown method "getStatus" for category "/" (error -1)"#.to_owned())
        );
        assert_eq!(
            check(&parse(r#"{"returnValue":false,"errorText":"two\nlines"}"#)),
            Err("two lines".to_owned())
        );
        assert_eq!(check(&parse("{}")), Err("no reason given".to_owned()));
        assert_eq!(check_message(&parse(r#"{"volume":3}"#)), Ok(()));
        assert!(check_message(&parse(r#"{"returnValue":false}"#)).is_err());
    }

    #[test]
    fn maps_lgs_power_states() {
        assert_eq!(
            power(&parse(r#"{"state":"Screen Saver","returnValue":true}"#)),
            Some(json!({ "state": "screensaver", "lg": "Screen Saver" }))
        );
        for (lg, ours) in [
            ("Active", "active"),
            ("Screen Off", "screen-off"),
            ("Screen Saver", "screensaver"),
            ("Suspend", "standby"),
            // Unknown: on, with LG's word beside it.
            ("Active Standby", "active"),
            ("Something New", "active"),
        ] {
            assert_eq!(power_state(lg), ours, "{lg}");
            assert!(protocol::POWER_STATES.contains(&ours));
        }
        assert_eq!(power(&parse(r#"{"returnValue":true}"#)), None);
        assert_eq!(power(&parse(r#"{"state":""}"#)), None);
        assert_eq!(
            power(&parse(r#"{"state":"Odd\u0000state"}"#)).unwrap()["lg"],
            json!("Odd state")
        );
    }

    #[test]
    fn reads_the_volume_as_the_b9_answers_and_nested() {
        assert_eq!(
            volume(&parse(
                r#"{"scenario":"mastervolume_tv_speaker_ext","muted":false,"volume":6,"returnValue":true}"#
            )),
            (Some(6), Some(false))
        );
        assert_eq!(
            volume(&parse(
                r#"{"volumeStatus":{"volume":12,"muteStatus":true}}"#
            )),
            (Some(12), Some(true))
        );
        assert_eq!(volume(&parse(r#"{"muted":true}"#)), (None, Some(true)));
        assert_eq!(volume(&parse(r#"{"volume":101}"#)), (None, None));
        assert_eq!(volume(&parse(r#"{"volume":-1}"#)), (None, None));
    }

    #[test]
    fn reads_the_app_in_front() {
        assert_eq!(
            foreground(&parse(
                r#"{"appId":"io.github.sunnyside007.lumen.shell","returnValue":true,"windowId":"","processId":""}"#
            )),
            Some(Some(LUMEN_ID.to_owned()))
        );
        assert_eq!(foreground(&parse(r#"{"appId":""}"#)), Some(None));
        assert_eq!(foreground(&parse(r#"{"appId":"a b"}"#)), Some(None));
        assert_eq!(foreground(&parse(r#"{"returnValue":true}"#)), None);
    }

    const B9_INPUTS: &str = r#"{
        "returnValue": true,
        "devices": [
            {"id": "AV_1", "appId": "com.webos.app.externalinput.av1", "label": "AV", "connected": false, "subList": []},
            {"id": "HDMI_1", "appId": "com.webos.app.hdmi1", "label": "PC", "connected": true, "hdmiPlugIn": false, "subList": []},
            {"id": "HDMI_2", "appId": "com.webos.app.hdmi2", "label": "Sound bar", "connected": true, "hdmiPlugIn": true,
             "subList": [{"id": "URCU", "labelName": "Acme", "brandName": "Acme Audio"}]},
            {"id": "HDMI_3", "appId": "com.webos.app.hdmi3", "label": "Streamer ", "connected": true, "hdmiPlugIn": true,
             "subList": [{"id": "SIMPLINK", "osdName": "Living\u0007Room"}]},
            {"id": "HDMI_4", "appId": "com.webos.app.hdmi4", "label": "", "connected": true, "hdmiPlugIn": false},
            {"id": "BAD", "appId": "../x", "label": "nope"},
            {"id": "HDMI_1b", "appId": "com.webos.app.hdmi1", "label": "again"}
        ]
    }"#;

    #[test]
    fn reads_the_inputs_as_the_b9_shapes_them() {
        let got = inputs(&parse(B9_INPUTS)).unwrap();
        let ids: Vec<&str> = got.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "com.webos.app.externalinput.av1",
                "com.webos.app.hdmi1",
                "com.webos.app.hdmi2",
                "com.webos.app.hdmi3",
                "com.webos.app.hdmi4"
            ]
        );
        assert!(!got[0].connected);
        // The cable, not `connected`, says whether something is plugged in.
        assert!(!got[1].connected && got[2].connected && got[3].connected && !got[4].connected);
        assert_eq!(got[2].device.as_deref(), Some("Acme"));
        assert_eq!(got[3].device.as_deref(), Some("Living Room"));
        assert_eq!(got[3].label, "Streamer");
        assert_eq!(got[4].label, "com.webos.app.hdmi4");
        assert_eq!(got[1].device, None);
        assert_eq!(
            got[3].to_json(),
            json!({ "id": "com.webos.app.hdmi3", "label": "Streamer", "device": "Living Room", "connected": true })
        );
        assert_eq!(inputs(&parse(r#"{"returnValue":true}"#)), None);
    }

    #[test]
    fn lists_lgs_apps_then_the_inputs() {
        let points = parse(
            r##"{"launchPoints": [
                {"id": "netflix", "title": "Netflix", "iconColor": "#E50914", "lptype": "default"},
                {"id": "youtube.leanback.v4", "title": "YouTube\n", "iconColor": "red"},
                {"id": "bookmark.1", "title": "A page", "lptype": "bookmark"},
                {"id": "netflix", "title": "Netflix again"},
                {"id": "x y", "title": "Bad id"},
                {"id": "com.webos.app.hdmi1", "title": "HDMI 1"},
                {"id": "cdp-30"}
            ]}"##,
        );
        let ins = inputs(&parse(B9_INPUTS)).unwrap();
        let got = apps(&points, Some(&ins));
        let ids: Vec<&str> = got.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "netflix",
                "youtube.leanback.v4",
                "com.webos.app.hdmi1",
                "cdp-30",
                "com.webos.app.externalinput.av1",
                "com.webos.app.hdmi2",
                "com.webos.app.hdmi3",
                "com.webos.app.hdmi4"
            ]
        );
        assert_eq!(
            got[0].to_json(),
            json!({ "id": "netflix", "title": "Netflix", "kind": "app", "iconColor": "#e50914" })
        );
        assert_eq!(got[1].title, "YouTube");
        assert_eq!(got[1].color, None);
        assert_eq!(got[3].title, "cdp-30");
        // An input listed as a launch point stays an app; the others follow.
        assert!(!got[2].input);
        assert_eq!(got[6].to_json()["kind"], json!("input"));
        assert!(apps(&points, None).iter().all(|a| !a.input));
    }

    #[test]
    fn reads_the_dynamic_range() {
        let with = |r: &str| range(&json!({ "dimension": { "dynamicRange": r } }));
        assert_eq!(with("sdr"), Ok(Range::Sdr));
        assert_eq!(with("hdr10"), Ok(Range::Hdr));
        assert_eq!(with("hlg"), Ok(Range::Hdr));
        assert_eq!(with("dolbyHdr"), Ok(Range::Dolby));
        assert!(with("technicolorHdr").is_err());
        assert!(range(&json!({ "settings": {} })).is_err());
    }

    fn lg(key: &str) -> &'static LgSetting {
        LG_SETTINGS.iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn every_lumen_value_has_a_place_in_the_table() {
        for s in &LG_SETTINGS {
            let Kind::Enum(values) = s.setting.kind else {
                continue;
            };
            // Exactly protocol.ts's values, in its order.
            let lumen: Vec<&str> = s.choices.iter().map(|c| c.lumen).collect();
            assert_eq!(lumen, values, "{}", s.key);
            // Every value has an LG name somewhere; seen names are mapped.
            for c in s.choices {
                assert!(!(c.sdr.is_empty() && c.hdr.is_empty() && c.dolby.is_empty()));
            }
            for seen in s.seen {
                assert!(to_lumen(s, &json!(seen)).is_some(), "{seen}");
            }
        }
        assert_eq!(lg_setting(&protocol::SETTINGS[1]).key, "backlight");
    }

    #[test]
    fn maps_settings_from_lg_to_lumen() {
        let mode = lg("pictureMode");
        for (lg_name, ours) in [
            ("normal", "standard"),
            ("hdrStandard", "standard"),
            ("dolbyHdrStandard", "standard"),
            ("expert1", "expert-bright"),
            ("expert2", "expert-dark"),
            ("hdrCinemaBright", "cinema-home"),
            ("dolbyHdrCinema", "cinema"),
            ("vivid", "vivid"),
        ] {
            assert_eq!(
                to_lumen(mode, &json!(lg_name)),
                Some(SettingValue::Name(ours)),
                "{lg_name}"
            );
        }
        // The B9's own mode on 2026-10-31 has no name in Lumen's list.
        assert_eq!(to_lumen(mode, &json!("hdrEffect")), None);
        let light = lg("backlight");
        assert_eq!(to_lumen(light, &json!("100")), Some(SettingValue::Int(100)));
        assert_eq!(to_lumen(light, &json!(80)), Some(SettingValue::Int(80)));
        assert_eq!(to_lumen(light, &json!("101")), None);
        assert_eq!(to_lumen(light, &json!("-1")), None);
        assert_eq!(
            to_lumen(lg("energySaving"), &json!("off")),
            Some(SettingValue::Name("off"))
        );
        assert_eq!(
            to_lumen(lg("energySaving"), &json!("max")),
            Some(SettingValue::Name("maximum"))
        );
        assert_eq!(
            to_lumen(lg("soundMode"), &json!("movie")),
            Some(SettingValue::Name("cinema"))
        );
        assert_eq!(to_lumen(lg("soundMode"), &json!(3)), None);
    }

    #[test]
    fn picks_lgs_name_by_range_and_by_what_the_tv_lists() {
        let mode = lg("pictureMode");
        // Seen on the B9: written without a description.
        assert_eq!(pick(mode, "standard", Range::Sdr, None), Ok("normal"));
        assert_eq!(pick(mode, "standard", Range::Hdr, None), Ok("hdrStandard"));
        assert_eq!(
            pick(mode, "cinema-home", Range::Dolby, None),
            Ok("dolbyHdrCinemaBright")
        );
        // No Cinema Home in SDR, no Eco in HDR.
        assert!(
            pick(mode, "cinema-home", Range::Sdr, None)
                .unwrap_err()
                .contains("no picture.mode cinema-home in SDR")
        );
        assert!(pick(mode, "eco", Range::Hdr, None).is_err());
        // Not seen, and no list: refused.
        assert!(
            pick(mode, "filmmaker", Range::Sdr, None)
                .unwrap_err()
                .contains("not confirmed")
        );
        // The TV's list decides when it can be read.
        let listed = vec!["filmMaker".to_owned(), "normal".to_owned()];
        assert_eq!(
            pick(mode, "filmmaker", Range::Sdr, Some(&listed)),
            Ok("filmMaker")
        );
        assert!(
            pick(mode, "vivid", Range::Sdr, Some(&listed))
                .unwrap_err()
                .contains("this TV's list has no")
        );
        // The first candidate the list has, for the uncertain spellings.
        let saving = lg("energySaving");
        let listed = vec!["auto".to_owned(), "minimum".to_owned()];
        assert_eq!(
            pick(saving, "minimum", Range::Sdr, Some(&listed)),
            Ok("minimum")
        );
        assert!(pick(saving, "minimum", Range::Sdr, None).is_err());
        assert_eq!(pick(saving, "off", Range::Hdr, None), Ok("off"));
        assert_eq!(
            pick(lg("soundMode"), "standard", Range::Sdr, None),
            Ok("standard")
        );
    }

    #[test]
    fn reads_lgs_description_of_a_setting() {
        let reply = parse(
            r#"{"returnValue": true, "results": [
                {"key": "pictureMode", "values": {"arrayExt": [
                    {"value": "vivid", "visible": true, "active": true},
                    {"value": "normal"},
                    {"value": "hidden", "visible": false},
                    {"value": "off", "active": false},
                    {"nothing": 1}
                ]}},
                {"key": "soundMode", "values": {"array": ["standard", "music", 3]}},
                {"key": "backlight", "values": {"range": {"min": 0, "max": 100}}}
            ]}"#,
        );
        assert_eq!(
            desc_values(&reply, "pictureMode"),
            Some(vec!["vivid".to_owned(), "normal".to_owned()])
        );
        assert_eq!(
            desc_values(&reply, "soundMode"),
            Some(vec!["standard".to_owned(), "music".to_owned()])
        );
        assert_eq!(desc_values(&reply, "backlight"), None);
        assert_eq!(desc_values(&reply, "energySaving"), None);
        assert_eq!(desc_values(&parse(r#"{"returnValue":false}"#), "x"), None);
    }

    #[test]
    fn writes_settings_as_lg_keeps_them() {
        assert_eq!(
            write_setting(lg("backlight"), int_value(80)),
            Call {
                uri: SET_SYSTEM_SETTINGS,
                payload: json!({ "category": "picture", "settings": { "backlight": "80" } }),
            }
        );
        assert_eq!(
            write_setting(lg("soundMode"), json!("standard")).payload,
            json!({ "category": "sound", "settings": { "soundMode": "standard" } })
        );
        assert_eq!(
            read_settings("picture", true).payload,
            json!({ "category": "picture", "keys": ["pictureMode", "backlight", "energySaving"], "subscribe": true })
        );
        assert_eq!(
            read_settings("sound", false).payload,
            json!({ "category": "sound", "keys": ["soundMode"] })
        );
        assert_eq!(
            read_desc(lg("pictureMode")).payload,
            json!({ "category": "picture", "keys": ["pictureMode"] })
        );
    }

    #[test]
    fn cleans_lgs_strings() {
        assert_eq!(clean("  a\tb\u{2028}c  ", 10), "a b c");
        assert_eq!(clean(&"x".repeat(300), 5), "xxxxx");
        assert_eq!(clean("é👋é", 2), "é👋");
    }
}
