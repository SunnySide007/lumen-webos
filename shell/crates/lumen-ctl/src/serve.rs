//! `lumen-ctl serve`: one session of the protocol on stdin and stdout, for
//! as long as the SSH session that runs it.
//!
//! 1. The subscriptions open (on a bus that has them), and their first
//!    replies are awaited for up to 3 s; a state whose method did not say
//!    `"subscribed": true` is polled instead.
//! 2. `hello` goes out, with the verbs this TV takes and the states that are
//!    polled, then the current value of each state.
//! 3. The main thread reads requests, one line each, and answers each in
//!    turn; a ticker sends `alive` every 10 s; a poller reads the polled
//!    states.
//!
//! The session ends when stdin ends, when a line is over 64 KiB, when stdout
//! is gone, or at SIGTERM, SIGHUP or SIGINT; the ticker sees the last three
//! within 50 ms, whatever the main thread is blocked in, and ends the process
//! with `_exit`.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::args::{BusKind, KeysKind, Serve};
use crate::bus::{self, Bus, CommandBus, Runner, Sink, Spawn};
use crate::fake::{self, FakeBus};
use crate::keys::{KeyRoute, NoKeys, Template};
use crate::lg::{self, App, LgSetting, Range};
use crate::log::{self, Log};
use crate::privacy;
use crate::protocol::{
    self, ErrorCode, MAX_LINE_BYTES, PROTOCOL_VERSION, Refusal, Setting, SettingValue, VERBS, Verb,
};
use crate::sys;
use crate::watch::{self, Source, States};

/// How long `serve` waits for the subscriptions' first replies.
const FIRST_WAIT: Duration = Duration::from_secs(3);
/// How long `apps.list`'s answer stands for `launch`.
const APPS_FRESH: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_millis(50);

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// stdout, one whole line at a time from any thread.
pub struct Out {
    w: Mutex<Box<dyn Write + Send>>,
    gone: AtomicBool,
}

impl Out {
    pub fn new(w: Box<dyn Write + Send>) -> Self {
        Self {
            w: Mutex::new(w),
            gone: AtomicBool::new(false),
        }
    }

    /// Writes `line` and a newline, and flushes; false once the client is
    /// gone, after which nothing more is written.
    pub fn line(&self, line: &str) -> bool {
        let mut w = lock(&self.w);
        if self.gone.load(Ordering::SeqCst) {
            return false;
        }
        let sent = w
            .write_all(line.as_bytes())
            .and_then(|()| w.write_all(b"\n"))
            .and_then(|()| w.flush());
        if sent.is_err() {
            self.gone.store(true, Ordering::SeqCst);
        }
        sent.is_ok()
    }

    pub fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }
}

/// Where every message about LG's state lands, from a subscription or a poll.
pub struct Events {
    states: Mutex<States>,
    out: Arc<Out>,
    log: Arc<Mutex<Log>>,
    /// Set once `hello` and the snapshot are out; nothing is pushed before.
    ready: AtomicBool,
    polled: [AtomicBool; Source::ALL.len()],
    /// Whether each source's last read failed and was logged.
    failing: Mutex<[bool; Source::ALL.len()]>,
}

impl Events {
    pub fn new(out: Arc<Out>, log: Arc<Mutex<Log>>) -> Self {
        Self {
            states: Mutex::new(States::default()),
            out,
            log,
            ready: AtomicBool::new(false),
            polled: Default::default(),
            failing: Mutex::new([false; Source::ALL.len()]),
        }
    }

    pub fn polled(&self, src: Source) -> bool {
        self.polled[src.index()].load(Ordering::SeqCst)
    }

    pub fn poll(&self, src: Source) {
        self.polled[src.index()].store(true, Ordering::SeqCst);
    }

    /// One message about `src`. A failed pushed message turns that state to
    /// polling; a failed read is logged once a run of failures.
    pub fn message(&self, src: Source, msg: Result<Value, String>, pushed: bool) {
        let msg = msg.and_then(|m| lg::check_message(&m).map(|()| m));
        let i = src.index();
        match msg {
            Ok(m) => {
                lock(&self.failing)[i] = false;
                let mut states = lock(&self.states);
                if let Some(line) = states.update(src, &m)
                    && self.ready.load(Ordering::SeqCst)
                {
                    self.out.line(&line);
                }
            }
            Err(e) if pushed && !self.polled(src) => {
                self.poll(src);
                lock(&self.log).line(&format!(
                    "{} {}: the subscription failed ({e}); polled from now on",
                    src.state(),
                    method(src)
                ));
            }
            Err(e) => {
                if !std::mem::replace(&mut lock(&self.failing)[i], true) {
                    lock(&self.log).line(&format!("{} {}: {e}", src.state(), method(src)));
                }
            }
        }
    }

