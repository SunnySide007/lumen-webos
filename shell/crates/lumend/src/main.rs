//! lumend's command line. The boot door (profiles/lumend/lumend) runs
//! `lumend start`; everything else is for the owner, through
//! `tools/lumend.sh`.

#![deny(unsafe_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lumend::bus::{self, Bus};
use lumend::conf::Conf;
use lumend::daemon;
use lumend::keeper::Keeper;
use lumend::lg_home::{self, LgHome, Next};
use lumend::paths::{APP_ID, BIN, BUS_NAME_TRY, DIR, KILL_DIR, KILL_NAME, Root, STATUS_NAME};
use lumend::run::Spawn;
use lumend::senders::{self, SENDERS, Tv};
use lumend::sys::luna::Luna;
use lumend::{groups, icons, link, modes, s1, say};
use lumend_proto::Part;

const USAGE: &str = "\
usage: lumend COMMAND
  start           start the daemon and return once it is ready (the boot door's)
  run [--stdout]  the daemon itself, in the foreground
  stop            stop the daemon (SIGTERM; it undoes nothing)
  status          the daemon's status
  check           what lumend would use, read only
  groups          the permission groups that hold each method lumend calls
  try PART [SENDER...]   one part once, by hand (the daemon must not run);
                  `try shell preload` launches with SAM's preload instead
  undo PART       a part's undo; `undo all` undoes every part
  version
PART is one of: s1 senders lg-home icons modes shell screen-off link";

/// How long `start` waits for the daemon's `ready`.
const READY_WAIT: Duration = Duration::from_secs(10);
const STOP_WAIT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["version"] => {
            println!("lumend {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        ["start"] => root_only().and_then(|()| start()),
        ["run"] => root_only().and_then(|()| run(false)),
        ["run", "--stdout"] => root_only().and_then(|()| run(true)),
        ["stop"] => root_only().and_then(|()| stop()),
        ["status"] => status(),
        ["check"] => check(),
        ["groups"] => {
            groups::report(&Root::tv())
                .iter()
                .for_each(|l| println!("{l}"));
            Ok(())
        }
        ["try", part, senders @ ..] => root_only().and_then(|()| try_part(part, senders)),
        ["undo", part] => root_only().and_then(|()| undo_part(part)),
        ["link-probe", jail, uid, gid] => root_only().and_then(|()| link_probe(jail, uid, gid)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("lumend: {}", lumend::log::clean(&e));
            ExitCode::FAILURE
        }
    }
}

fn root_only() -> Result<(), String> {
    if lumend::sys::os::euid() == 0 {
        Ok(())
    } else {
        Err("root only".to_owned())
    }
}

/// This binary's path, which must be `BIN`: the role names that path, and
/// the door runs nothing else.
fn own_exe() -> Result<PathBuf, String> {
    let exe = std::fs::read_link("/proc/self/exe").map_err(|e| format!("/proc/self/exe: {e}"))?;
    if exe != Path::new(BIN) {
        return Err(format!("run from {}, not {BIN}", exe.display()));
    }
    Ok(exe)
}

/// `start`: the daemon in a session of its own, then back once it says
/// `ready`, so the door's time box covers its start and nothing after.
fn start() -> Result<(), String> {
    let root = Root::tv();
    if daemon::kill_switch(&root) {
        return Err(format!(
            "the kill switch {KILL_DIR}/{KILL_NAME} exists; nothing started"
        ));
    }
    if let Ok(Some(pid)) = daemon::running(&root) {
        println!("lumend runs already (pid {pid})");
        return Ok(());
    }
    let exe = own_exe()?;
    let mut child = lumend::run::start_daemon(&exe, &["run"])?;
    let Some(out) = child.stdout.take() else {
        return Err("no pipe from the daemon".to_owned());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ready".to_owned())
        .stack_size(64 << 10)
        .spawn(move || {
            for line in BufReader::new(out).lines().take(64) {
                let Ok(line) = line else { break };
                let ready = line == "ready";
                if tx.send(line).is_err() || ready {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + READY_WAIT;
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(line) if line == "ready" => {
                println!("lumend started (pid {})", child.id());
                return Ok(());
            }
            Ok(line) => println!("{}", lumend::log::clean(&line)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait().map_err(|e| e.to_string())?;
                return Err(format!("the daemon ended before it was ready ({status})"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "the daemon said nothing in {} s; left running",
                    READY_WAIT.as_secs()
                ));
            }
        }
    }
}

/// `run`: the daemon. Its log is its file unless `--stdout`.
fn run(stdout: bool) -> Result<(), String> {
    let root = Root::tv();
    if stdout {
        lumend::log::to_stdout();
    } else {
        own_exe()?;
        if let Err(e) = lumend::log::to_file(&root) {
            // `start` relays this line; the daemon then logs nowhere.
            println!("log: {e}");
        }
    }
    let result = daemon::run(root, || {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(b"ready\n");
        let _ = out.flush();
    });
    match result {
        Ok(never) => match never {},
        Err(e) => {
            say!("lumend: {e}");
            lumend::log::close();
            Err(e)
        }
    }
}

/// `stop`: SIGTERM to the daemon, SIGKILL if it is still there after five
/// seconds. lumend's RAM changes stay; `undo` takes them away.
fn stop() -> Result<(), String> {
    let root = Root::tv();
    let Some(pid) = daemon::running(&root)? else {
        println!("lumend is not running");
        return Ok(());
    };
    lumend::sys::os::kill(pid, false).map_err(|e| format!("SIGTERM to {pid}: {e}"))?;
    let deadline = Instant::now() + STOP_WAIT;
    while Instant::now() < deadline {
        if daemon::running(&root)?.is_none() {
            println!("lumend stopped (pid {pid})");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    lumend::sys::os::kill(pid, true).map_err(|e| format!("SIGKILL to {pid}: {e}"))?;
    std::thread::sleep(Duration::from_millis(500));
    println!("lumend killed (pid {pid}) after {} s", STOP_WAIT.as_secs());
    Ok(())
}

/// `status`: whether it runs, and the status file it keeps.
fn status() -> Result<(), String> {
    let root = Root::tv();
    match daemon::running(&root) {
        Ok(Some(pid)) => println!("running pid {pid}"),
        Ok(None) => println!("not running"),
        Err(e) => println!("unknown: {}", lumend::log::clean(&e)),
    }
    let dir = lumend::fsat::Dir::open_path(&root.path(DIR)).map_err(|e| format!("{DIR}: {e}"))?;
    match dir.open_read(std::ffi::OsStr::new(STATUS_NAME)) {
        Ok(mut f) => {
            let (bytes, _) =
                lumend::fsat::read_capped(&mut f, 16 << 10).map_err(|e| e.to_string())?;
            for line in String::from_utf8_lossy(&bytes).lines() {
                println!("{}", lumend::log::clean(line));
            }
        }
        Err(e) => println!("no status file: {e}"),
    }
    Ok(())
}

/// A bus client under the trial name, for `try` and `check`.
fn bus() -> Result<Arc<dyn Bus>, String> {
    let sink: lumend::sys::luna::Sink = Arc::new(|_, _| {});
    Ok(Arc::new(Luna::start(BUS_NAME_TRY, sink)?))
}

/// `check`: what lumend would use, changing nothing.
fn check() -> Result<(), String> {
    let root = Root::tv();
    let me = lumend::sys::os::euid();
    println!(
        "euid {me}{}",
        if me == 0 {
            ""
        } else {
            " (NOT ROOT: most checks fail)"
        }
    );
    match own_exe() {
        Ok(_) => println!("binary: {BIN}"),
        Err(e) => println!("binary: {e}"),
    }
    match Conf::load(&root) {
        Ok((conf, warnings)) => {
            println!("conf: {}", conf.describe());
            warnings.iter().for_each(|w| println!("conf: {w}"));
        }
        Err(e) => println!("conf: REFUSED: {e}"),
    }
    println!(
        "kill switch: {}",
        if daemon::kill_switch(&root) {
            "SET (lumend will not start)"
        } else {
            "not set"
        }
    );
    match daemon::running(&root) {
        Ok(Some(pid)) => println!("daemon: running, pid {pid}"),
        Ok(None) => println!("daemon: not running"),
        Err(e) => println!("daemon: {e}"),
    }
    match senders::standin(&Tv) {
        Ok(_) => println!("stand-in: {}", lumend::paths::STANDIN),
        Err(e) => println!("stand-in: {e}"),
    }
    for s in SENDERS {
        let held = senders::held(s, &Tv);
        println!(
            "sender {}: {}",
            s.name,
            if held { "held down" } else { "LG's program" }
        );
    }
    match bus() {
        Ok(bus) => {
            println!("bus: registered as {BUS_NAME_TRY}");
            match bus::foreground(bus.as_ref()) {
                Ok(app) => println!("bus: in front: {}", app.as_deref().unwrap_or("nothing")),
                Err(e) => println!("bus: getForegroundAppInfo: {e}"),
            }
            match bus::default_home(bus.as_ref()) {
                Ok(home) => println!("bus: default home: {home}"),
                Err(e) => println!("bus: the default home: {e}"),
            }
        }
        Err(e) => println!("bus: {e}"),
    }
    Ok(())
}

/// `try` and `undo` hold the daemon's lock, so neither runs beside it.
fn trial_lock(root: &Root) -> Result<std::fs::File, String> {
    if daemon::kill_switch(root) {
        return Err(format!("the kill switch {KILL_DIR}/{KILL_NAME} exists"));
    }
    daemon::lock(root, true)?.ok_or_else(|| "lumend runs; `lumend stop` first".to_owned())
}

fn part(name: &str) -> Result<Part, String> {
    Part::from_name(name).ok_or_else(|| format!("no part named {}", lumend::log::clean(name)))
}

/// `try PART`: the part once, as the daemon would do it, saying each step.
fn try_part(name: &str, names: &[&str]) -> Result<(), String> {
    let root = Root::tv();
    let part = part(name)?;
    let preload = part == Part::Shell && names == ["preload"];
    if part != Part::Senders && !names.is_empty() && !preload {
        return Err("only `try senders` and `try shell preload` take more words".to_owned());
    }
    let _lock = trial_lock(&root)?;
    lumend::log::to_stdout();
    match part {
        Part::S1 => {
            match s1::add_rule(&Spawn) {
                Ok(line) => say!("s1: {line}"),
                Err(e) => say!("s1: {e}"),
            }
            let lines = s1::stop_jobs(&Spawn);
            if lines.is_empty() {
                say!("s1: none of {} runs", s1::JOBS.join(", "));
            }
            lines.iter().for_each(|l| say!("s1: {l}"));
        }
        Part::Senders => {
            if names.is_empty() {
                return Err(format!(
                    "name the senders: {}",
                    SENDERS.iter().map(|s| s.name).collect::<Vec<_>>().join(" ")
                ));
            }
            for name in names {
                let Some(sender) = SENDERS.iter().find(|s| s.name == *name) else {
                    return Err(format!("no sender named {}", lumend::log::clean(name)));
                };
                senders::hold(sender, &Tv)?.iter().for_each(|l| say!("{l}"));
                std::thread::sleep(Duration::from_secs(2));
                senders::stop_originals(sender, &Tv, true)?
                    .iter()
                    .for_each(|l| say!("{l}"));
                say!(
                    "{}: {}",
                    sender.name,
                    if senders::held(sender, &Tv) {
                        "held down until the next boot or `lumend undo senders`"
                    } else {
                        "NOT held"
                    }
                );
            }
        }
        Part::LgHome => {
            let bus = bus()?;
            let mut lg = LgHome::default();
            let (lines, next) = lg.look(bus.as_ref(), Instant::now());
            lines.iter().for_each(|l| say!("{l}"));
            match next {
                Next::VerifyAt(when, pid) => {
                    std::thread::sleep(when.saturating_duration_since(Instant::now()));
                    say!("{}", lg_home::verify(bus.as_ref(), pid.as_deref()));
                }
                Next::LookAt(when) => say!(
                    "lg-home: the daemon would look again in {} s",
                    when.saturating_duration_since(Instant::now()).as_secs()
                ),
                Next::Nothing => {}
            }
        }
        Part::Icons => {
            let bus = bus()?;
            say!("{}", icons::describe(&icons::copy(&root, bus.as_ref())));
        }
        Part::Modes => say!("{}", modes::fix(&root, &mut |_| ())?.describe()),
        Part::Shell => {
            let bus = bus()?;
            if preload {
                // depth.md's fact 10, as it is asked: SAM's own preload.
                let (uri, payload) = bus::launch_shell_preload();
                match bus::checked(bus.as_ref(), uri, &payload) {
                    Ok(_) => say!("shell: launched with preload full"),
                    Err(e) => say!("shell: FAILED to launch with preload: {e}"),
                }
            } else {
                say!(
                    "{}",
                    Keeper::default().launch(bus.as_ref(), Instant::now(), false)
                );
            }
            std::thread::sleep(Duration::from_secs(3));
            match bus::foreground(bus.as_ref()) {
                Ok(app) if app.as_deref() == Some(APP_ID) => {
                    say!("shell: Lumen is IN FRONT: this shell does not start hidden");
                }
                Ok(app) => say!("shell: in front: {}", app.as_deref().unwrap_or("nothing")),
                Err(e) => say!("shell: getForegroundAppInfo: {e}"),
            }
            match bus::running(bus.as_ref()) {
                Ok(list) => say!(
                    "shell: {}",
                    if list.iter().any(|r| r.id == APP_ID) {
                        "running"
                    } else {
                        "NOT running"
                    }
                ),
                Err(e) => say!("shell: running: {e}"),
            }
        }
        Part::ScreenOff => {
            let bus = bus()?;
            match bus::foreground(bus.as_ref())? {
                Some(app) if app == APP_ID => {
                    let (uri, payload) = bus::screen_off();
                    match bus::checked(bus.as_ref(), uri, &payload) {
                        Ok(_) => {
                            say!("screen-off: done; any key on the remote turns the screen on")
                        }
                        Err(e) => say!("screen-off: FAILED: {e}"),
                    }
                }
                other => say!(
                    "screen-off: refused: Lumen is not in front ({})",
                    other.as_deref().unwrap_or("nothing")
                ),
            }
        }
        Part::Link => {
            let exe = own_exe()?;
            link::probe(&root, &exe).iter().for_each(|l| say!("{l}"));
        }
    }
    Ok(())
}

/// `undo PART`: what the part changed, taken away.
fn undo_part(name: &str) -> Result<(), String> {
    let root = Root::tv();
    let parts: Vec<Part> = if name == "all" {
        Part::ALL.to_vec()
    } else {
        vec![part(name)?]
    };
    // Undo runs with the kill switch set, too: it is how the switch's
    // effects are finished off by hand.
    let _lock =
        daemon::lock(&root, true)?.ok_or_else(|| "lumend runs; `lumend stop` first".to_owned())?;
    lumend::log::to_stdout();
    for part in parts {
        match part {
            Part::S1 => s1::undo(&Spawn).iter().for_each(|l| say!("s1: {l}")),
            Part::Senders => {
                for s in SENDERS {
                    senders::release(s, &Tv).iter().for_each(|l| say!("{l}"));
                }
            }
            Part::Icons => match icons::remove(&root) {
                Ok(lines) => lines.iter().for_each(|l| say!("{l}")),
                Err(e) => say!("icons: {e}"),
            },
            Part::Link => match link::remove(&root) {
                Ok(lines) => lines.iter().for_each(|l| say!("{l}")),
                Err(e) => say!("link: {e}"),
            },
            Part::LgHome => say!("lg-home: nothing to undo; LG Home opens again when called"),
            Part::Modes => say!(
                "modes: nothing to undo; LG's devmode job opens the folder again at the next boot"
            ),
            Part::Shell => say!("shell: nothing to undo; close Lumen as any app"),
            Part::ScreenOff => say!("screen-off: nothing to undo"),
        }
    }
    Ok(())
}

/// `link-probe JAIL UID GID`: `try link`'s child.
fn link_probe(jail: &str, uid: &str, gid: &str) -> Result<(), String> {
    let uid: u32 = uid.parse().map_err(|_| "a uid is a number".to_owned())?;
    let gid: u32 = gid.parse().map_err(|_| "a gid is a number".to_owned())?;
    if uid == 0 || gid == 0 {
        return Err("the probe never runs as root".to_owned());
    }
    let expected = Root::tv().path(&format!("{}/{APP_ID}", lumend::paths::JAILS));
    if Path::new(jail) != expected {
        return Err("the probe enters Lumen's jail only".to_owned());
    }
    println!("{}", link::probe_child(Path::new(jail), uid, gid)?);
    Ok(())
}
