//! `lumend run`: the daemon. One thread decides; the others only wait on
//! something and tell it, through one channel:
//!
//! * `signals`: SIGTERM, SIGINT and SIGHUP, through `sigwait`;
//! * `inotify`: the kill switch appearing, and mode changes in Lumen's
//!   folders;
//! * `luna`: the bus thread's subscription messages (SAM's life events, the
//!   hub's word on the held-down senders);
//! * `link-accept` and `link-read`: the shell's connection and requests;
//! * `icons`: one icon copy at a time.
//!
//! Each part starts only if lumend.conf switches it on. A part that fails
//! says so in the log and the status file and stops; the others go on.
//!
//! The kill switch (`/var/luna/preferences/lumen_disable`) is watched: when
//! it appears lumend takes its stand-ins away and its socket out of the jail,
//! and exits within a second. What S1 stopped stays stopped until the
//! reboot, which then starts the TV without Lumen (docs/safety.md, rule 5).
//! A plain SIGTERM ends lumend and changes nothing.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumend_proto::{
    Icons, IconsFault, Part, Parts, Refusal, Reply, Request, ScreenOff, State, VERSION,
};
use serde_json::Value;

use crate::bus::{self, Bus};
use crate::conf::Conf;
use crate::fsat::Dir;
use crate::keeper::{Ask, Keeper};
use crate::lg_home::{self, LgHome, Next};
use crate::link::{self, Conn, JailSocket, LinkEvent};
use crate::paths::{
    APP_ID, BUS_NAME, DIR, KILL_DIR, KILL_NAME, LG_HOME, LOCK_NAME, Root, STATUS_NAME,
};
use crate::run::{Runner, Spawn};
use crate::senders::{self, SENDERS, System, Tv};
use crate::sys::inotify::{self, Inotify};
use crate::{icons, modes, s1, say};

/// Subscription ids: SAM's life events, then one per sender.
const SUB_LIFE: u32 = 1;
const SUB_SENDER: u32 = 100;
/// The quiet after the last mode change before the walk.
const MODES_QUIET: Duration = Duration::from_secs(3);
/// The longest a stream of changes delays the walk.
const MODES_MAX_DELAY: Duration = Duration::from_secs(30);
const BUS_RETRY: Duration = Duration::from_secs(5);
const BUS_TRIES: u32 = 24;
const RESUBSCRIBE: Duration = Duration::from_secs(30);
/// Requests the shell may make in ten seconds.
const MAX_REQUESTS: usize = 20;
const SCREEN_OFF_GAP: Duration = Duration::from_secs(60);
/// How many times a sender's stand-in is mounted again in one run.
const MAX_REMOUNTS: u32 = 3;
const MODE_MASK: u32 = inotify::IN_ATTRIB
    | inotify::IN_CREATE
    | inotify::IN_MOVED_TO
    | inotify::IN_MOVED_FROM
    | inotify::IN_DELETE
    | inotify::IN_ONLYDIR;