    /// Writes `hello` and the snapshot, then lets changes through. Under the
    /// states' lock, so no change falls between the two.
    pub fn start(&self, hello: &str) {
        let states = lock(&self.states);
        self.out.line(hello);
        for line in states.snapshot() {
            self.out.line(&line);
        }
        self.ready.store(true, Ordering::SeqCst);
    }

    pub fn settings(&self, keys: &[&'static str]) -> Value {
        lock(&self.states).read(keys)
    }
}

fn method(src: Source) -> &'static str {
    let uri = src.call(false).uri;
    uri.rsplit('/').next().unwrap_or(uri)
}

fn failed(e: impl Into<String>) -> Refusal {
    Refusal::new(ErrorCode::Failed, e)
}

fn unavailable(e: impl Into<String>) -> Refusal {
    Refusal::new(ErrorCode::Unavailable, e)
}

/// The verbs, each a fixed call or two on the bus.
pub struct Ctl {
    bus: Arc<dyn Bus>,
    keys: Box<dyn KeyRoute>,
    events: Arc<Events>,
    paths: privacy::Paths,
    runner: Box<dyn Runner>,
    apps: Mutex<Option<(Instant, Vec<String>)>>,
    desc: Mutex<HashMap<&'static str, Option<Vec<String>>>>,
}

/// A verb's answer, and the state to read again once the reply is out.
pub type Answer = (Result<Value, Refusal>, Option<Source>);

impl Ctl {
    pub fn new(
        bus: Arc<dyn Bus>,
        keys: Box<dyn KeyRoute>,
        events: Arc<Events>,
        paths: privacy::Paths,
        runner: Box<dyn Runner>,
    ) -> Self {
        Self {
            bus,
            keys,
            events,
            paths,
            runner,
            apps: Mutex::new(None),
            desc: Mutex::new(HashMap::new()),
        }
    }

