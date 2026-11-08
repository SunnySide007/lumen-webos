//! The bench's robot remote: `lumen-ctl play`, `key` and `watch-remote`
//! (docs/device/bench.md; remote.md R2's `uinput` route).
//!
//! A virtual remote made through `/dev/uinput` plays a pattern file with
//! fixed timing, so every bench run makes the same moves. Meanwhile every
//! other input node (the Magic Remote's, LG's RCU, the Bluetooth ones) is
//! read, never grabbed: an event there is a person, and the run yields at
//! once, letting go of any key it holds (docs/safety.md, "Yield"). An event
//! on another node with a code this run sent within `ECHO_MS` is counted as
//! an echo, not a person, in case LG's input service mirrors new devices.
//!
//! The key codes are the Linux codes the B9's Magic Remote sends (its node's
//! key map, 2026-11-08, and the evdev recordings of 2026-10-22, b9.md).
//!
//! A pattern file, one step a line, `#` for comments:
//!
//! ```text
//! repeat off            # or: repeat 500 50 (value-2 repeats while held)
//! wait 1000             # ms
//! tap right             # pressed 60 ms
//! tap down 120          # pressed 120 ms
//! hold right 2000       # pressed 2000 ms
//! taps left 5 300       # 5 taps, 300 ms from one press to the next
//! ```

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::bus::Bus;
use crate::keys::KeyRoute;
use crate::protocol::{ErrorCode, KEY_NAMES, Refusal};
use crate::sys::uinput::{EV_KEY, EV_MSC, EV_SYN, Evdev, InputEvent, Uinput};

pub const DEVICE_NAME: &str = "Lumen robot remote";

/// Names to Linux key codes. `home` is LG's 773, which the Magic Remote
/// holds 150 to 200 ms before each trip through LG's launcher.
pub const CODES: [(&str, u16); 12] = [
    ("up", 103),
    ("down", 108),
    ("left", 105),
    ("right", 106),
    ("ok", 28),
    ("back", 412),
    ("home", 773),
    ("play", 207),
    ("pause", 119),
    ("stop", 128),
    ("rewind", 168),
    ("fastforward", 208),
];

const TAP_MS: u64 = 60;
const MAX_STEP_MS: u64 = 60_000;
const MAX_LENGTH_MS: u64 = 10 * 60_000;
const MAX_LINES: usize = 500;
const MAX_FILE: u64 = 32 << 10;
const MAX_TAPS: u64 = 200;
/// An event elsewhere with a code sent this recently is an echo.
const ECHO_MS: u64 = 250;
/// How long the compositor gets to open the new device before the first key.
pub const SETTLE: Duration = Duration::from_millis(1500);
/// The longest sleep between looks at the real remote.
const SLICE: Duration = Duration::from_millis(5);

pub fn code_of(name: &str) -> Option<u16> {
    CODES.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

fn name_of(code: u16) -> &'static str {
    CODES
        .iter()
        .find(|&&(_, c)| c == code)
        .map_or("?", |&(n, _)| n)
}