/// What the threads tell the deciding one.
#[derive(Debug)]
pub enum Event {
    Stop(&'static str),
    KillSwitch,
    ModesDirty,
    Luna(u32, Result<Value, String>),
    Link(LinkEvent),
    IconsDone(Result<Icons, (IconsFault, String)>),
}

impl From<LinkEvent> for Event {
    fn from(e: LinkEvent) -> Self {
        Self::Link(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Timer {
    S1Round,
    LgLook,
    LgVerify(Option<String>),
    LgFallback(u32),
    ShellBoot,
    ShellRestart,
    ModesWalk,
    SenderKill(usize),
    BusRetry,
    Resubscribe(u32),
}

/// The link's state on the deciding thread.
struct Link {
    socket: JailSocket,
    stop: Arc<AtomicBool>,
    hello: Option<u64>,
    requests: Vec<Instant>,
}

pub struct Daemon {
    root: Root,
    conf: Conf,
    parts: Parts,
    tx: Sender<Event>,
    bus: Option<Arc<dyn Bus>>,
    bus_tries: u32,
    runner: Arc<dyn Runner>,
    sys: Arc<dyn System>,
    timers: BTreeMap<(Instant, u64), Timer>,
    seq: u64,
    started: Instant,
    lg: LgHome,
    keeper: Keeper,
    inotify: Option<Arc<Inotify>>,
    kill_wd: Arc<AtomicI32>,
    mode_wds: Vec<i32>,
    modes_first_dirty: Option<Instant>,
    modes_walked: bool,
    link: Option<Link>,
    conn: Arc<Conn>,
    icons_busy: bool,
    icons_reply_to: Option<u64>,
    icons_after_modes: bool,
    screen_last: Option<Instant>,
    remounts: u32,
    last: BTreeMap<&'static str, String>,
    _lock: File,
}

/// The lock file (`lumend.pid` in root's folder), opened, not locked.
fn lock_file(root: &Root) -> Result<File, String> {
    let me = crate::sys::os::euid();
    let dir = Dir::open_path(&root.path(DIR)).map_err(|e| format!("{DIR}: {e}"))?;
    crate::fsat::check_private(&dir.meta().map_err(|e| e.to_string())?, me, DIR)?;
    let name = OsStr::new(LOCK_NAME);
    match dir.stat_opt(name).map_err(|e| e.to_string())? {
        None => dir.create(name, 0o600).map_err(|e| e.to_string()),
        Some(m) if m.is_file() && m.uid == me && m.nlink == 1 => {
            dir.open_rw(name).map_err(|e| e.to_string())
        }
        Some(_) => Err(format!("{LOCK_NAME} is not a plain file of root's")),
    }
}

/// Takes the daemon's lock: `None` while a daemon (or a `lumend try`) holds
/// it. The lock is the file's `flock`, so it is gone with its holder,
/// whatever ended it. `pid` writes this process's id in the file.
pub fn lock(root: &Root, pid: bool) -> Result<Option<File>, String> {
    let file = lock_file(root)?;
    match file.try_lock() {
        Ok(()) => {
            if pid {
                file.set_len(0).map_err(|e| e.to_string())?;
                (&file)
                    .write_all(format!("{}\n", std::process::id()).as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(Some(file))
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.to_string()),
    }
}

/// The running daemon's pid: `None` when the lock is free. The pid is the
/// file's, checked against `/proc` to be lumend's own binary.
pub fn running(root: &Root) -> Result<Option<u32>, String> {
    let mut file = lock_file(root)?;
    match file.try_lock() {
        Ok(()) => return Ok(None),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(std::fs::TryLockError::Error(e)) => return Err(e.to_string()),
    }
    let (bytes, _) = crate::fsat::read_capped(&mut file, 32).map_err(|e| e.to_string())?;
    let pid: u32 = String::from_utf8_lossy(&bytes)
        .trim()
        .parse()
        .map_err(|_| format!("{LOCK_NAME} holds no pid"))?;
    let exe =
        std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|e| format!("pid {pid}: {e}"))?;
    let exe = exe.to_string_lossy();
    if root.is_tv() && exe.trim_end_matches(" (deleted)") != crate::paths::BIN {
        return Err(format!("pid {pid} in {LOCK_NAME} is not lumend ({exe})"));
    }
    Ok(Some(pid))
}

/// How long a tool's hold on the shell still lasts (`HOLD_NAME` in root's
/// folder, a plain file of root's alone); nothing when there is none, it
/// is past, or it can't be trusted.
pub fn shell_hold(root: &Root) -> Option<Duration> {
    use std::ffi::OsStr;
    let me = crate::sys::os::euid();
    let dir = Dir::open_path(&root.path(DIR)).ok()?;
    let name = OsStr::new(crate::paths::HOLD_NAME);
    let meta = dir.stat_opt(name).ok()??;
    if !meta.is_file() || meta.uid != me || meta.shared_write() {
        return None;
    }
    let mut file = dir.open_read(name).ok()?;
    let (bytes, _) = crate::fsat::read_capped(&mut file, 32).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    crate::keeper::hold_left(&String::from_utf8_lossy(&bytes), now)
}

/// The kill switch's state, by `lstat` (a link there counts).
pub fn kill_switch(root: &Root) -> bool {
    std::fs::symlink_metadata(root.path(KILL_DIR).join(KILL_NAME)).is_ok()
}

impl Daemon {
    fn new(root: Root, conf: Conf, tx: Sender<Event>, lock: File) -> Self {
        Self {
            root,
            conf,
            parts: Parts::default(),
            tx,
            bus: None,
            bus_tries: 0,
            runner: Arc::new(Spawn),
            sys: Arc::new(Tv),
            timers: BTreeMap::new(),
            seq: 0,
            started: Instant::now(),
            lg: LgHome::default(),
            keeper: Keeper::default(),
            inotify: None,
            kill_wd: Arc::new(AtomicI32::new(-1)),
            mode_wds: Vec::new(),
            modes_first_dirty: None,
            modes_walked: false,
            link: None,
            conn: Arc::new(Conn::default()),
            icons_busy: false,
            icons_reply_to: None,
            icons_after_modes: false,
            screen_last: None,
            remounts: 0,
            last: BTreeMap::new(),
            _lock: lock,
        }
    }

    fn at(&mut self, when: Instant, timer: Timer) {
        self.seq += 1;
        self.timers.insert((when, self.seq), timer);
    }

    /// `timer` at `when`, unless the same one is due sooner.
    fn at_most(&mut self, when: Instant, timer: Timer) {
        let sooner = self
            .timers
            .iter()
            .any(|((t, _), x)| *x == timer && *t <= when);
        if !sooner {
            self.timers.retain(|_, x| *x != timer);
            self.at(when, timer);
        }
    }

    fn on(&self, part: Part) -> bool {
        self.conf.on(part) && self.parts.get(part) != State::Failed
    }

    /// One line in the log, kept as the part's last word in the status.
    fn note(&mut self, key: &'static str, line: String) {
        say!("{line}");
        self.last.insert(key, line);
        self.write_status();
    }

    fn fail(&mut self, part: Part, why: String) {
        self.parts.set(part, State::Failed);
        self.note(
            part.name(),
            format!("{}: FAILED, stopped: {why}", part.name()),
        );
    }

    /// `status` in root's folder, rewritten whole: for `lumend status`,
    /// `tools/lumend.sh status` and, later, the Privacy panel.
    fn write_status(&self) {
        let mut text = format!(
            "pid {}\nversion {}\nup {}s\n",
            std::process::id(),
            env!("CARGO_PKG_VERSION"),
            self.started.elapsed().as_secs()
        );
        text.push_str(&Reply::Status(self.parts).line());
        for (key, line) in &self.last {
            text.push_str(&format!("last {key} {}\n", crate::log::clean(line)));
        }
        let write = || -> std::io::Result<()> {
            let dir = Dir::open_path(&self.root.path(DIR))?;
            let tmp = OsStr::new("status.new");
            let _ = dir.unlink(tmp);
            let mut f = dir.create(tmp, 0o600)?;
            f.write_all(text.as_bytes())?;
            dir.rename(tmp, OsStr::new(STATUS_NAME))
        };
        if let Err(e) = write() {
            say!("status: not written: {e}");
        }
    }

    /// Everything that needs no bus, then the bus.
    fn start(&mut self) {
        for part in Part::ALL {
            if self.conf.on(part) {
                self.parts.set(part, State::On);
            }
        }
        if self.on(Part::S1) {
            match s1::add_rule(self.runner.as_ref()) {
                Ok(line) => self.note("s1", format!("s1: {line}")),
                Err(e) => self.note("s1", format!("s1: {e}")),
            }
            self.at(Instant::now(), Timer::S1Round);
        }
        if self.on(Part::Senders) {
            if self.conf.senders.is_empty() {
                self.note(
                    "senders",
                    "senders: on, but no sender is switched on".to_owned(),
                );
            }
            for (i, sender) in SENDERS.iter().enumerate() {
                if !self.conf.senders.contains(sender.name) {
                    continue;
                }
                match senders::hold(sender, self.sys.as_ref()) {
                    Ok(lines) => {
                        for line in lines {
                            self.note("senders", line);
                        }
                        self.at(
                            Instant::now() + Duration::from_secs(2),
                            Timer::SenderKill(i),
                        );
                    }
                    Err(e) => self.note("senders", format!("senders: {e}")),
                }
            }
        }
        if self.on(Part::Modes) {
            self.at(Instant::now(), Timer::ModesWalk);
        }
        if self.on(Part::Link) {
            self.start_link();
        }
        if self.needs_bus() {
            self.at(Instant::now(), Timer::BusRetry);
        }
        self.write_status();
    }

    fn needs_bus(&self) -> bool {
        [Part::LgHome, Part::Icons, Part::Shell, Part::ScreenOff]
            .iter()
            .any(|p| self.on(*p))
            || (self.on(Part::Senders) && !self.conf.senders.is_empty())
    }

    fn start_link(&mut self) {
        if let Some(old) = self.link.take() {
            old.stop.store(true, Ordering::SeqCst);
            crate::sys::os::shutdown_listener(&old.socket.listener);
        }
        match link::make(&self.root) {
            Ok(socket) => {
                let stop = Arc::new(AtomicBool::new(false));
                let (listener, conn, tx, s) = (
                    Arc::clone(&socket.listener),
                    Arc::clone(&self.conn),
                    self.tx.clone(),
                    Arc::clone(&stop),
                );
                let uid = socket.shell_uid;
                let spawned = std::thread::Builder::new()
                    .name("link-accept".to_owned())
                    .stack_size(64 << 10)
                    .spawn(move || link::serve(listener, uid, conn, tx, s));
                if let Err(e) = spawned {
                    self.fail(Part::Link, format!("no thread: {e}"));
                    return;
                }
                self.note(
                    "link",
                    format!("link: listening in Lumen's jail for uid {uid}"),
                );
                self.link = Some(Link {
                    socket,
                    stop,
                    hello: None,
                    requests: Vec::new(),
                });
            }
            Err(e) => self.fail(Part::Link, e),
        }
    }

    fn start_bus(&mut self) {
        self.bus_tries += 1;
        let tx = Mutex::new(self.tx.clone());
        let sink: crate::sys::luna::Sink = Arc::new(move |id, msg| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(Event::Luna(id, msg));
            }
        });
        match crate::sys::luna::Luna::start(BUS_NAME, sink) {
            Ok(luna) => {
                say!("bus: registered as {BUS_NAME}");
                self.bus = Some(Arc::new(luna));
                self.bus_up();
            }
            Err(e) if self.bus_tries < BUS_TRIES => {
                if self.bus_tries == 1 {
                    say!("bus: not yet ({e}); trying every {}s", BUS_RETRY.as_secs());
                }
                self.at(Instant::now() + BUS_RETRY, Timer::BusRetry);
            }
            Err(e) => {
                for part in [Part::LgHome, Part::Icons, Part::Shell, Part::ScreenOff] {
                    if self.on(part) {
                        self.fail(part, format!("no bus after {} tries: {e}", self.bus_tries));
                    }
                }
            }
        }
    }

    fn subscribe(&mut self, id: u32) {
        let Some(bus) = self.bus.clone() else {
            return;
        };
        let (uri, payload) = if id == SUB_LIFE {
            bus::subscribe_life_events()
        } else {
            let Some(sender) = SENDERS.get((id - SUB_SENDER) as usize) else {
                return;
            };
            bus::server_status(sender.service)
        };
        if let Err(e) = bus.subscribe(id, uri, &payload) {
            say!("bus: FAILED to subscribe to {uri}: {e}");
        }
    }

    fn bus_up(&mut self) {
        if self.on(Part::LgHome) || self.on(Part::Shell) || self.on(Part::Link) {
            self.subscribe(SUB_LIFE);
        }
        if self.on(Part::Senders) {
            for (i, sender) in SENDERS.iter().enumerate() {
                if self.conf.senders.contains(sender.name) {
                    self.subscribe(SUB_SENDER + i as u32);
                }
            }
        }
        let now = Instant::now();
        if self.on(Part::LgHome) {
            self.at(now, Timer::LgLook);
        }
        if self.on(Part::Icons) {
            if self.on(Part::Modes) && !self.modes_walked {
                self.icons_after_modes = true;
            } else {
                self.start_icons(None);
            }
        }
        if self.on(Part::Shell) {
            self.at(
                (self.started + crate::keeper::BOOT_WAIT).max(now),
                Timer::ShellBoot,
            );
        }
    }

    fn start_icons(&mut self, reply_to: Option<u64>) {
        if reply_to.is_some() {
            self.icons_reply_to = reply_to;
        }
        if self.icons_busy {
            return;
        }
        let Some(bus) = self.bus.clone() else {
            self.finish_icons(Ok(Icons::Busy));
            return;
        };
        self.icons_busy = true;
        let (root, tx) = (self.root.clone(), self.tx.clone());
        let spawned = std::thread::Builder::new()
            .name("icons".to_owned())
            .stack_size(256 << 10)
            .spawn(move || {
                let result = icons::copy(&root, bus.as_ref());
                let _ = tx.send(Event::IconsDone(result));
            });
        if spawned.is_err() {
            self.icons_busy = false;
            self.finish_icons(Err((IconsFault::Io, "no thread".to_owned())));
        }
    }

    fn finish_icons(&mut self, result: Result<Icons, (IconsFault, String)>) {
        self.icons_busy = false;
        self.note("icons", icons::describe(&result));
        let answer = match result {
            Ok(icons) => icons,
            Err((fault, _)) => Icons::Failed(fault),
        };
        if let Some(id) = self.icons_reply_to.take() {
            self.conn.send(id, &Reply::Icons(answer));
        }
    }

    fn walk_modes(&mut self) {
        self.modes_first_dirty = None;
        let first = !self.modes_walked;
        self.modes_walked = true;
        let mut seen = Vec::new();
        let ino = self.inotify.clone();
        let result = modes::fix(&self.root, &mut |dir: &Dir| {
            if let Some(ino) = &ino
                && let Ok(wd) = ino.add(&dir.proc_path(), MODE_MASK)
            {
                seen.push(wd);
            }
        });
        if let Some(ino) = &self.inotify {
            for wd in self.mode_wds.iter().filter(|w| !seen.contains(w)) {
                ino.remove(*wd);
            }
        }
        self.mode_wds = seen;
        match result {
            Ok(report) => {
                if first || report.fixed > 0 || report.suspicious() {
                    self.note("modes", report.describe());
                }
                // A copy that refused for the folders' modes can go now.
                if self.icons_after_modes
                    || (report.fixed > 0 && self.on(Part::Icons) && self.icons_failed_folders())
                {
                    self.icons_after_modes = false;
                    if self.bus.is_some() {
                        self.start_icons(None);
                    }
                }
            }
            Err(e) => self.note("modes", format!("modes: {e}")),
        }
    }

    fn icons_failed_folders(&self) -> bool {
        self.last
            .get("icons")
            .is_some_and(|l| l.contains("FAILED (Folders)"))
    }

    fn on_life(&mut self, msg: &Value) {
        if msg.get("returnValue") == Some(&Value::Bool(false)) {
            let why = bus::refusal(msg);
            say!("bus: SAM's life events refused or ended: {why}");
            if self.on(Part::LgHome) {
                // As lumen-home did: five looks a minute apart.
                self.at(Instant::now(), Timer::LgFallback(0));
            }
            self.at(Instant::now() + RESUBSCRIBE, Timer::Resubscribe(SUB_LIFE));
            return;
        }
        let Some(event) = bus::life_event(msg) else {
            return;
        };
        let now = Instant::now();
        if event.app == APP_ID || event.app == LG_HOME {
            say!(
                "sam: {} {}{}",
                event.app,
                event.event,
                if event.home_key { " (homeKey)" } else { "" }
            );
        }
        if self.on(Part::LgHome) && lg_home::wants_look(&event) {
            self.at_most(now + lg_home::GRACE, Timer::LgLook);
        }
        let ask = self.keeper.on_event(&event, now);
        match ask {
            Ask::BootLaunch if self.on(Part::Shell) => {
                self.timers.retain(|_, t| *t != Timer::ShellBoot);
                self.at(now, Timer::ShellBoot);
            }
            Ask::RestartAt(when) if self.on(Part::Shell) => {
                self.note(
                    "shell",
                    "shell: closed; starting it again in 5 s".to_owned(),
                );
                // One restart, however many events one close makes (SAM
                // says both `stop` and `close`).
                self.timers.retain(|_, t| *t != Timer::ShellRestart);
                self.at(when, Timer::ShellRestart);
            }
            Ask::Relaunched { home_key } => {
                self.conn.send_current(&Reply::Relaunch { home_key });
            }
            _ => {}
        }
        if event.app == APP_ID
            && event.event == "launch"
            && let Some(link) = &self.link
            && !link.socket.still_there(&self.root)
        {
            say!("link: the jail changed; making the socket again");
            self.start_link();
        }
    }

    fn on_sender(&mut self, i: usize, msg: &Value) {
        let Some(sender) = SENDERS.get(i) else {
            return;
        };
        if msg.get("returnValue") == Some(&Value::Bool(false)) {
            say!(
                "senders: the hub refused to report {}: {}",
                sender.service,
                bus::refusal(msg)
            );
            return;
        }
        if msg.get("connected") != Some(&Value::Bool(true)) {
            return;
        }
        say!("senders: {} connected", sender.service);
        if !senders::held(sender, self.sys.as_ref()) {
            if self.remounts >= MAX_REMOUNTS {
                self.note(
                    "senders",
                    format!(
                        "senders: {}: the stand-in's mount went again; left alone",
                        sender.name
                    ),
                );
                return;
            }
            self.remounts += 1;
            self.note(
                "senders",
                format!(
                    "senders: {}: the stand-in's mount was gone; mounting it again",
                    sender.name
                ),
            );
            match senders::hold(sender, self.sys.as_ref()) {
                Ok(lines) => lines.into_iter().for_each(|l| self.note("senders", l)),
                Err(e) => self.note("senders", format!("senders: {e}")),
            }
        } else if let Ok(lines) = senders::stop_originals(sender, self.sys.as_ref(), false) {
            lines.into_iter().for_each(|l| self.note("senders", l));
        }
        self.at(
            Instant::now() + Duration::from_secs(2),
            Timer::SenderKill(i),
        );
    }

    fn on_link(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::Up(id) => {
                say!("link: the shell connected");
                if let Some(link) = &mut self.link {
                    link.hello = None;
                    link.requests.clear();
                }
                let _ = id;
            }
            LinkEvent::Down(_, why) => say!("link: the connection ended: {why}"),
            LinkEvent::Request(id, req) => self.on_request(id, req),
        }
    }

    fn refuse(&self, id: u64, why: Refusal) {
        self.conn.send(id, &Reply::Error(why));
        self.conn.drop_conn(id);
        say!("link: refused the shell's request ({why:?}); connection closed");
    }

    fn on_request(&mut self, id: u64, req: Request) {
        let Some(link) = &mut self.link else {
            return;
        };
        let now = Instant::now();
        link.requests
            .retain(|t| now.duration_since(*t) < Duration::from_secs(10));
        link.requests.push(now);
        if link.requests.len() > MAX_REQUESTS {
            self.refuse(id, Refusal::TooFast);
            return;
        }
        let greeted = link.hello == Some(id);
        match req {
            Request::Hello { version } if version == VERSION => {
                link.hello = Some(id);
                self.conn.send(id, &Reply::Hello { version: VERSION });
            }
            Request::Hello { .. } => self.refuse(id, Refusal::Version),
            _ if !greeted => self.refuse(id, Refusal::Unknown),
            Request::Status => self.conn.send(id, &Reply::Status(self.parts)),
            Request::Icons => {
                if self.on(Part::Icons) {
                    self.start_icons(Some(id));
                } else {
                    self.conn.send(id, &Reply::Icons(Icons::Off));
                }
            }
            Request::ScreenOff => {
                let answer = self.screen_off(now);
                self.conn.send(id, &Reply::ScreenOff(answer));
            }
            Request::Leaving => {
                self.keeper.leaving(now);
                self.conn.send(id, &Reply::Leaving);
            }
        }
    }

    /// Decision 16's Screen Off, asked by the shell after two hours of its
    /// screensaver: only while Lumen is in front, once a minute at most.
    fn screen_off(&mut self, now: Instant) -> ScreenOff {
        if !self.on(Part::ScreenOff) {
            return ScreenOff::Off;
        }
        if self
            .screen_last
            .is_some_and(|t| now.duration_since(t) < SCREEN_OFF_GAP)
        {
            return ScreenOff::Refused;
        }
        let Some(bus) = self.bus.clone() else {
            return ScreenOff::Failed;
        };
        match bus::foreground(bus.as_ref()) {
            Ok(Some(app)) if app == APP_ID => {}
            _ => {
                self.note(
                    "screen-off",
                    "screen-off: refused, Lumen is not in front".to_owned(),
                );
                return ScreenOff::Refused;
            }
        }
        self.screen_last = Some(now);
        let (uri, payload) = bus::screen_off();
        match bus::checked(bus.as_ref(), uri, &payload) {
            Ok(_) => {
                self.note(
                    "screen-off",
                    "screen-off: LG's Screen Off, at the shell's request".to_owned(),
                );
                ScreenOff::Done
            }
            Err(e) => {
                self.note("screen-off", format!("screen-off: FAILED: {e}"));
                ScreenOff::Failed
            }
        }
    }

    fn on_timer(&mut self, timer: Timer) {
        let now = Instant::now();
        match timer {
            Timer::S1Round => {
                for line in s1::stop_jobs(self.runner.as_ref()) {
                    self.note("s1", format!("s1: {line}"));
                }
                self.at(now + s1::next_round(self.started.elapsed()), Timer::S1Round);
            }
            Timer::LgLook | Timer::LgFallback(_) => {
                let Some(bus) = self.bus.clone() else {
                    return;
                };
                let (lines, next) = self.lg.look(bus.as_ref(), now);
                for line in lines {
                    self.note("lg-home", line);
                }
                match next {
                    Next::Nothing => {}
                    Next::LookAt(when) => self.at_most(when, Timer::LgLook),
                    Next::VerifyAt(when, pid) => self.at(when, Timer::LgVerify(pid)),
                }
                if let Timer::LgFallback(n) = timer
                    && n < 4
                {
                    self.at(now + Duration::from_secs(60), Timer::LgFallback(n + 1));
                }
            }
            Timer::LgVerify(pid) => {
                if let Some(bus) = self.bus.clone() {
                    let line = lg_home::verify(bus.as_ref(), pid.as_deref());
                    self.note("lg-home", line);
                }
            }
            Timer::ShellBoot | Timer::ShellRestart => {
                let restart = timer == Timer::ShellRestart;
                if !restart && self.keeper.started {
                    return;
                }
                // A tool launched it again meanwhile: a second launch would
                // be a relaunch, which shows a hidden shell.
                if restart && self.keeper.running() {
                    return;
                }
                // A tool closed it on purpose (an install, a bench run):
                // look again when its hold ends.
                if let Some(left) = shell_hold(&self.root) {
                    self.note(
                        "shell",
                        format!(
                            "shell: a tool holds it closed for {} s more; looking again then",
                            left.as_secs()
                        ),
                    );
                    self.timers.retain(|_, t| *t != timer);
                    self.at(now + left + Duration::from_secs(1), timer);
                    return;
                }
                if let Some(bus) = self.bus.clone() {
                    let line = self.keeper.launch(bus.as_ref(), now, restart);
                    self.note("shell", line);
                }
            }
            Timer::ModesWalk => self.walk_modes(),
            Timer::SenderKill(i) => {
                if let Some(sender) = SENDERS.get(i)
                    && let Ok(lines) = senders::stop_originals(sender, self.sys.as_ref(), true)
                {
                    lines.into_iter().for_each(|l| self.note("senders", l));
                }
            }
            Timer::BusRetry => self.start_bus(),
            Timer::Resubscribe(id) => self.subscribe(id),
        }
    }

    fn on_modes_dirty(&mut self) {
        if !self.on(Part::Modes) {
            return;
        }
        let now = Instant::now();
        let first = *self.modes_first_dirty.get_or_insert(now);
        let when = (now + MODES_QUIET).min(first + MODES_MAX_DELAY);
        self.timers.retain(|_, t| *t != Timer::ModesWalk);
        self.at(when, Timer::ModesWalk);
    }

    /// The kill switch: stand-ins and socket away, then out.
    fn kill_switch(&mut self) -> ! {
        say!(
            "kill switch: {KILL_DIR}/{KILL_NAME} exists; taking lumend's RAM changes away and stopping"
        );
        if self.on(Part::Senders) {
            for sender in SENDERS
                .iter()
                .filter(|s| self.conf.senders.contains(s.name))
            {
                for line in senders::release(sender, self.sys.as_ref()) {
                    say!("{line}");
                }
            }
        }
        if self.link.is_some() {
            match link::remove(&self.root) {
                Ok(lines) => lines.iter().for_each(|l| say!("{l}")),
                Err(e) => say!("link: {e}"),
            }
        }
        say!("kill switch: lumend stopped; what S1 stopped stays so until the next boot");
        crate::log::close();
        std::process::exit(0)
    }

    fn run(mut self, rx: &Receiver<Event>) -> ! {
        self.start();
        loop {
            let next = self
                .timers
                .keys()
                .next()
                .map(|(t, _)| t.saturating_duration_since(Instant::now()));
            // Without inotify the kill switch is looked at every second.
            let wait = match (next, self.inotify.is_none()) {
                (Some(wait), true) => Some(wait.min(Duration::from_secs(1))),
                (None, true) => Some(Duration::from_secs(1)),
                (next, false) => next,
            };
            let event = match wait {
                Some(wait) => match rx.recv_timeout(wait) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Event::Stop("a lost channel")),
                },
                None => Some(rx.recv().unwrap_or(Event::Stop("a lost channel"))),
            };
            match event {
                None => {
                    if let Some(key) = self.timers.keys().next().copied()
                        && key.0 <= Instant::now()
                        && let Some(timer) = self.timers.remove(&key)
                    {
                        self.on_timer(timer);
                    }
                }
                Some(Event::Stop(signal)) => {
                    say!("stopping on {signal}; nothing undone");
                    self.write_status();
                    crate::log::close();
                    std::process::exit(0);
                }
                Some(Event::KillSwitch) => self.kill_switch(),
                Some(Event::ModesDirty) => self.on_modes_dirty(),
                Some(Event::Luna(SUB_LIFE, Ok(msg))) => self.on_life(&msg),
                Some(Event::Luna(id, Ok(msg))) if id >= SUB_SENDER => {
                    self.on_sender((id - SUB_SENDER) as usize, &msg);
                }
                Some(Event::Luna(id, Err(e))) => {
                    say!("bus: subscription {id} failed: {e}");
                    self.at(Instant::now() + RESUBSCRIBE, Timer::Resubscribe(id));
                }
                Some(Event::Luna(..)) => {}
                Some(Event::Link(e)) => self.on_link(e),
                Some(Event::IconsDone(result)) => self.finish_icons(result),
            }
            if kill_switch(&self.root) {
                self.kill_switch();
            }
        }
    }
}