    /// The verbs this TV takes, for `hello`: all but `key` without a route,
    /// and `ambient.start`, which waits for the ambient track.
    pub fn verbs(&self) -> Vec<&'static str> {
        VERBS
            .iter()
            .copied()
            .filter(|v| *v != "ambient.start" && (*v != "key" || self.keys.any()))
            .collect()
    }

    fn done(&self, c: &lg::Call) -> Result<Value, Refusal> {
        bus::checked(&*self.bus, c)
            .map(|_| json!({}))
            .map_err(failed)
    }

    pub fn handle(&self, verb: &Verb) -> Answer {
        match verb {
            Verb::PowerOff => (self.done(&lg::power_off()), Some(Source::Power)),
            Verb::ScreenOff => (self.done(&lg::screen(false)), Some(Source::Power)),
            Verb::ScreenOn => (self.done(&lg::screen(true)), Some(Source::Power)),
            Verb::VolumeUp => (self.done(&lg::volume_step(true)), Some(Source::Volume)),
            Verb::VolumeDown => (self.done(&lg::volume_step(false)), Some(Source::Volume)),
            Verb::VolumeSet(level) => (self.done(&lg::set_volume(*level)), Some(Source::Volume)),
            Verb::MuteSet(muted) => (self.done(&lg::set_muted(*muted)), Some(Source::Volume)),
            Verb::AppsList => (
                self.list_apps().map(
                    |apps| json!({ "apps": apps.iter().map(App::to_json).collect::<Vec<_>>() }),
                ),
                None,
            ),
            Verb::Launch(id) => (self.launch(id), Some(Source::Foreground)),
            Verb::Key(name) => (self.keys.press(&*self.bus, name).map(|()| json!({})), None),
            Verb::TextInsert(text) => (self.done(&lg::insert_text(text)), None),
            Verb::TextDelete(count) => (self.done(&lg::delete_characters(*count)), None),
            Verb::TextEnter => (self.done(&lg::send_enter_key()), None),
            Verb::LumenOpen => (
                self.done(&lg::launch(lg::LUMEN_ID)),
                Some(Source::Foreground),
            ),
            Verb::AmbientStart => (
                Err(unavailable(
                    "ambient.start waits for Lumen's ambient screensaver (remote.md, risk 11)",
                )),
                None,
            ),
            Verb::PrivacyState => (Ok(privacy::read(&self.paths, &*self.runner)), None),
            Verb::SettingsGet(keys) => (self.settings_get(keys.as_deref()), None),
            Verb::SettingsSet(s, v) => {
                let lg = lg::lg_setting(s);
                let src = if lg.category == "sound" {
                    Source::Sound
                } else {
                    Source::Picture
                };
                (self.settings_set(lg, *v), Some(src))
            }
            Verb::Toast(text) => (self.done(&lg::toast(text)), None),
        }
    }

    /// Reads `src` once and pushes what changed.
    pub fn refresh(&self, src: Source) {
        let c = src.call(false);
        self.events
            .message(src, self.bus.call(c.uri, &c.payload), false);
    }

    /// LG's launch points, then the inputs (whose read may fail alone).
    fn list_apps(&self) -> Result<Vec<App>, Refusal> {
        let points = bus::checked(&*self.bus, &lg::list_launch_points()).map_err(failed)?;
        let inputs = bus::checked(&*self.bus, &lg::list_inputs())
            .ok()
            .and_then(|r| lg::inputs(&r));
        let apps = lg::apps(&points, inputs.as_deref());
        *lock(&self.apps) = Some((Instant::now(), apps.iter().map(|a| a.id.clone()).collect()));
        Ok(apps)
    }

    /// Launches `id` only if LG's own list names it: the last list if it is
    /// fresh and has it, else a new one.
    fn launch(&self, id: &str) -> Result<Value, Refusal> {
        let known = lock(&self.apps)
            .as_ref()
            .is_some_and(|(at, ids)| at.elapsed() < APPS_FRESH && ids.iter().any(|i| i == id));
        if !known && !self.list_apps()?.iter().any(|a| a.id == id) {
            return Err(Refusal::new(
                ErrorCode::NotFound,
                format!("no app or input {id} on this TV"),
            ));
        }
        self.done(&lg::launch(id))
    }

    fn settings_get(&self, keys: Option<&[&'static Setting]>) -> Result<Value, Refusal> {
        let keys: Vec<&'static str> = match keys {
            Some(list) => list.iter().map(|s| s.key).collect(),
            None => protocol::SETTINGS.iter().map(|s| s.key).collect(),
        };
        for (category, src) in [("picture", Source::Picture), ("sound", Source::Sound)] {
            let wanted = lg::LG_SETTINGS
                .iter()
                .any(|s| s.category == category && keys.contains(&s.setting.key));
            if wanted {
                let reply = bus::checked(&*self.bus, &lg::read_settings(category, false))
                    .map_err(failed)?;
                self.events.message(src, Ok(reply), false);
            }
        }
        Ok(self.events.settings(&keys))
    }

    /// LG's list of values for `s`, read once a session.
    fn desc(&self, s: &'static LgSetting) -> Option<Vec<String>> {
        if let Some(known) = lock(&self.desc).get(s.key) {
            return known.clone();
        }
        let listed = bus::checked(&*self.bus, &lg::read_desc(s))
            .ok()
            .and_then(|r| lg::desc_values(&r, s.key));
        lock(&self.desc).insert(s.key, listed.clone());
        listed
    }

    fn settings_set(&self, s: &'static LgSetting, value: SettingValue) -> Result<Value, Refusal> {
        let lg_value = match value {
            SettingValue::Int(n) => lg::int_value(n),
            SettingValue::Name(name) => {
                let range = if s.by_range {
                    let reply = bus::checked(&*self.bus, &lg::read_dimension()).map_err(failed)?;
                    lg::range(&reply).map_err(unavailable)?
                } else {
                    Range::Sdr
                };
                let listed = self.desc(s);
                json!(lg::pick(s, name, range, listed.as_deref()).map_err(unavailable)?)
            }
        };
        self.done(&lg::write_setting(s, lg_value))
    }
}

/// What a line of input was.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Text,
    TooLong,
    End,
}