/// One key event at its time from the pattern's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Emit {
    pub at_ms: u64,
    pub code: u16,
    /// 1 down, 0 up, 2 a repeat.
    pub value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub emits: Vec<Emit>,
    pub length_ms: u64,
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut emits = Vec::new();
        let mut t = 0u64;
        let mut repeat: Option<(u64, u64)> = None;
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > MAX_LINES {
            return Err(format!("more than {MAX_LINES} lines"));
        }
        for (n, raw) in lines.iter().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| format!("line {}: {why}: {line:?}", n + 1);
            let words: Vec<&str> = line.split_whitespace().collect();
            let ms = |i: usize| -> Result<u64, String> {
                words
                    .get(i)
                    .and_then(|w| w.parse::<u64>().ok())
                    .filter(|v| (1..=MAX_STEP_MS).contains(v))
                    .ok_or_else(|| bad("a time from 1 to 60000 ms"))
            };
            let key = |i: usize| -> Result<u16, String> {
                words
                    .get(i)
                    .and_then(|w| code_of(w))
                    .ok_or_else(|| bad("a key name"))
            };
            match (words[0], words.len()) {
                ("repeat", 2) if words[1] == "off" => repeat = None,
                ("repeat", 3) => repeat = Some((ms(1)?, ms(2)?)),
                ("wait", 2) => t += ms(1)?,
                ("tap", 2) => {
                    press(&mut emits, repeat, t, key(1)?, TAP_MS);
                    t += TAP_MS;
                }
                ("tap", 3) | ("hold", 3) => {
                    let held = ms(2)?;
                    press(&mut emits, repeat, t, key(1)?, held);
                    t += held;
                }
                ("taps", 4) => {
                    let code = key(1)?;
                    let count = words[2]
                        .parse::<u64>()
                        .ok()
                        .filter(|c| (1..=MAX_TAPS).contains(c))
                        .ok_or_else(|| bad("a count from 1 to 200"))?;
                    let gap = ms(3)?;
                    if gap <= TAP_MS {
                        return Err(bad("a gap longer than a tap (60 ms)"));
                    }
                    for i in 0..count {
                        press(&mut emits, repeat, t + i * gap, code, TAP_MS);
                    }
                    t += (count - 1) * gap + TAP_MS;
                }
                _ => return Err(bad("not a step")),
            }
            if t > MAX_LENGTH_MS {
                return Err(bad("the pattern runs past 10 minutes"));
            }
        }
        emits.sort_by_key(|e| (e.at_ms, e.value != 0));
        Ok(Self {
            emits,
            length_ms: t,
        })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() || meta.len() > MAX_FILE {
            return Err(format!("{}: not a file of at most 32 KiB", path.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn codes(&self) -> Vec<u16> {
        let mut c: Vec<u16> = self.emits.iter().map(|e| e.code).collect();
        c.sort_unstable();
        c.dedup();
        c
    }

    pub fn presses(&self) -> usize {
        self.emits.iter().filter(|e| e.value == 1).count()
    }
}

/// One press of `code` at `t`, held `held` ms, with value-2 repeats every
/// `period` from `delay` on when `repeat` asks for them.
fn press(emits: &mut Vec<Emit>, repeat: Option<(u64, u64)>, t: u64, code: u16, held: u64) {
    emits.push(Emit {
        at_ms: t,
        code,
        value: 1,
    });
    if let Some((delay, period)) = repeat {
        let mut r = t + delay;
        while r < t + held {
            emits.push(Emit {
                at_ms: r,
                code,
                value: 2,
            });
            r += period;
        }
    }
    emits.push(Emit {
        at_ms: t + held,
        code,
        value: 0,
    });
}

/// The input nodes in `/proc/bus/input/devices`' text, by name: every
/// `eventN` handler, with the device's name.
pub fn input_nodes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut name = None;
        let mut events = Vec::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("N: Name=") {
                name = Some(rest.trim().trim_matches('"').to_owned());
            } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
                events.extend(
                    rest.split_whitespace()
                        .filter(|h| {
                            h.strip_prefix("event").is_some_and(|n| {
                                !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
                            })
                        })
                        .map(str::to_owned),
                );
            }
        }
        if let Some(name) = name {
            for e in events {
                out.push((name.clone(), e));
            }
        }
    }
    out
}

/// Whether an event on a real node is a person: anything but the
/// separators, the scan codes and an echo of a key this run just sent.
pub fn is_person(ev: &InputEvent, sent_recently: impl Fn(u16) -> bool) -> bool {
    match ev.kind {
        EV_SYN | EV_MSC => false,
        EV_KEY => !sent_recently(ev.code),
        _ => true,
    }
}

fn epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Every input node but the robot's, opened for reading.
struct Watch {
    nodes: Vec<(String, String, Evdev)>,
    /// When each code was last sent.
    sent: BTreeMap<u16, Instant>,
    echoes: u64,
}

