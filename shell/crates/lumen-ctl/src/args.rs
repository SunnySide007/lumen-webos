//! The command line, parsed by hand as lumen-guard's is, and the PM's
//! settings file. Anything unknown is refused: only `authorized_keys`'
//! forced command and Lumen's own scripts start lumen-ctl, and the forced
//! command passes no flags (the SSH client's own command, in
//! `SSH_ORIGINAL_COMMAND`, is never read).

use std::path::PathBuf;
use std::time::Duration;

use crate::devices::STATE_DIR;
use crate::keys::KEYS_FILE;
use crate::log::{LOG_FILE, WEB_LOG_FILE};
use crate::protocol::Link;

pub const USAGE: &str = "\
usage: lumen-ctl serve [--bus library|command] [--keys none|keymanager|inject] [--log FILE]
                       [--link ssh|web]
       lumen-ctl probe [--bus library|command]
       lumen-ctl probe-keys [--bus library|command]
       lumen-ctl play FILE [--settle-ms N]
       lumen-ctl key NAME [--hold-ms N] [--settle-ms N]
       lumen-ctl watch-remote SECONDS

serve speaks Lumen Remote's protocol (remote/ui/src/protocol.ts) on stdin and
stdout, one JSON object per line, until stdin ends or SIGTERM or SIGHUP comes.
--bus picks how luna is reached: library (libluna-service2, with
subscriptions) or command (/usr/bin/luna-send, polled). --keys picks the route
for `key`: none, keymanager (LG's createKeyEvent, from the template in
/var/lib/lumen/remote/keys.json) or inject (the robot remote, as play
makes). Without the flags, /var/lib/lumen/remote/
ctl.conf decides (`bus command`, `keys none` lines), else command and none.
--log is where each request and reply is logged (default
/var/lib/lumen/remote/lumen-ctl.log, moved to .1 at 1 MiB).
--link says who is at the other end: ssh (the default, and the forced
command's) is the PC, with owner rights and the pairing verbs; web is
lumen-remote, whose every request carries a paired device's token, checked
against /var/lib/lumen/remote/devices.json (log lumen-ctl-web.log).

probe makes only read-only calls and says which of them this TV's bus allows,
over library and then command (or the one --bus names). probe-keys calls
createKeyEvent with no parameters, so LG's error names the ones it wants; no
key is sent. It uses command unless --bus says otherwise.

play, key and watch-remote are the bench's robot remote (docs/device/
bench.md): play writes a pattern file's keys into the Magic Remote's own
input node (or, on a TV without one, a virtual remote made through
/dev/uinput), after --settle-ms (default 1500); key plays one press (held
--hold-ms, default 60) of up, down, left, right, ok, back, home, play,
pause, stop, rewind or fastforward. Both watch every input node meanwhile,
never grabbing it, and stop at the first person's event (exit 3); the
robot's own keys coming back on the remote's node are echoes, not a
person. watch-remote prints the people's events on those nodes for
SECONDS, with the kernel's stamps. Each prints JSON lines.

For tests and the PC: --bus fake (a TV in the process, no luna), --config
FILE, --keys-file FILE, --state DIR (where devices.json and pairing.json
are) and --alive-ms N.
";

pub const CONFIG_FILE: &str = "/var/lib/lumen/remote/ctl.conf";
pub const ALIVE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusKind {
    Library,
    Command,
    Fake,
}