/// Reads one line into `buf`, without its newline, never holding more than
/// `max` bytes of it.
pub fn next_line(r: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> io::Result<Line> {
    buf.clear();
    loop {
        let chunk = match r.fill_buf() {
            Ok(chunk) => chunk,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if chunk.is_empty() {
            return Ok(if buf.is_empty() {
                Line::End
            } else {
                Line::Text
            });
        }
        let (take, newline) = match chunk.iter().position(|&b| b == b'\n') {
            Some(at) => (at, true),
            None => (chunk.len(), false),
        };
        if buf.len() + take > max {
            return Ok(Line::TooLong);
        }
        buf.extend_from_slice(&chunk[..take]);
        r.consume(take + usize::from(newline));
        if newline {
            return Ok(Line::Text);
        }
    }
}

/// Answers one line: the reply, and the state to read again after it.
pub fn answer(ctl: &Ctl, log: &Mutex<Log>, line: &[u8]) -> (String, Option<Source>) {
    let started = Instant::now();
    let request = match protocol::parse_request(line) {
        Ok(request) => request,
        Err(refused) => {
            let id = refused
                .id
                .map_or_else(|| "-".to_owned(), |id| id.to_string());
            lock(log).line(&format!(
                "> {id} refused, {}: {}",
                refused.refusal.code.as_str(),
                refused.refusal.message
            ));
            return (protocol::reply_err(refused.id, &refused.refusal), None);
        }
    };
    let (id, verb) = (request.id, &request.verb);
    let args = verb.log_args();
    lock(log).line(&format!(
        "> {id} {}{}{args}",
        verb.name(),
        if args.is_empty() { "" } else { " " }
    ));
    let (result, refresh) = ctl.handle(verb);
    let mut reply = match &result {
        Ok(value) => protocol::reply_ok(id, value.clone()),
        Err(refusal) => protocol::reply_err(Some(id), refusal),
    };
    if reply.len() > MAX_LINE_BYTES {
        reply = protocol::reply_err(Some(id), &failed("the reply would be over 64 KiB"));
    }
    let ms = started.elapsed().as_millis();
    // LG's reason for a text verb could hold the text: only its code is kept.
    let typed = matches!(verb, Verb::TextInsert(_) | Verb::Toast(_));
    let outcome = match &result {
        Ok(_) => "ok".to_owned(),
        Err(r) if typed => r.code.as_str().to_owned(),
        Err(r) => format!("{}: {}", r.code.as_str(), r.message),
    };
    lock(log).line(&format!("< {id} {outcome} ({ms} ms)"));
    (reply, refresh)
}

/// Logs why the session ends, and ends it.
fn finish(log: &Mutex<Log>, why: &str, code: i32) -> ! {
    lock(log).line(&format!("end: {why}"));
    sys::os::exit_now(code)
}

/// Opens the subscriptions and waits for their first replies; every state
/// whose method did not take the subscription is polled, and read once now
/// if nothing came.
fn subscribe(bus: &dyn Bus, events: &Arc<Events>) {
    let subs: Vec<(&'static str, Value)> = Source::ALL
        .iter()
        .map(|s| {
            let c = s.call(true);
            (c.uri, c.payload)
        })
        .collect();
    let firsts: Arc<(Mutex<Vec<Option<Value>>>, Condvar)> =
        Arc::new((Mutex::new(vec![None; Source::ALL.len()]), Condvar::new()));
    let (into, to) = (Arc::clone(&firsts), Arc::clone(events));
    let sink: Sink = Arc::new(move |i, msg: Result<Value, String>| {
        let src = Source::ALL[i];
        {
            let mut seen = lock(&into.0);
            if seen[i].is_none() {
                seen[i] = Some(
                    msg.clone()
                        .unwrap_or_else(|e| json!({ "returnValue": false, "errorText": e })),
                );
                into.1.notify_all();
            }
        }
        to.message(src, msg, true);
    });
    match bus.watch(subs, sink) {
        None => Source::ALL.iter().for_each(|s| events.poll(*s)),
        Some(Err(e)) => {
            lock(&events.log).line(&format!("no subscriptions: {e}"));
            Source::ALL.iter().for_each(|s| events.poll(*s));
        }
        Some(Ok(())) => {
            let deadline = Instant::now() + FIRST_WAIT;
            let mut seen = lock(&firsts.0);
            while seen.iter().any(Option::is_none) {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                seen = firsts
                    .1
                    .wait_timeout(seen, left)
                    .unwrap_or_else(|p| p.into_inner())
                    .0;
            }
            for (src, first) in Source::ALL.iter().zip(seen.iter()) {
                if !first.as_ref().is_some_and(watch::subscribed) {
                    events.poll(*src);
                }
            }
        }
    }
    let first_seen: Vec<bool> = lock(&firsts.0).iter().map(Option::is_some).collect();
    for (src, seen) in Source::ALL.iter().zip(first_seen) {
        if events.polled(*src) && !seen {
            let c = src.call(false);
            events.message(*src, bus.call(c.uri, &c.payload), false);
        }
    }
}

/// The names in `hello`'s `polling` list, once each.
fn polling(events: &Events) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for src in Source::ALL {
        if events.polled(src) && !names.contains(&src.state()) {
            names.push(src.state());
        }
    }
    names
}

pub fn hello(ctl: &Ctl, events: &Events, bus: &str, keys: &str) -> String {
    protocol::event(
        "hello",
        json!({
            "protocol": PROTOCOL_VERSION,
            "server": concat!("lumen-ctl ", env!("CARGO_PKG_VERSION")),
            "verbs": ctl.verbs(),
            "polling": polling(events),
            "bus": bus,
            "keys": keys,
        }),
    )
}

/// Sends `alive`, and ends the session when a stop signal comes or the
/// client is gone.
fn ticker(out: Arc<Out>, log: Arc<Mutex<Log>>, alive: Duration) {
    let mut next = Instant::now() + alive;
    loop {
        thread::sleep(TICK);
        if let Some(signal) = sys::os::stop_signal() {
            finish(&log, &format!("stopped by {signal}"), 0);
        }
        if out.gone() {
            finish(&log, "stdout closed", 0);
        }
        if Instant::now() >= next {
            out.line(&protocol::event("alive", json!({})));
            next += alive;
        }
    }
}

/// Reads each polled state when it is due.
fn poller(bus: Arc<dyn Bus>, events: Arc<Events>) {
    let mut due = [Instant::now(); Source::ALL.len()];
    for src in Source::ALL {
        due[src.index()] += src.poll_every();
    }
    loop {
        thread::sleep(TICK * 2);
        for src in Source::ALL {
            let i = src.index();
            if !events.polled(src) || Instant::now() < due[i] {
                continue;
            }
            let c = src.call(false);
            events.message(src, bus.call(c.uri, &c.payload), false);
            due[i] = Instant::now() + src.poll_every();
        }
    }
}

/// The bus `serve` uses, and the fake TV behind it when it is one, for its
/// script.
type Opened = (Arc<dyn Bus>, Option<Arc<FakeBus>>);

fn open_bus(kind: BusKind) -> Result<Opened, String> {
    Ok(match kind {
        BusKind::Library => (Arc::new(sys::luna::LibraryBus::open()?), None),
        BusKind::Command => (Arc::new(CommandBus::new(Box::new(Spawn))), None),
        BusKind::Fake => {
            let fake = Arc::new(FakeBus::new(true));
            (Arc::clone(&fake) as Arc<dyn Bus>, Some(fake))
        }
    })
}

pub fn run(cfg: Serve) -> ! {
    let log = Arc::new(Mutex::new(Log::open(&cfg.log, log::MAX_BYTES)));
    if let Err(e) = sys::os::catch_stop_signals() {
        eprintln!("lumen-ctl: cannot catch SIGTERM: {e}");
        finish(&log, &format!("cannot catch SIGTERM: {e}"), 1);
    }
    let (bus, fake) = match open_bus(cfg.bus) {
        Ok(bus) => bus,
        Err(e) => {
            eprintln!("lumen-ctl: no {} bus: {e}", bus_name(cfg.bus));
            finish(&log, &format!("no {} bus: {e}", bus_name(cfg.bus)), 1);
        }
    };
    let keys: Box<dyn KeyRoute> = match cfg.keys {
        KeysKind::None => Box::new(NoKeys),
        KeysKind::Keymanager => match Template::load(&cfg.keys_file, Some(sys::os::euid())) {
            Ok(t) => Box::new(t),
            Err(e) => {
                lock(&log).line(&format!("keys: {e}; key is left out"));
                Box::new(NoKeys)
            }
        },
    };
    let out = Arc::new(Out::new(Box::new(io::stdout())));
    let events = Arc::new(Events::new(Arc::clone(&out), Arc::clone(&log)));
    let runner: Box<dyn Runner> = match cfg.bus {
        // The fake TV has no iptables or initctl to ask.
        BusKind::Fake => Box::new(NoPrograms),
        _ => Box::new(Spawn),
    };
    let ctl = Ctl::new(
        Arc::clone(&bus),
        keys,
        Arc::clone(&events),
        privacy::Paths::tv(),
        runner,
    );
    subscribe(&*bus, &events);
    let hello = hello(&ctl, &events, bus.name(), ctl.keys.name());
    lock(&log).line(&format!(
        "start: {} bus, keys {}, polling {:?}",
        bus.name(),
        ctl.keys.name(),
        polling(&events)
    ));
    events.start(&hello);

    let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
        thread::Builder::new()
            .name(name.to_owned())
            .stack_size(256 << 10)
            .spawn(f)
    };
    let (o, l) = (Arc::clone(&out), Arc::clone(&log));
    let alive = cfg.alive;
    if let Err(e) = spawn("ticker", Box::new(move || ticker(o, l, alive))) {
        finish(&log, &format!("no ticker thread: {e}"), 1);
    }
    let (b, ev) = (Arc::clone(&bus), Arc::clone(&events));
    if let Err(e) = spawn("poller", Box::new(move || poller(b, ev))) {
        finish(&log, &format!("no poller thread: {e}"), 1);
    }
    if let Some(fake) = &fake {
        fake::start_script(fake);
    }

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut buf = Vec::with_capacity(4096);
    loop {
        match next_line(&mut input, &mut buf, MAX_LINE_BYTES) {
            Ok(Line::End) => finish(&log, "stdin closed", 0),
            Ok(Line::TooLong) => {
                out.line(&protocol::reply_err(
                    None,
                    &Refusal::new(
                        ErrorCode::BadLine,
                        format!("line is longer than {MAX_LINE_BYTES} bytes"),
                    ),
                ));
                finish(&log, "a line over 64 KiB", 0);
            }
            Ok(Line::Text) => {
                if buf.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let (reply, refresh) = answer(&ctl, &log, &buf);
                out.line(&reply);
                if let Some(src) = refresh {
                    ctl.refresh(src);
                }
            }
            Err(e) => finish(&log, &format!("stdin: {e}"), 1),
        }
    }
}

fn bus_name(kind: BusKind) -> &'static str {
    match kind {
        BusKind::Library => "library",
        BusKind::Command => "command",
        BusKind::Fake => "fake",
    }
}