impl Watch {
    fn open() -> Result<Self, String> {
        let text = std::fs::read_to_string("/proc/bus/input/devices")
            .map_err(|e| format!("/proc/bus/input/devices: {e}"))?;
        let mut nodes = Vec::new();
        for (name, event) in input_nodes(&text) {
            if name == DEVICE_NAME {
                return Err(format!(
                    "another {DEVICE_NAME} exists ({event}): is a run going on?"
                ));
            }
            let path = PathBuf::from("/dev/input").join(&event);
            match Evdev::open(&path) {
                Ok(dev) => nodes.push((name, event, dev)),
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        }
        if nodes.is_empty() {
            return Err("no input node to watch: no way to yield, so no run".to_owned());
        }
        Ok(Self {
            nodes,
            sent: BTreeMap::new(),
            echoes: 0,
        })
    }

    /// The first person seen since the last look, if any.
    fn person(&mut self) -> Result<Option<serde_json::Value>, String> {
        let now = Instant::now();
        let sent = &self.sent;
        let recent = |code: u16| {
            sent.get(&code)
                .is_some_and(|t| now.duration_since(*t) <= Duration::from_millis(ECHO_MS))
        };
        for (name, event, dev) in &mut self.nodes {
            let events = dev.read_ready().map_err(|e| format!("{event}: {e}"))?;
            for ev in events {
                if is_person(&ev, recent) {
                    return Ok(Some(json!({
                        "node": event, "name": name, "type": ev.kind,
                        "code": ev.code, "value": ev.value, "at": ev.at,
                    })));
                }
                if ev.kind == EV_KEY {
                    self.echoes += 1;
                }
            }
        }
        Ok(None)
    }

    /// Sleeps until `until`, looking for a person every `SLICE`.
    fn wait_until(&mut self, until: Instant) -> Result<Option<serde_json::Value>, String> {
        loop {
            if let Some(p) = self.person()? {
                return Ok(Some(p));
            }
            let now = Instant::now();
            if now >= until {
                return Ok(None);
            }
            thread::sleep(SLICE.min(until - now));
        }
    }
}

fn say(out: &mut impl Write, v: &serde_json::Value) {
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// Plays `pattern` on a new virtual remote. Exit 0 when played, 3 when a
/// person yielded it, 1 on an error; one JSON line per stage on stdout.
pub fn play(pattern: &Pattern, settle: Duration, out: &mut impl Write) -> ExitCode {
    match play_inner(pattern, settle, out) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(3),
        Err(e) => {
            say(out, &json!({ "error": e }));
            ExitCode::from(1)
        }
    }
}

fn play_inner(pattern: &Pattern, settle: Duration, out: &mut impl Write) -> Result<bool, String> {
    // The real nodes are opened before the robot's exists, so it is never
    // among them.
    let mut watch = Watch::open()?;
    let mut kb =
        Uinput::create(DEVICE_NAME, &pattern.codes()).map_err(|e| format!("/dev/uinput: {e}"))?;
    let watched: Vec<&str> = watch.nodes.iter().map(|(_, e, _)| e.as_str()).collect();
    say(
        out,
        &json!({ "ready": DEVICE_NAME, "watching": watched, "settle_ms": settle.as_millis() as u64,
                 "presses": pattern.presses(), "length_ms": pattern.length_ms }),
    );
    if let Some(p) = watch.wait_until(Instant::now() + settle)? {
        say(out, &json!({ "yield": p, "played": 0 }));
        return Ok(false);
    }
    let start = Instant::now();
    let mut held: Vec<u16> = Vec::new();
    let (mut first, mut last, mut played) = (None::<f64>, None::<f64>, 0usize);
    for e in &pattern.emits {
        let due = start + Duration::from_millis(e.at_ms);
        if let Some(p) = watch.wait_until(due)? {
            for code in held.drain(..) {
                let _ = kb.emit(code, 0);
            }
            say(
                out,
                &json!({ "yield": p, "played": played, "first": first, "last": last,
                         "echoes": watch.echoes }),
            );
            return Ok(false);
        }
        kb.emit(e.code, e.value)
            .map_err(|err| format!("writing {}: {err}", name_of(e.code)))?;
        watch.sent.insert(e.code, Instant::now());
        match e.value {
            1 => {
                let now = epoch();
                first.get_or_insert(now);
                last = Some(now);
                played += 1;
                held.push(e.code);
            }
            0 => held.retain(|&c| c != e.code),
            _ => {}
        }
    }
    // Echoes of the last keys come after them.
    let tail = watch.wait_until(Instant::now() + Duration::from_millis(ECHO_MS))?;
    let late = start.elapsed().as_millis() as u64;
    say(
        out,
        &json!({ "done": played, "first": first, "last": last, "late_ms": late.saturating_sub(pattern.length_ms),
                 "echoes": watch.echoes, "person_after": tail }),
    );
    drop(kb);
    Ok(true)
}

/// Prints every person's event on the real nodes for `secs`, one JSON line
/// each, with the kernel's stamp (target 5's Home press, a lab check).
pub fn watch_remote(secs: u64, out: &mut impl Write) -> ExitCode {
    let mut watch = match Watch::open() {
        Ok(w) => w,
        Err(e) => {
            say(out, &json!({ "error": e }));
            return ExitCode::from(1);
        }
    };
    let until = Instant::now() + Duration::from_secs(secs);
    let mut seen = 0u64;
    while Instant::now() < until {
        for (name, event, dev) in &mut watch.nodes {
            match dev.read_ready() {
                Ok(events) => {
                    for ev in events.iter().filter(|ev| is_person(ev, |_| false)) {
                        seen += 1;
                        say(
                            out,
                            &json!({ "node": event, "name": name, "type": ev.kind,
                                     "code": ev.code, "value": ev.value, "at": ev.at }),
                        );
                    }
                }
                Err(e) => {
                    say(out, &json!({ "error": format!("{event}: {e}") }));
                    return ExitCode::from(1);
                }
            }
        }
        thread::sleep(SLICE);
    }
    say(out, &json!({ "watched_s": secs, "events": seen }));
    ExitCode::SUCCESS
}

/// `serve --keys uinput`: the Remote's `key` verb on one virtual remote,
/// made at the first press and kept for the session, so only that press
/// waits `SETTLE` for the compositor to find it. A key the Magic Remote has
/// no code for (`menu`) is refused as unavailable.
pub struct UinputKeys {
    device: Mutex<Option<Uinput>>,
    settle: Duration,
}

impl UinputKeys {
    pub fn new(settle: Duration) -> Self {
        Self {
            device: Mutex::new(None),
            settle,
        }
    }
}

impl KeyRoute for UinputKeys {
    fn name(&self) -> &'static str {
        "uinput"
    }

