//! The command line, parsed by hand as lumen-guard's is, and the PM's
//! settings file. Anything unknown is refused: only `authorized_keys`'
//! forced command and Lumen's own scripts start lumen-ctl, and the forced
//! command passes no flags (the SSH client's own command, in
//! `SSH_ORIGINAL_COMMAND`, is never read).

use std::path::PathBuf;
use std::time::Duration;

use crate::keys::KEYS_FILE;
use crate::log::LOG_FILE;

pub const USAGE: &str = "\
usage: lumen-ctl serve [--bus library|command] [--keys none|keymanager] [--log FILE]
       lumen-ctl probe [--bus library|command]
       lumen-ctl probe-keys [--bus library|command]

serve speaks Lumen Remote's protocol (remote/ui/src/protocol.ts) on stdin and
stdout, one JSON object per line, until stdin ends or SIGTERM or SIGHUP comes.
--bus picks how luna is reached: library (libluna-service2, with
subscriptions) or command (/usr/bin/luna-send, polled). --keys picks the route
for `key`: none, or keymanager (LG's createKeyEvent, from the template in
/var/lib/lumen/remote/keys.json). Without the flags, /var/lib/lumen/remote/
ctl.conf decides (`bus command`, `keys none` lines), else command and none.
--log is where each request and reply is logged (default
/var/lib/lumen/remote/lumen-ctl.log, moved to .1 at 1 MiB).

probe makes only read-only calls and says which of them this TV's bus allows,
over library and then command (or the one --bus names). probe-keys calls
createKeyEvent with no parameters, so LG's error names the ones it wants; no
key is sent. It uses command unless --bus says otherwise.

For tests and the PC: --bus fake (a TV in the process, no luna), --config
FILE, --keys-file FILE and --alive-ms N.
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
}

impl KeysKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "keymanager" => Ok(Self::Keymanager),
            other => Err(format!("--keys must be none or keymanager, not {other}")),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Serve(ServeFlags),
    /// `None`: library, then command.
    Probe(Option<BusKind>),
    ProbeKeys(BusKind),
    Help,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((sub, rest)) = args.split_first() else {
        return Err("no command".to_owned());
    };
    if matches!(sub.as_str(), "-h" | "--help" | "help") {
        return Ok(Command::Help);
    }
    let mut serve = ServeFlags {
        bus: None,
        keys: None,
        log: PathBuf::from(LOG_FILE),
        config: PathBuf::from(CONFIG_FILE),
        keys_file: PathBuf::from(KEYS_FILE),
        alive: ALIVE,
    };
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
            "--log" if serve_only => serve.log = PathBuf::from(value),
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
    fn a_bad_settings_file_stops_serve() {
        for bad in ["bus fake\n", "bus\n", "keys uinput\n", "colour blue\n"] {
            assert!(resolve(flags(&["serve"]), Some(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        for bad in [
            &["serve", "--bus", "dbus"][..],
            &["serve", "--keys", "uinput"],
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