/// Starts the thread that turns inotify events into `ModesDirty` and
/// `KillSwitch`, and watches the kill switch's folder.
fn start_inotify(
    root: &Root,
    tx: Sender<Event>,
    kill_wd: Arc<AtomicI32>,
) -> Result<Arc<Inotify>, String> {
    let ino = Arc::new(Inotify::new().map_err(|e| format!("inotify: {e}"))?);
    let prefs = Dir::open_path(&root.path(KILL_DIR)).map_err(|e| format!("{KILL_DIR}: {e}"))?;
    let wd = ino
        .add(
            &prefs.proc_path(),
            inotify::IN_CREATE | inotify::IN_MOVED_TO | inotify::IN_ONLYDIR,
        )
        .map_err(|e| format!("watching {KILL_DIR}: {e}"))?;
    kill_wd.store(wd, Ordering::SeqCst);
    let reader = Arc::clone(&ino);
    std::thread::Builder::new()
        .name("inotify".to_owned())
        .stack_size(64 << 10)
        .spawn(move || {
            loop {
                let Ok(events) = reader.read() else {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                };
                let kill = kill_wd.load(Ordering::SeqCst);
                let mut dirty = false;
                for e in events {
                    if e.wd == kill {
                        if e.name.as_deref() == Some(OsStr::new(KILL_NAME))
                            && tx.send(Event::KillSwitch).is_err()
                        {
                            return;
                        }
                    } else {
                        dirty = true;
                    }
                }
                if dirty && tx.send(Event::ModesDirty).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| format!("no inotify thread: {e}"))?;
    Ok(ino)
}

/// `lumend run`: everything above. `ready` is called once the parts that
/// need no bus have started, so `lumend start` can return.
pub fn run(root: Root, ready: impl FnOnce()) -> Result<std::convert::Infallible, String> {
    crate::sys::os::block_stop_signals().map_err(|e| format!("signals: {e}"))?;
    if kill_switch(&root) {
        return Err(format!(
            "the kill switch {KILL_DIR}/{KILL_NAME} exists; nothing started"
        ));
    }
    let Some(lock) = lock(&root, true)? else {
        return Err("lumend runs already".to_owned());
    };
    let (conf, warnings) = Conf::load(&root)?;
    say!(
        "lumend {} (pid {}): {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        conf.describe()
    );
    for w in warnings {
        say!("lumend.conf: {w}");
    }
    let (tx, rx) = mpsc::channel();
    let signals = tx.clone();
    std::thread::Builder::new()
        .name("signals".to_owned())
        .stack_size(32 << 10)
        .spawn(move || {
            let _ = signals.send(Event::Stop(crate::sys::os::wait_stop_signal()));
        })
        .map_err(|e| format!("no signal thread: {e}"))?;
    let mut daemon = Daemon::new(root.clone(), conf, tx.clone(), lock);
    match start_inotify(&root, tx, Arc::clone(&daemon.kill_wd)) {
        Ok(ino) => daemon.inotify = Some(ino),
        Err(e) => {
            say!("inotify: {e}; the kill switch is checked after each event instead");
            if daemon.conf.on(Part::Modes) {
                daemon.fail(Part::Modes, "no inotify".to_owned());
            }
        }
    }
    ready();
    daemon.run(&rx)
}