    fn any(&self) -> bool {
        KEY_NAMES.iter().any(|k| code_of(k).is_some())
    }

    fn press(&self, _: &dyn Bus, key: &'static str) -> Result<(), Refusal> {
        let code = code_of(key).ok_or_else(|| {
            Refusal::new(
                ErrorCode::Unavailable,
                format!("the remote has no code for {key}"),
            )
        })?;
        let mut device = self.device.lock().unwrap_or_else(|e| e.into_inner());
        if device.is_none() {
            let codes: Vec<u16> = CODES.iter().map(|&(_, c)| c).collect();
            let made = Uinput::create(DEVICE_NAME, &codes)
                .map_err(|e| Refusal::new(ErrorCode::Failed, format!("/dev/uinput: {e}")))?;
            *device = Some(made);
            thread::sleep(self.settle);
        }
        let Some(dev) = device.as_mut() else {
            return Err(Refusal::new(ErrorCode::Failed, "no virtual remote"));
        };
        let fail = |e: std::io::Error| Refusal::new(ErrorCode::Failed, format!("{key}: {e}"));
        dev.emit(code, 1).map_err(fail)?;
        thread::sleep(Duration::from_millis(TAP_MS));
        dev.emit(code, 0).map_err(fail)
    }
}

/// A pattern of one press of `name`, held `held_ms`.
pub fn one_key(name: &str, held_ms: u64) -> Result<Pattern, String> {
    if code_of(name).is_none() {
        return Err(format!(
            "{name:?} is not one of {}",
            CODES.map(|(n, _)| n).join(", ")
        ));
    }
    Pattern::parse(&format!("tap {name} {held_ms}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emits(text: &str) -> Vec<(u64, u16, i32)> {
        Pattern::parse(text)
            .unwrap()
            .emits
            .iter()
            .map(|e| (e.at_ms, e.code, e.value))
            .collect()
    }

    #[test]
    fn steps_become_timed_key_events() {
        let p = Pattern::parse(
            "# a comment\nwait 1000\ntap right\nwait 200 # gap\nhold down 500\ntaps left 2 300\n",
        )
        .unwrap();
        assert_eq!(
            p.emits
                .iter()
                .map(|e| (e.at_ms, e.code, e.value))
                .collect::<Vec<_>>(),
            vec![
                (1000, 106, 1),
                (1060, 106, 0),
                (1260, 108, 1),
                (1760, 108, 0),
                (1760, 105, 1),
                (1820, 105, 0),
                (2060, 105, 1),
                (2120, 105, 0),
            ]
        );
        assert_eq!(p.length_ms, 2120);
        assert_eq!(p.presses(), 4);
        assert_eq!(p.codes(), vec![105, 106, 108]);
    }

    #[test]
    fn a_release_and_a_press_at_one_time_go_release_first() {
        assert_eq!(
            emits("hold up 100\ntap down"),
            vec![(0, 103, 1), (100, 103, 0), (100, 108, 1), (160, 108, 0)]
        );
    }

    #[test]
    fn repeats_fill_a_hold_when_asked() {
        assert_eq!(
            emits("repeat 500 50\nhold right 620\nrepeat off\nhold left 620"),
            vec![
                (0, 106, 1),
                (500, 106, 2),
                (550, 106, 2),
                (600, 106, 2),
                (620, 106, 0),
                (620, 105, 1),
                (1240, 105, 0),
            ]
        );
    }

    #[test]
    fn nonsense_is_refused_with_its_line() {
        for (bad, line) in [
            ("tap power", 1),
            ("wait 0", 1),
            ("wait 70000", 1),
            ("tap up\nhold up", 2),
            ("taps up 0 300", 1),
            ("taps up 3 50", 1),
            ("jump up", 1),
            ("repeat 500", 1),
        ] {
            let err = Pattern::parse(bad).unwrap_err();
            assert!(err.starts_with(&format!("line {line}:")), "{bad:?}: {err}");
        }
        let long = "wait 60000\n".repeat(11);
        assert!(Pattern::parse(&long).unwrap_err().contains("10 minutes"));
        assert!(Pattern::parse(&"tap up\n".repeat(501)).is_err());
    }

    #[test]
    fn one_key_knows_only_the_remotes_names() {
        assert_eq!(
            one_key("home", 180).unwrap().emits,
            vec![
                Emit {
                    at_ms: 0,
                    code: 773,
                    value: 1
                },
                Emit {
                    at_ms: 180,
                    code: 773,
                    value: 0
                },
            ]
        );
        assert!(one_key("power", 60).is_err());
    }

    #[test]
    fn the_uinput_route_offers_keys_and_refuses_one_without_a_code() {
        let route = UinputKeys::new(Duration::ZERO);
        assert_eq!(route.name(), "uinput");
        assert!(route.any());
        let bus = crate::fake::FakeBus::new(false);
        assert_eq!(
            route.press(&bus, "menu").unwrap_err().code,
            ErrorCode::Unavailable
        );
        for name in KEY_NAMES {
            assert!(
                name == "menu" || code_of(name).is_some(),
                "{name} has no code"
            );
        }
    }

    #[test]
    fn the_b9s_input_list_names_its_nodes() {
        let text = "I: Bus=0006 Vendor=0001 Product=0001 Version=0004\nN: Name=\"CHECK INPUT\"\nH: Handlers=kbd event0 \nB: EV=3\n\nI: Bus=0003\nN: Name=\"LGE M-RCU - Builtin [0]\"\nH: Handlers=kbd mouse0 event3 \nB: EV=f\n\nI: Bus=0003\nN: Name=\"Nothing\"\nH: Handlers=kbd\n";
        assert_eq!(
            input_nodes(text),
            vec![
                ("CHECK INPUT".to_owned(), "event0".to_owned()),
                ("LGE M-RCU - Builtin [0]".to_owned(), "event3".to_owned()),
            ]
        );
    }

    #[test]
    fn a_person_is_anything_but_separators_and_echoes() {
        let ev = |kind, code| InputEvent {
            at: 0.0,
            kind,
            code,
            value: 1,
        };
        assert!(is_person(&ev(EV_KEY, 106), |_| false));
        assert!(!is_person(&ev(EV_KEY, 106), |c| c == 106));
        assert!(is_person(&ev(EV_KEY, 108), |c| c == 106));
        assert!(!is_person(&ev(EV_SYN, 0), |_| false));
        assert!(!is_person(&ev(EV_MSC, 4), |_| false));
        assert!(is_person(&ev(3, 0), |_| true), "pointer motion");
        assert!(is_person(&ev(2, 8), |_| true), "the wheel");
    }
}
