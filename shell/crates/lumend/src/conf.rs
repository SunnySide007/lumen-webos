//! `lumend.conf`: which parts run. Every part is off unless a line turns it
//! on, so a missing or unreadable file means a daemon that does nothing
//! (docs/safety.md, rule 3: each part tried by hand before it runs at boot).
//!
//! ```text
//! # lumend's switches; tools/lumend.sh conf writes this file.
//! s1 on
//! lg-home on
//! sender acr2 on
//! ```
//!
//! One switch a line: a part's name (`lumend_proto::Part`) or `sender
//! <name>` (S2's stand-ins, each its own switch, from `senders::SENDERS`),
//! then `on` or `off`. Blank lines and `#` comments are skipped; any other
//! line is reported and ignored. The file is read only if it is a regular
//! file root owns, writable by no one else, in root's own folder.

use std::collections::BTreeSet;
use std::ffi::OsStr;

use lumend_proto::Part;

use crate::fsat::{Dir, check_private, read_capped};
use crate::paths::{CONF_NAME, DIR, Root};
use crate::senders::SENDERS;

/// The largest settings file read.
const MAX_BYTES: usize = 4 << 10;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conf {
    parts: BTreeSet<Part>,
    /// The senders switched on, by their names in `SENDERS`.
    pub senders: BTreeSet<&'static str>,
}

impl Conf {
    pub fn on(&self, part: Part) -> bool {
        self.parts.contains(&part)
    }

    pub fn set(&mut self, part: Part, on: bool) {
        if on {
            self.parts.insert(part);
        } else {
            self.parts.remove(&part);
        }
    }

    /// Reads `text`; the second value lists the lines it ignored, and why.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut conf = Self::default();
        let mut warnings = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let on = match words.last() {
                Some(&"on") => true,
                Some(&"off") => false,
                _ => {
                    warnings.push(format!("line {}: ends in neither on nor off", n + 1));
                    continue;
                }
            };
            match words.as_slice() {
                [name, _] => match Part::from_name(name) {
                    Some(part) => conf.set(part, on),
                    None => warnings.push(format!("line {}: no part named {name}", n + 1)),
                },
                ["sender", name, _] => match SENDERS.iter().find(|s| s.name == *name) {
                    Some(sender) => {
                        if on {
                            conf.senders.insert(sender.name);
                        } else {
                            conf.senders.remove(sender.name);
                        }
                    }
                    None => warnings.push(format!("line {}: no sender named {name}", n + 1)),
                },
                _ => warnings.push(format!("line {}: not a switch", n + 1)),
            }
        }
        // Clean words only in what is echoed back.
        let warnings = warnings
            .into_iter()
            .map(|w| crate::log::clean(&w))
            .collect();
        (conf, warnings)
    }

    /// The settings from root's folder; everything off when the file is
    /// missing, and a refusal when it or the folder fails its check.
    pub fn load(root: &Root) -> Result<(Self, Vec<String>), String> {
        let me = crate::sys::os::euid();
        let dir = Dir::open_path(&root.path(DIR)).map_err(|e| format!("{DIR}: {e}"))?;
        check_private(&dir.meta().map_err(|e| e.to_string())?, me, DIR)?;
        let name = OsStr::new(CONF_NAME);
        let Some(meta) = dir.stat_opt(name).map_err(|e| e.to_string())? else {
            return Ok((
                Self::default(),
                vec![format!("no {CONF_NAME}: every part is off")],
            ));
        };
        if !meta.is_file() || meta.uid != me || meta.shared_write() {
            return Err(format!(
                "{CONF_NAME} is not a plain file of root's alone ({:04o}, uid {})",
                meta.perm(),
                meta.uid
            ));
        }
        let mut file = dir
            .open_read(name)
            .map_err(|e| format!("{CONF_NAME}: {e}"))?;
        let (bytes, over) = read_capped(&mut file, MAX_BYTES).map_err(|e| e.to_string())?;
        if over {
            return Err(format!("{CONF_NAME} is over {MAX_BYTES} bytes"));
        }
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// One line per switch, for the log and `lumend check`.
    pub fn describe(&self) -> String {
        let mut words: Vec<String> = Part::ALL
            .iter()
            .map(|p| format!("{}={}", p.name(), if self.on(*p) { "on" } else { "off" }))
            .collect();
        for s in SENDERS {
            let on = self.senders.contains(s.name);
            words.push(format!(
                "sender:{}={}",
                s.name,
                if on { "on" } else { "off" }
            ));
        }
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_by_default() {
        let (conf, warnings) = Conf::parse("");
        assert!(warnings.is_empty());
        assert!(Part::ALL.iter().all(|p| !conf.on(*p)));
        assert!(conf.senders.is_empty());
    }

    #[test]
    fn reads_switches_and_reports_the_rest() {
        let text = "# lumend\n\ns1 on\nlg-home on\nlg-home off\nicons   on  \nsender acr2 on\n\
                    sender iconnectivity on\nshell maybe\nfoo on\nsender\nscreen-off on extra\n";
        let (conf, warnings) = Conf::parse(text);
        assert!(conf.on(Part::S1));
        assert!(!conf.on(Part::LgHome));
        assert!(conf.on(Part::Icons));
        assert!(!conf.on(Part::Shell));
        assert!(!conf.on(Part::ScreenOff));
        assert_eq!(conf.senders.iter().copied().collect::<Vec<_>>(), ["acr2"]);
        assert_eq!(warnings.len(), 5, "{warnings:?}");
        assert!(warnings[0].contains("iconnectivity"));
    }

    #[test]
    fn loads_only_roots_own_file() {
        use std::os::unix::fs::PermissionsExt;
        let base = crate::fsat::scratch("conf");
        let root = Root::at(&base);
        let dir = root.path(DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (conf, notes) = Conf::load(&root).unwrap();
        assert_eq!(conf, Conf::default());
        assert!(notes[0].contains("every part is off"));

        std::fs::write(dir.join(CONF_NAME), "modes on\n").unwrap();
        std::fs::set_permissions(dir.join(CONF_NAME), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        assert!(Conf::load(&root).unwrap().0.on(Part::Modes));

        std::fs::set_permissions(dir.join(CONF_NAME), std::fs::Permissions::from_mode(0o666))
            .unwrap();
        assert!(Conf::load(&root).is_err());

        std::fs::remove_file(dir.join(CONF_NAME)).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", dir.join(CONF_NAME)).unwrap();
        assert!(Conf::load(&root).is_err());

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Conf::load(&root).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