/// `privacy.state` on the fake TV: no program to start.
struct NoPrograms;

impl Runner for NoPrograms {
    fn run(&self, program: &'static str, _: &[&str]) -> Result<bus::Output, String> {
        Err(format!("{program} is not run on the fake TV"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            lock(&self.0).extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(lock(&self.0).clone())
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        }
        fn clear(&self) {
            lock(&self.0).clear();
        }
    }

    struct Tv {
        ctl: Ctl,
        fake: Arc<FakeBus>,
        events: Arc<Events>,
        out: Shared,
        log: Arc<Mutex<Log>>,
        dir: std::path::PathBuf,
    }

    impl Drop for Tv {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn tv(name: &str, keys: Box<dyn KeyRoute>) -> Tv {
        let dir =
            std::env::temp_dir().join(format!("lumen-ctl-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = Shared::default();
        let log = Arc::new(Mutex::new(Log::open(
            &dir.join("lumen-ctl.log"),
            log::MAX_BYTES,
        )));
        let events = Arc::new(Events::new(
            Arc::new(Out::new(Box::new(out.clone()))),
            Arc::clone(&log),
        ));
        let fake = Arc::new(FakeBus::new(false));
        let paths = privacy::Paths {
            hook: dir.join("hook"),
            hook_log: dir.join("hook.log"),
            guard_status: dir.join("status"),
            owner: None,
        };
        let ctl = Ctl::new(
            Arc::clone(&fake) as Arc<dyn Bus>,
            keys,
            Arc::clone(&events),
            paths,
            Box::new(NoPrograms),
        );
        Tv {
            ctl,
            fake,
            events,
            out,
            log,
            dir,
        }
    }

    impl Tv {
        fn ask(&self, line: &str) -> Value {
            let (reply, refresh) = answer(&self.ctl, &self.log, line.as_bytes());
            if let Some(src) = refresh {
                self.ctl.refresh(src);
            }
            serde_json::from_str(&reply).unwrap()
        }
        fn log_text(&self) -> String {
            std::fs::read_to_string(self.dir.join("lumen-ctl.log")).unwrap_or_default()
        }
    }

    #[test]
    fn says_hello_with_the_verbs_and_then_the_state() {
        let t = tv("hello", Box::new(NoKeys));
        subscribe(&*t.fake, &t.events);
        let hello = hello(&t.ctl, &t.events, "fake", "none");
        t.events.start(&hello);
        let lines = t.out.lines();
        let names: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "hello",
                "power",
                "volume",
                "foreground",
                "inputs",
                "settings"
            ]
        );
        let data = &lines[0]["data"];
        assert_eq!(data["protocol"], json!(1));
        assert!(data["server"].as_str().unwrap().starts_with("lumen-ctl "));
        // The fake's power method ignores `subscribe`, so it is polled.
        assert_eq!(data["polling"], json!(["power"]));
        let verbs: Vec<&str> = data["verbs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(verbs.len(), VERBS.len() - 2);
        assert!(!verbs.contains(&"key") && !verbs.contains(&"ambient.start"));
        assert_eq!(
            lines[2],
            json!({ "event": "volume", "data": { "level": 14, "muted": false } })
        );
        assert_eq!(
            lines[5]["data"]["values"],
            json!({ "picture.mode": "cinema", "picture.oledLight": 80, "picture.energySaving": "off", "sound.mode": "standard" })
        );
        // Once started, a change made at the TV is pushed.
        t.out.clear();
        t.fake.press_volume_up();
        assert_eq!(
            t.out.lines(),
            [json!({ "event": "volume", "data": { "level": 15, "muted": false } })]
        );
    }

    #[test]
    fn answers_every_verb_through_the_fake() {
        let keys = Template::parse(
            br#"{"service":"keymanager","press":[{"keyCode":"$code"}],"codes":{"ok":13}}"#,
        )
        .unwrap();
        let t = tv("verbs", Box::new(keys));
        assert!(t.ctl.verbs().contains(&"key"));
        let ok = |args: &str, verb: &str| {
            let line = if args.is_empty() {
                format!(r#"{{"id":1,"verb":"{verb}"}}"#)
            } else {
                format!(r#"{{"id":1,"verb":"{verb}","args":{args}}}"#)
            };
            let r = t.ask(&line);
            assert_eq!(r["ok"], json!(true), "{verb}: {r}");
            r["result"].clone()
        };
        for verb in [
            "power.off",
            "screen.off",
            "screen.on",
            "volume.up",
            "volume.down",
            "text.enter",
            "lumen.open",
        ] {
            assert_eq!(ok("", verb), json!({}));
        }
        ok(r#"{"level":30}"#, "volume.set");
        ok(r#"{"muted":true}"#, "mute.set");
        let apps = ok("", "apps.list");
        let ids: Vec<&str> = apps["apps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"netflix") && ids.contains(&"com.webos.app.hdmi3"));
        assert!(!ids.contains(&"bookmark.1"));
        ok(r#"{"id":"com.webos.app.hdmi3"}"#, "launch");
        ok(r#"{"name":"ok"}"#, "key");
        assert_eq!(t.fake.keys_pressed(), [json!(13)]);
        ok(r#"{"text":"héllo"}"#, "text.insert");
        ok(r#"{"count":2}"#, "text.delete");
        assert_eq!(t.fake.typed(), "hél");
        ok(r#"{"text":"Volume 20"}"#, "toast");
        assert_eq!(t.fake.toasts(), 1);
        let privacy = ok("", "privacy.state");
        assert_eq!(privacy["guard"], Value::Null);
        assert_eq!(privacy["jobs"].as_array().unwrap().len(), 4);
        assert_eq!(
            ok(r#"{"keys":["picture.mode","sound.mode"]}"#, "settings.get"),
            json!({ "values": { "picture.mode": "cinema", "sound.mode": "standard" } })
        );
        ok(r#"{"key":"picture.oledLight","value":55}"#, "settings.set");
        ok(r#"{"key":"sound.mode","value":"cinema"}"#, "settings.set");
        assert_eq!(
            ok("", "settings.get")["values"],
            json!({ "picture.mode": "cinema", "picture.oledLight": 55, "picture.energySaving": "off", "sound.mode": "cinema" })
        );
    }

    #[test]
    fn refuses_what_the_tv_cannot_do() {
        let t = tv("refuse", Box::new(NoKeys));
        let code = |line: &str| t.ask(line)["error"]["code"].as_str().unwrap().to_owned();
        assert_eq!(
            code(r#"{"id":1,"verb":"key","args":{"name":"ok"}}"#),
            "unavailable"
        );
        assert_eq!(code(r#"{"id":2,"verb":"ambient.start"}"#), "unavailable");
        assert_eq!(
            code(r#"{"id":3,"verb":"launch","args":{"id":"com.example.notinstalled"}}"#),
            "not-found"
        );
        // Bookmarks are not apps.
        assert_eq!(
            code(r#"{"id":4,"verb":"launch","args":{"id":"bookmark.1"}}"#),
            "not-found"
        );
        assert_eq!(code(r#"{"id":5,"verb":"reboot"}"#), "unknown-verb");
        assert_eq!(
            code(r#"{"id":6,"verb":"volume.set","args":{"level":101}}"#),
            "bad-args"
        );
        // No Cinema Home in SDR; energy saving's minimum is not confirmed.
        assert_eq!(
            code(
                r#"{"id":7,"verb":"settings.set","args":{"key":"picture.mode","value":"cinema-home"}}"#
            ),
            "unavailable"
        );
        assert_eq!(
            code(
                r#"{"id":8,"verb":"settings.set","args":{"key":"picture.energySaving","value":"minimum"}}"#
            ),
            "unavailable"
        );
        let r = t.ask("not json");
        assert_eq!(
            r,
            json!({ "id": null, "ok": false, "error": { "code": "bad-line", "message": "not JSON" } })
        );
    }

    #[test]
    fn writes_lgs_names_for_lumens_settings() {
        let t = tv("settings", Box::new(NoKeys));
        subscribe(&*t.fake, &t.events);
        t.events.start("{}");
        t.out.clear();
        // Standard is LG's normal; Filmmaker is on the fake TV's own list.
        for (value, lg_name) in [
            ("standard", "normal"),
            ("filmmaker", "filmMaker"),
            ("expert-dark", "expert2"),
        ] {
            let r = t.ask(&format!(
                r#"{{"id":1,"verb":"settings.set","args":{{"key":"picture.mode","value":"{value}"}}}}"#
            ));
            assert_eq!(r["ok"], json!(true), "{value}: {r}");
            let read = t
                .fake
                .call(
                    lg::GET_SYSTEM_SETTINGS,
                    &json!({ "category": "picture", "keys": ["pictureMode"] }),
                )
                .unwrap();
            assert_eq!(read["settings"]["pictureMode"], json!(lg_name));
        }
        // Each change was pushed as Lumen's name, once.
        let pushed: Vec<Value> = t
            .out
            .lines()
            .into_iter()
            .filter(|l| l["event"] == json!("settings"))
            .map(|l| l["data"]["values"]["picture.mode"].clone())
            .collect();
        assert_eq!(
            pushed,
            [json!("standard"), json!("filmmaker"), json!("expert-dark")]
        );
    }

    #[test]
    fn logs_each_request_and_reply_but_never_typed_text() {
        let t = tv("log", Box::new(NoKeys));
        t.ask(r#"{"id":7,"verb":"text.insert","args":{"text":"hunter2"}}"#);
        t.ask(r#"{"id":8,"verb":"toast","args":{"text":"secret toast"}}"#);
        t.ask(r#"{"id":9,"verb":"volume.set","args":{"level":20}}"#);
        t.ask(r#"{"id":10,"verb":"text.insert","args":{"text":"bad\u0001"}}"#);
        t.ask("{\"id\":11,\"verb\":\"text.insert\",\"args\":{\"text\":\"hunter3\"},\"x\":1}");
        let log = t.log_text();
        assert!(!log.contains("hunter") && !log.contains("secret"), "{log}");
        assert!(log.contains("> 7 text.insert (text withheld)"), "{log}");
        assert!(log.contains("< 7 ok ("), "{log}");
        assert!(log.contains("> 9 volume.set {\"level\":20}"), "{log}");
        assert!(
            log.contains("> 10 refused, bad-args: text must not contain control characters"),
            "{log}"
        );
        assert!(
            log.contains("> 11 refused, bad-line: unexpected field \"x\""),
            "{log}"
        );
        assert_eq!(log.lines().count(), 8);
    }

    #[test]
    fn a_failed_subscription_turns_to_polling() {
        let t = tv("poll", Box::new(NoKeys));
        t.events
            .message(Source::Volume, Err("Service is not running".into()), true);
        assert!(t.events.polled(Source::Volume));
        // Logged once while it keeps failing.
        t.events
            .message(Source::Volume, Err("still down".into()), false);
        t.events
            .message(Source::Volume, Err("still down".into()), false);
        let log = t.log_text();
        assert!(log.contains("volume getVolume: the subscription failed (Service is not running)"));
        assert_eq!(log.matches("still down").count(), 1);
    }

    #[test]
    fn reads_lines_without_holding_more_than_the_limit() {
        let mut buf = Vec::new();
        let mut r = Cursor::new(b"one\ntwo\r\n\nlast".to_vec());
        for want in ["one", "two\r", "", "last"] {
            assert_eq!(next_line(&mut r, &mut buf, 8).unwrap(), Line::Text);
            assert_eq!(buf, want.as_bytes());
        }
        assert_eq!(next_line(&mut r, &mut buf, 8).unwrap(), Line::End);
        // At the limit is a line; one byte over is not, newline or not.
        let mut r = Cursor::new(b"12345678\n123456789\n".to_vec());
        assert_eq!(next_line(&mut r, &mut buf, 8).unwrap(), Line::Text);
        assert_eq!(next_line(&mut r, &mut buf, 8).unwrap(), Line::TooLong);
        let mut r = Cursor::new(vec![b'x'; 100]);
        assert_eq!(next_line(&mut r, &mut buf, 8).unwrap(), Line::TooLong);
        assert!(buf.len() <= 8);
    }

    #[test]
    fn a_reply_over_the_limit_is_refused_whole() {
        struct Big;
        impl Bus for Big {
            fn name(&self) -> &'static str {
                "big"
            }
            fn call(&self, uri: &str, _: &Value) -> Result<Value, String> {
                let points: Vec<Value> = (0..2000)
                    .map(|i| json!({ "id": format!("app{i}"), "title": "x".repeat(60) }))
                    .collect();
                Ok(if uri == lg::LIST_LAUNCH_POINTS {
                    json!({ "returnValue": true, "launchPoints": points })
                } else {
                    json!({ "returnValue": false })
                })
            }
        }
        let t = tv("big", Box::new(NoKeys));
        let ctl = Ctl::new(
            Arc::new(Big),
            Box::new(NoKeys),
            Arc::clone(&t.events),
            privacy::Paths::tv(),
            Box::new(NoPrograms),
        );
        let (reply, _) = answer(&ctl, &t.log, br#"{"id":1,"verb":"apps.list"}"#);
        assert_eq!(
            serde_json::from_str::<Value>(&reply).unwrap()["error"]["message"],
            json!("the reply would be over 64 KiB")
        );
    }
}
