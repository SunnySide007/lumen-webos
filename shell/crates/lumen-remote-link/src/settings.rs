//! The app's folder and settings file.
//!
//! Everything the app keeps is in one folder, `~/.lumen/remote/` (or the one
//! `LUMEN_REMOTE_DIR` names, for tests): the key `tools/remote.sh keygen`
//! made (`id_ed25519`), the settings (`tv.conf`) and the app's own log
//! (`app.log`). `tv.conf` is lines of `name value`, like lumen-ctl's
//! `ctl.conf`:
//!
//! ```text
//! host 192.168.86.20
//! port 22
//! mac a8:23:fe:01:02:03
//! ```
//!
//! `ssh` names another ssh than Windows' own, and `LUMEN_SSH` and
//! `LUMEN_REMOTE_KEY` override the ssh and the key as they do for ctl.py.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub const DIR_ENV: &str = "LUMEN_REMOTE_DIR";
pub const KEY_ENV: &str = "LUMEN_REMOTE_KEY";
pub const SSH_ENV: &str = "LUMEN_SSH";
pub const DEFAULT_PORT: u16 = 22;
/// The settings file is a few lines; anything larger isn't ours.
const MAX_CONF: u64 = 4 << 10;

/// Where the app keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub dir: PathBuf,
    pub key: PathBuf,
}

impl Paths {
    /// `~/.lumen/remote/`, or what the environment names (`get` reads it).
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let dir = match get(DIR_ENV).filter(|d| !d.is_empty()) {
            Some(d) => PathBuf::from(d),
            None => home(&get)?.join(".lumen").join("remote"),
        };
        let key = get(KEY_ENV)
            .filter(|k| !k.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("id_ed25519"));
        Some(Self { dir, key })
    }

    pub fn conf(&self) -> PathBuf {
        self.dir.join("tv.conf")
    }

    pub fn log(&self) -> PathBuf {
        self.dir.join("app.log")
    }

    /// The settings, or the defaults when there is no file yet.
    pub fn load(&self) -> Result<Settings, String> {
        let path = self.conf();
        let text = match read_small(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Settings::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Writes the settings through a file beside them, so a crash leaves the
    /// old ones or the new ones, never half.
    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        let path = self.conf();
        let tmp = self.dir.join("tv.conf.new");
        std::fs::create_dir_all(&self.dir)
            .and_then(|()| std::fs::write(&tmp, settings.render()))
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn home(get: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    get(name).filter(|h| !h.is_empty()).map(PathBuf::from)
}

fn read_small(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_CONF + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_CONF {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "longer than 4 KiB",
        ));
    }
    Ok(text)
}

/// A MAC address, for Wake-on-LAN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mac(pub [u8; 6]);

impl Mac {
    /// Six pairs of hex digits, joined by `:` or `-`, or not at all.
    pub fn parse(text: &str) -> Result<Self, String> {
        let digits: String = text
            .trim()
            .chars()
            .filter(|c| *c != ':' && *c != '-')
            .collect();
        let bad = || format!("not a MAC address: {text:?} (six pairs of hex digits)");
        if digits.len() != 12 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let mut mac = [0u8; 6];
        for (i, byte) in mac.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
        }
        Ok(Self(mac))
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

/// The TV's address: a host name or an IP address, as ctl.py takes it.
/// Nothing that ssh could read as an option.
pub fn check_host(host: &str) -> Result<(), String> {
    let ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:%-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("not a host name or address: {host:?}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub host: Option<String>,
    pub port: u16,
    pub mac: Option<Mac>,
    pub ssh: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            host: None,
            port: DEFAULT_PORT,
            mac: None,
            ssh: None,
        }
    }
}

