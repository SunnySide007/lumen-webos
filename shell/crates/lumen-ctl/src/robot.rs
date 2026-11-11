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
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::bus::Bus;
use crate::keys::{KeyRoute, no_pointer};
use crate::protocol::{ErrorCode, KEY_NAMES, POINTER_SPAN, Refusal};
use crate::sys::uinput::{
    Axis, EV_KEY, EV_MSC, EV_SYN, Evdev, Inject, InputEvent, KeySink, Uinput,
};

pub const DEVICE_NAME: &str = "Lumen robot remote";

/// Names to Linux key codes. `home` is LG's 773, which the Magic Remote
/// holds 150 to 200 ms before each trip through LG's launcher.
///
/// The second group is Linux's own codes for keys the Magic Remote's node
/// declares in its key map (read on 2026-11-11) but that were not in the
/// 2026-10-22 recordings: the number keys, the channel rocker, the four
/// colours, Info and Exit. `menu` is the gear: 139 (KEY_MENU), which the
/// owner's four presses of it sent on 2026-11-11, each down and up within
/// 0.3 ms.
pub const CODES: [(&str, u16); 31] = [
    ("up", 103),
    ("down", 108),
    ("left", 105),
    ("right", 106),
    ("ok", 28),
    ("back", 412),
    ("home", 773),
    ("menu", 139),
    ("play", 207),
    ("pause", 119),
    ("stop", 128),
    ("rewind", 168),
    ("fastforward", 208),
    ("info", 358),
    ("exit", 174),
    ("channelup", 402),
    ("channeldown", 403),
    ("red", 398),
    ("green", 399),
    ("yellow", 400),
    ("blue", 401),
    ("0", 11),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
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
    // The real nodes are opened before a virtual remote could exist, so it
    // is never among them. Keys written into the Magic Remote's node come
    // back to the watch there, as echoes.
    let mut watch = Watch::open()?;
    let (mut kb, via) = open_sink(&pattern.codes())?;
    let watched: Vec<&str> = watch.nodes.iter().map(|(_, e, _)| e.as_str()).collect();
    say(
        out,
        &json!({ "ready": DEVICE_NAME, "via": via, "watching": watched, "settle_ms": settle.as_millis() as u64,
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

/// The node the Magic Remote's keys arrive on, by its name in
/// `/proc/bus/input/devices`.
pub const MAGIC_REMOTE: &str = "LGE M-RCU";

/// A place keys go, and its name for the log.
type Sink = (Box<dyn KeySink>, String);

/// The Magic Remote's node, opened for writing.
fn open_magic() -> Result<Option<Sink>, String> {
    let text = std::fs::read_to_string("/proc/bus/input/devices")
        .map_err(|e| format!("/proc/bus/input/devices: {e}"))?;
    let Some((name, event)) = input_nodes(&text)
        .into_iter()
        .find(|(name, _)| name.starts_with(MAGIC_REMOTE))
    else {
        return Ok(None);
    };
    let path = PathBuf::from("/dev/input").join(&event);
    let sink = Inject::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some((Box::new(sink), format!("{event} ({name})"))))
}

/// Where the robot's keys go: written into the Magic Remote's own node,
/// which LG's compositor has held open since boot (it opens no node that
/// appears later, and the B9 runs no udev; 2026-11-08), or, on a TV without
/// one, a new virtual remote through `/dev/uinput`. Also says which.
pub fn open_sink(codes: &[u16]) -> Result<Sink, String> {
    if let Some(magic) = open_magic()? {
        return Ok(magic);
    }
    let sink = Uinput::create(DEVICE_NAME, codes).map_err(|e| format!("/dev/uinput: {e}"))?;
    Ok((Box::new(sink), "uinput".to_owned()))
}

/// `serve --keys inject`: the Remote's `key` verb through `open_sink`,
/// opened at the first press and kept for the session. A key the Magic
/// Remote has no code for would be refused as unavailable; today every
/// name has one.
///
/// The pointer goes the same way, as the Magic Remote sends its own (b9.md):
/// LG's 1198 when it appears, its place as absolute X and Y, a click as
/// `BTN_LEFT`, and LG's 1199 once it has rested `POINTER_REST`. It starts
/// from where the kernel last had it, so it never jumps from under a hand
/// on the real remote. Only the remote's own node has a pointer; a virtual
/// remote has none.
pub struct RobotKeys {
    hand: Arc<Mutex<Hand>>,
    settle: Duration,
    rest: Duration,
}

/// LG's codes for the pointer showing and hiding, and Linux's click.
pub const POINTER_SHOW: u16 = 1198;
pub const POINTER_HIDE: u16 = 1199;
pub const BTN_LEFT: u16 = 272;
/// How long after its last move the Magic Remote hides its pointer (about
/// 5 s in the recordings of 2026-10-22).
pub const POINTER_REST: Duration = Duration::from_secs(5);

struct Hand {
    device: Option<Box<dyn KeySink>>,
    /// The pointer is up by this route's doing, and owes a 1199.
    shown: bool,
    /// A key went since the last move: LG may have hidden the pointer, so
    /// the next move says 1198 again.
    stale: bool,
    moved: Instant,
    at: (i32, i32),
    /// The thread that hides a rested pointer is running.
    hider: bool,
}

/// `at` moved by `d` pixels of a screen `span` wide, in the axis's units
/// and held inside it.
pub fn step(axis: Axis, at: i32, d: i16, span: i64) -> i32 {
    let range = i64::from(axis.max) - i64::from(axis.min);
    // To the nearest unit, halves away from zero.
    let scaled = (i64::from(d) * range + i64::from(d.signum()) * span / 2) / span;
    (i64::from(at) + scaled).clamp(i64::from(axis.min), i64::from(axis.max)) as i32
}

fn tap(dev: &mut dyn KeySink, code: u16, held: Duration) -> std::io::Result<()> {
    dev.emit(code, 1)?;
    if !held.is_zero() {
        thread::sleep(held);
    }
    dev.emit(code, 0)
}

impl RobotKeys {
    /// `settle`: the wait after making a virtual remote, for a compositor
    /// that finds new ones; none when writing into the remote's own node.
    pub fn new(settle: Duration) -> Self {
        Self::with(None, settle, POINTER_REST)
    }

    fn with(device: Option<Box<dyn KeySink>>, settle: Duration, rest: Duration) -> Self {
        Self {
            hand: Arc::new(Mutex::new(Hand {
                device,
                shown: false,
                stale: false,
                moved: Instant::now(),
                at: (0, 0),
                hider: false,
            })),
            settle,
            rest,
        }
    }

    /// The remote's own node and its axes; never a virtual remote.
    fn pointer_axes(hand: &mut Hand) -> Result<(Axis, Axis), Refusal> {
        if hand.device.is_none() {
            let magic = open_magic().map_err(|e| Refusal::new(ErrorCode::Failed, e))?;
            hand.device = magic.map(|(dev, _)| dev);
        }
        hand.device
            .as_mut()
            .and_then(|dev| dev.axes())
            .ok_or_else(no_pointer)
    }

    /// Hides the pointer once it has rested, as the remote itself does.
    fn hide_when_rested(&self) {
        let (hand, rest) = (Arc::clone(&self.hand), self.rest);
        let look = rest.min(Duration::from_millis(250));
        let run = move || loop {
            thread::sleep(look);
            let mut hand = hand.lock().unwrap_or_else(|e| e.into_inner());
            if hand.shown && hand.moved.elapsed() < rest {
                continue;
            }
            if hand.shown {
                if let Some(dev) = hand.device.as_mut() {
                    let _ = tap(&mut **dev, POINTER_HIDE, Duration::ZERO);
                }
                hand.shown = false;
            }
            hand.hider = false;
            return;
        };
        if thread::Builder::new()
            .name("pointer-rest".to_owned())
            .spawn(run)
            .is_err()
        {
            let mut hand = self.hand.lock().unwrap_or_else(|e| e.into_inner());
            hand.hider = false;
        }
    }
}

impl KeyRoute for RobotKeys {
    fn name(&self) -> &'static str {
        "inject"
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
        let mut hand = self.hand.lock().unwrap_or_else(|e| e.into_inner());
        if hand.device.is_none() {
            let codes: Vec<u16> = CODES.iter().map(|&(_, c)| c).collect();
            let (made, via) = open_sink(&codes).map_err(|e| Refusal::new(ErrorCode::Failed, e))?;
            hand.device = Some(made);
            if via == "uinput" {
                thread::sleep(self.settle);
            }
        }
        hand.stale = true;
        let Some(dev) = hand.device.as_mut() else {
            return Err(Refusal::new(ErrorCode::Failed, "no virtual remote"));
        };
        tap(&mut **dev, code, Duration::from_millis(TAP_MS))
            .map_err(|e| Refusal::new(ErrorCode::Failed, format!("{key}: {e}")))
    }

    fn pointer(&self) -> bool {
        let mut hand = self.hand.lock().unwrap_or_else(|e| e.into_inner());
        Self::pointer_axes(&mut hand).is_ok()
    }

    fn point(&self, dx: i16, dy: i16) -> Result<(), Refusal> {
        let fail = |e: std::io::Error| Refusal::new(ErrorCode::Failed, format!("pointer: {e}"));
        let mut hand = self.hand.lock().unwrap_or_else(|e| e.into_inner());
        let (x, y) = Self::pointer_axes(&mut hand)?;
        let (was_shown, stale) = (hand.shown, hand.stale);
        if !was_shown {
            hand.at = (x.value, y.value);
        }
        let to = (
            step(x, hand.at.0, dx, POINTER_SPAN.0),
            step(y, hand.at.1, dy, POINTER_SPAN.1),
        );
        let Some(dev) = hand.device.as_mut() else {
            return Err(no_pointer());
        };
        if !was_shown || stale {
            tap(&mut **dev, POINTER_SHOW, Duration::ZERO).map_err(fail)?;
        }
        dev.point(to.0, to.1).map_err(fail)?;
        hand.at = to;
        hand.shown = true;
        hand.stale = false;
        hand.moved = Instant::now();
        let start = !hand.hider;
        hand.hider = true;
        drop(hand);
        if start {
            self.hide_when_rested();
        }
        Ok(())
    }

    fn click(&self) -> Result<(), Refusal> {
        let mut hand = self.hand.lock().unwrap_or_else(|e| e.into_inner());
        Self::pointer_axes(&mut hand)?;
        // A click keeps a shown pointer up, as a hand on the remote would.
        hand.moved = Instant::now();
        let Some(dev) = hand.device.as_mut() else {
            return Err(no_pointer());
        };
        tap(&mut **dev, BTN_LEFT, Duration::from_millis(TAP_MS))
            .map_err(|e| Refusal::new(ErrorCode::Failed, format!("click: {e}")))
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
    fn the_robot_route_offers_keys_and_has_a_code_for_every_name() {
        let route = RobotKeys::new(Duration::ZERO);
        assert_eq!(route.name(), "inject");
        assert!(route.any());
        for name in KEY_NAMES {
            assert!(code_of(name).is_some(), "{name} has no code");
        }
        assert_eq!(code_of("menu"), Some(139));
        assert_eq!(code_of("power"), None);
        assert_eq!(CODES.len(), KEY_NAMES.len());
    }

    /// What a route wrote: `K code value` for a key, `P x y` for a place.
    type Tape = Arc<Mutex<Vec<String>>>;

    struct Recorder {
        tape: Tape,
        axes: Option<(Axis, Axis)>,
    }

    impl KeySink for Recorder {
        fn emit(&mut self, code: u16, value: i32) -> std::io::Result<()> {
            self.tape.lock().unwrap().push(format!("K {code} {value}"));
            Ok(())
        }

        fn axes(&mut self) -> Option<(Axis, Axis)> {
            self.axes
        }

        fn point(&mut self, x: i32, y: i32) -> std::io::Result<()> {
            self.tape.lock().unwrap().push(format!("P {x} {y}"));
            Ok(())
        }
    }

    fn recorded(axes: Option<(Axis, Axis)>, rest: Duration) -> (RobotKeys, Tape) {
        let tape = Tape::default();
        let sink = Recorder {
            tape: Arc::clone(&tape),
            axes,
        };
        (
            RobotKeys::with(Some(Box::new(sink)), Duration::ZERO, rest),
            tape,
        )
    }

    fn screen(x: i32, y: i32) -> Option<(Axis, Axis)> {
        Some((
            Axis {
                value: x,
                min: 0,
                max: 1919,
            },
            Axis {
                value: y,
                min: 0,
                max: 1079,
            },
        ))
    }

    #[test]
    fn a_step_scales_to_the_axis_and_stays_inside_it() {
        let wide = Axis {
            value: 0,
            min: 0,
            max: 3840,
        };
        assert_eq!(step(wide, 100, 10, 1920), 120);
        assert_eq!(step(wide, 100, -10, 1920), 80);
        assert_eq!(step(wide, 100, -1920, 1920), 0);
        assert_eq!(step(wide, 3800, 1920, 1920), 3840);
        let narrow = Axis {
            value: 0,
            min: -100,
            max: 100,
        };
        // 200 units over 1920 pixels: 5 pixels round to 1, 4 to 0.
        assert_eq!(step(narrow, 0, 5, 1920), 1);
        assert_eq!(step(narrow, 0, -5, 1920), -1);
        assert_eq!(step(narrow, 0, 4, 1920), 0);
        assert_eq!(step(narrow, -100, -1920, 1920), -100);
    }

    #[test]
    fn the_pointer_shows_once_then_moves_from_where_the_kernel_had_it() {
        let (route, tape) = recorded(screen(960, 540), Duration::from_secs(60));
        assert!(route.pointer());
        route.point(10, -20).unwrap();
        route.point(5, 5).unwrap();
        route.point(1920, 1080).unwrap();
        route.click().unwrap();
        assert_eq!(
            *tape.lock().unwrap(),
            [
                "K 1198 1",
                "K 1198 0",
                "P 970 520",
                "P 975 525",
                "P 1919 1079",
                "K 272 1",
                "K 272 0",
            ]
        );
    }

    #[test]
    fn a_key_between_moves_shows_the_pointer_again() {
        let (route, tape) = recorded(screen(100, 100), Duration::from_secs(60));
        let bus = crate::fake::FakeBus::new(false);
        route.point(1, 1).unwrap();
        route.press(&bus, "down").unwrap();
        route.point(1, 1).unwrap();
        assert_eq!(
            *tape.lock().unwrap(),
            [
                "K 1198 1",
                "K 1198 0",
                "P 101 101",
                "K 108 1",
                "K 108 0",
                "K 1198 1",
                "K 1198 0",
                "P 102 102",
            ]
        );
    }

    #[test]
    fn a_rested_pointer_is_hidden_and_the_next_move_shows_it_again() {
        let (route, tape) = recorded(screen(100, 100), Duration::from_millis(30));
        route.point(1, 1).unwrap();
        let hidden = |tape: &Tape| tape.lock().unwrap().iter().any(|l| l == "K 1199 0");
        let until = Instant::now() + Duration::from_secs(5);
        while !hidden(&tape) && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            *tape.lock().unwrap(),
            ["K 1198 1", "K 1198 0", "P 101 101", "K 1199 1", "K 1199 0"]
        );
        route.point(1, 1).unwrap();
        assert_eq!(
            tape.lock().unwrap()[5..8],
            ["K 1198 1", "K 1198 0", "P 101 101"]
        );
    }

    #[test]
    fn a_remote_without_a_pointer_refuses_the_pointer() {
        let (route, tape) = recorded(None, Duration::from_secs(60));
        assert!(!route.pointer());
        assert_eq!(route.point(1, 1).unwrap_err().code, ErrorCode::Unavailable);
        assert_eq!(route.click().unwrap_err().code, ErrorCode::Unavailable);
        assert!(tape.lock().unwrap().is_empty());
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

    #[test]
    fn the_benchs_patterns_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/bench");
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !(name.starts_with("pattern-") && name.ends_with(".txt")) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let p = Pattern::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                (30_000..=60_000).contains(&p.length_ms),
                "{name}: {} ms",
                p.length_ms
            );
            seen += 1;
        }
        assert!(seen >= 3, "found {seen} patterns in {}", dir.display());
    }
}