impl BusKind {
    fn parse(value: &str, fake: bool) -> Result<Self, String> {
        match value {
            "library" => Ok(Self::Library),
            "command" => Ok(Self::Command),
            "fake" if fake => Ok(Self::Fake),
            other => Err(format!("--bus must be library or command, not {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysKind {
    None,
    Keymanager,
    Inject,
}

impl KeysKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "keymanager" => Ok(Self::Keymanager),
            "inject" => Ok(Self::Inject),
            other => Err(format!(
                "--keys must be none, keymanager or inject, not {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serve {
    pub bus: BusKind,
    pub keys: KeysKind,
    pub log: PathBuf,
    pub keys_file: PathBuf,
    pub alive: Duration,
    pub link: Link,
    pub state: PathBuf,
}

/// `serve`'s flags before the settings file fills in what they leave out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeFlags {
    pub bus: Option<BusKind>,
    pub keys: Option<KeysKind>,
    pub log: PathBuf,
    pub config: PathBuf,
    pub keys_file: PathBuf,
    pub alive: Duration,
    pub link: Link,
    pub state: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Serve(ServeFlags),
    /// `None`: library, then command.
    Probe(Option<BusKind>),
    ProbeKeys(BusKind),
    Play {
        file: PathBuf,
        settle: Duration,
    },
    Key {
        name: String,
        hold_ms: u64,
        settle: Duration,
    },
    WatchRemote(u64),
    Help,
}

fn ms_in(flag: &str, value: &str, lo: u64, hi: u64) -> Result<u64, String> {
    value
        .parse()
        .ok()
        .filter(|ms| (lo..=hi).contains(ms))
        .ok_or_else(|| format!("{flag} must be {lo} to {hi}, not {value}"))
}

/// The robot remote's commands: one word, then their own flags.
fn parse_robot(sub: &str, rest: &[String]) -> Result<Command, String> {
    let Some((word, flags)) = rest.split_first() else {
        return Err(format!("{sub} needs an argument"));
    };
    let (mut settle, mut hold_ms) = (crate::robot::SETTLE, 60);
    let mut it = flags.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match (sub, flag.as_str()) {
            ("play" | "key", "--settle-ms") => {
                settle = Duration::from_millis(ms_in(flag, value, 0, 10_000)?)
            }
            ("key", "--hold-ms") => hold_ms = ms_in(flag, value, 10, 5_000)?,
            _ => return Err(format!("unknown argument for {sub}: {flag}")),
        }
    }
    match sub {
        "play" => Ok(Command::Play {
            file: PathBuf::from(word),
            settle,
        }),
        "key" => Ok(Command::Key {
            name: word.clone(),
            hold_ms,
            settle,
        }),
        _ => Ok(Command::WatchRemote(ms_in("SECONDS", word, 1, 3_600)?)),
    }
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = args.split_first() else {
        return Err("no command".to_owned());
    };
    if matches!(sub.as_str(), "-h" | "--help" | "help") {
        return Ok(Command::Help);
    }
    if matches!(sub.as_str(), "play" | "key" | "watch-remote") {
        return parse_robot(sub, rest);
    }
    let mut serve = ServeFlags {
        bus: None,
        keys: None,
        log: PathBuf::from(LOG_FILE),
        config: PathBuf::from(CONFIG_FILE),
        keys_file: PathBuf::from(KEYS_FILE),
        alive: ALIVE,
        link: Link::Ssh,
        state: PathBuf::from(STATE_DIR),
    };
    let mut log_given = false;
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        if flag == "-h" || flag == "--help" {
            return Ok(Command::Help);
        }
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let serve_only = sub == "serve";
        match flag.as_str() {
            "--bus" => serve.bus = Some(BusKind::parse(value, true)?),
            "--keys" if serve_only => serve.keys = Some(KeysKind::parse(value)?),
            "--log" if serve_only => {
                serve.log = PathBuf::from(value);
                log_given = true;
            }
            "--link" if serve_only => {
                serve.link = match value.as_str() {
                    "ssh" => Link::Ssh,
                    "web" => Link::Web,
                    other => return Err(format!("--link must be ssh or web, not {other}")),
                }
            }
            "--state" if serve_only => serve.state = PathBuf::from(value),
            "--config" if serve_only => serve.config = PathBuf::from(value),
            "--keys-file" if serve_only => serve.keys_file = PathBuf::from(value),
            "--alive-ms" if serve_only => {
                let ms: u64 = value
                    .parse()
                    .ok()
                    .filter(|ms| (100..=60_000).contains(ms))
                    .ok_or_else(|| format!("--alive-ms must be 100 to 60000, not {value}"))?;
                serve.alive = Duration::from_millis(ms);
            }
            other => return Err(format!("unknown argument for {sub}: {other}")),
        }
    }
    if serve.link == Link::Web && !log_given {
        serve.log = PathBuf::from(WEB_LOG_FILE);
    }
    match sub.as_str() {
        "serve" => Ok(Command::Serve(serve)),
        "probe" => Ok(Command::Probe(serve.bus)),
        "probe-keys" => Ok(Command::ProbeKeys(serve.bus.unwrap_or(BusKind::Command))),
        other => Err(format!("unknown command: {other}")),
    }
}

/// The flags, then `ctl.conf` (`text`, when it exists), then the defaults:
/// the command bus, which works as root today, and no keys.
pub fn resolve(flags: ServeFlags, text: Option<&str>) -> Result<Serve, String> {
    let (mut bus, mut keys) = (None, None);
    for (n, line) in text.unwrap_or("").lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("ctl.conf line {}: {line:?}", n + 1);
        let (key, value) = line.split_once(char::is_whitespace).ok_or_else(bad)?;
        match key {
            "bus" => {
                bus = Some(
                    BusKind::parse(value.trim(), false).map_err(|e| format!("{}: {e}", bad()))?,
                )
            }
            "keys" => {
                keys = Some(KeysKind::parse(value.trim()).map_err(|e| format!("{}: {e}", bad()))?)
            }
            _ => return Err(bad()),
        }
    }
    Ok(Serve {
        bus: flags.bus.or(bus).unwrap_or(BusKind::Command),
        keys: flags.keys.or(keys).unwrap_or(KeysKind::None),
        log: flags.log,
        keys_file: flags.keys_file,
        alive: flags.alive,
        link: flags.link,
        state: flags.state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn flags(list: &[&str]) -> ServeFlags {
        match parse(&args(list)) {
            Ok(Command::Serve(f)) => f,
            other => panic!("not serve: {other:?}"),
        }
    }

    #[test]
    fn the_forced_command_is_serve_with_the_defaults() {
        let f = flags(&["serve"]);
        assert_eq!(f.bus, None);
        assert_eq!(f.log, PathBuf::from("/var/lib/lumen/remote/lumen-ctl.log"));
        assert_eq!(f.config, PathBuf::from("/var/lib/lumen/remote/ctl.conf"));
        assert_eq!(f.alive, Duration::from_secs(10));
        assert_eq!(f.link, Link::Ssh);
        assert_eq!(f.state, PathBuf::from("/var/lib/lumen/remote"));
        let s = resolve(f, None).unwrap();
        assert_eq!((s.bus, s.keys), (BusKind::Command, KeysKind::None));
        assert_eq!(
            s.keys_file,
            PathBuf::from("/var/lib/lumen/remote/keys.json")
        );
    }

    #[test]
    fn flags_win_over_the_settings_file() {
        let conf = "# set by tools/remote.sh config\nbus library\nkeys keymanager\n";
        let s = resolve(flags(&["serve"]), Some(conf)).unwrap();
        assert_eq!((s.bus, s.keys), (BusKind::Library, KeysKind::Keymanager));
        let s = resolve(
            flags(&["serve", "--bus", "command", "--keys", "none"]),
            Some(conf),
        )
        .unwrap();
        assert_eq!((s.bus, s.keys), (BusKind::Command, KeysKind::None));
        let s = resolve(
            flags(&[
                "serve",
                "--bus",
                "fake",
                "--alive-ms",
                "200",
                "--log",
                "/tmp/x",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(s.bus, BusKind::Fake);
        assert_eq!(s.alive, Duration::from_millis(200));
        assert_eq!(s.log, PathBuf::from("/tmp/x"));
    }

    #[test]
    fn the_web_link_has_its_own_log() {
        let f = flags(&["serve", "--link", "web"]);
        assert_eq!(f.link, Link::Web);
        assert_eq!(
            f.log,
            PathBuf::from("/var/lib/lumen/remote/lumen-ctl-web.log")
        );
        let f = flags(&[
            "serve", "--log", "/tmp/w", "--link", "web", "--state", "/tmp/s",
        ]);
        assert_eq!(
            (f.log, f.state),
            (PathBuf::from("/tmp/w"), PathBuf::from("/tmp/s"))
        );
        for bad in [
            &["serve", "--link", "owner"][..],
            &["probe", "--link", "web"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_bad_settings_file_stops_serve() {
        for bad in ["bus fake\n", "bus\n", "keys usb\n", "colour blue\n"] {
            assert!(resolve(flags(&["serve"]), Some(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        for bad in [
            &["serve", "--bus", "dbus"][..],
            &["serve", "--keys", "usb"],
            &["serve", "--alive-ms", "5"],
            &["serve", "--shell", "sh"],
            &["serve", "--bus"],
            &["probe", "--keys", "none"],
            &["probe", "--log", "/tmp/x"],
            &["sh"],
            &["serve", "-c", "id"],
            &[],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parses_the_robot_remote() {
        assert_eq!(
            parse(&args(&["play", "/var/lib/lumen/bench/p.txt"])),
            Ok(Command::Play {
                file: PathBuf::from("/var/lib/lumen/bench/p.txt"),
                settle: Duration::from_millis(1500)
            })
        );
        assert_eq!(
            parse(&args(&[
                "key",
                "home",
                "--hold-ms",
                "180",
                "--settle-ms",
                "0"
            ])),
            Ok(Command::Key {
                name: "home".to_owned(),
                hold_ms: 180,
                settle: Duration::ZERO
            })
        );
        assert_eq!(
            parse(&args(&["watch-remote", "30"])),
            Ok(Command::WatchRemote(30))
        );
        for bad in [
            &["play"][..],
            &["play", "p", "--hold-ms", "10"],
            &["key", "up", "--settle-ms", "20000"],
            &["key", "up", "--hold-ms"],
            &["watch-remote", "0"],
            &["watch-remote", "x"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parses_the_probes() {
        assert_eq!(parse(&args(&["probe"])), Ok(Command::Probe(None)));
        assert_eq!(
            parse(&args(&["probe", "--bus", "library"])),
            Ok(Command::Probe(Some(BusKind::Library)))
        );
        assert_eq!(
            parse(&args(&["probe-keys"])),
            Ok(Command::ProbeKeys(BusKind::Command))
        );
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["serve", "-h"])), Ok(Command::Help));
    }
}