impl Settings {
    /// `name value` lines; `#` starts a comment line. A name it doesn't
    /// know, or a value it can't use, is refused with its line number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut s = Self::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let value = value.trim();
            let at = |e: String| format!("line {}: {e}", n + 1);
            match name {
                "host" => {
                    check_host(value).map_err(at)?;
                    s.host = Some(value.to_owned());
                }
                "port" => s.port = parse_port(value).map_err(at)?,
                "mac" => s.mac = Some(Mac::parse(value).map_err(at)?),
                "ssh" if !value.is_empty() => s.ssh = Some(PathBuf::from(value)),
                "ssh" => return Err(at("ssh needs a path".into())),
                other => return Err(at(format!("unknown setting {other:?}"))),
            }
        }
        Ok(s)
    }

    pub fn render(&self) -> String {
        let mut out =
            String::from("# Lumen Remote's settings (docs/specs/remote.md, R3): name value.\n");
        if let Some(host) = &self.host {
            out += &format!("host {host}\n");
        }
        out += &format!("port {}\n", self.port);
        if let Some(mac) = self.mac {
            out += &format!("mac {mac}\n");
        }
        if let Some(ssh) = &self.ssh {
            out += &format!("ssh {}\n", ssh.display());
        }
        out
    }
}

pub fn parse_port(value: &str) -> Result<u16, String> {
    match value.parse::<u16>() {
        Ok(p) if p > 0 && value.bytes().all(|b| b.is_ascii_digit()) => Ok(p),
        _ => Err(format!("not a port number: {value:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_it_writes() {
        let s = Settings {
            host: Some("192.168.86.20".into()),
            port: 2222,
            mac: Some(Mac([0xa8, 0x23, 0xfe, 0x01, 0x02, 0x03])),
            ssh: Some(PathBuf::from(r"C:\Program Files\ssh.exe")),
        };
        assert_eq!(Settings::parse(&s.render()), Ok(s));
        assert_eq!(Settings::parse(""), Ok(Settings::default()));
    }

    #[test]
    fn refuses_what_it_cannot_use_with_the_line() {
        for (text, want) in [
            ("host -oProxyCommand=x", "line 1: not a host name"),
            ("# c\nport 0", "line 2: not a port number"),
            ("port +22", "not a port number"),
            ("mac 01:02:03", "not a MAC address"),
            ("colour blue", "unknown setting \"colour\""),
            ("ssh", "ssh needs a path"),
            ("host a b", "not a host name"),
        ] {
            let err = Settings::parse(text).unwrap_err();
            assert!(err.contains(want), "{text:?}: {err}");
        }
    }

    #[test]
    fn macs_take_colons_dashes_or_nothing() {
        let want = Mac([0xa8, 0x23, 0xfe, 0x01, 0x02, 0xff]);
        for text in ["a8:23:fe:01:02:ff", "A8-23-FE-01-02-FF", "a823fe0102ff"] {
            assert_eq!(Mac::parse(text), Ok(want), "{text}");
        }
        assert_eq!(want.to_string(), "a8:23:fe:01:02:ff");
        assert!(Mac::parse("a8:23:fe:01:02:fg").is_err());
        assert!(Mac::parse("a8:23:fe:01:02:ff:00").is_err());
    }

    #[test]
    fn the_folder_is_under_home_unless_named() {
        fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect();
            move |name| {
                pairs
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
            }
        }
        let p = Paths::from_env(env(&[("HOME", "/h"), ("USERPROFILE", "/h")])).unwrap();
        assert_eq!(p.dir, Path::new("/h").join(".lumen").join("remote"));
        assert_eq!(p.key, p.dir.join("id_ed25519"));
        let p = Paths::from_env(env(&[(DIR_ENV, "/d"), (KEY_ENV, "/k")])).unwrap();
        assert_eq!((p.dir, p.key), (PathBuf::from("/d"), PathBuf::from("/k")));
        assert_eq!(Paths::from_env(env(&[])), None);
    }

    #[test]
    fn saves_and_loads_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("lumen-link-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            key: dir.join("id_ed25519"),
            dir: dir.clone(),
        };
        assert_eq!(paths.load(), Ok(Settings::default()));
        let s = Settings {
            host: Some("tv.lan".into()),
            ..Settings::default()
        };
        paths.save(&s).unwrap();
        assert_eq!(paths.load(), Ok(s));
        std::fs::write(paths.conf(), "x".repeat(5000)).unwrap();
        assert!(paths.load().unwrap_err().contains("longer than 4 KiB"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
