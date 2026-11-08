//! The four files through which only the owner changes a running Guard
//! (decision 12), read again whenever they change, within a second:
//!
//! * the TLS file (`--tls-file`, stage G3): `tls` sends what Guard forwards
//!   to the TLS server; `tls <boot id>` does until the next boot only;
//!   `clear` sends it to the router as G1 and G2 did. With no file, `--tls`.
//!
//! * the mode file (`--mode-file`): `block` blocks; `block <boot id>`
//!   blocks until the next boot only, for a trial that a reboot undoes;
//!   `log` only logs. With no file, `--mode` says.
//! * the owner's rules (`--owner`): rules.rs's owner syntax.
//! * the learning window (`--learn`): `<start> <end>`, unix seconds by the
//!   TV's clock, at most an hour apart. While the clock is inside it, names
//!   no rule covers resolve and are logged as `learn`.
//!
//! Each counts only when it is a regular file owned by Guard's own user and
//! writable by no one else, so nothing but root (the owner's SSH key, through
//! `tools/guard.sh`) can loosen the TV. A file that fails is left out, and
//! Guard says so once.

use std::collections::BTreeSet;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::rules::{self, Rules, Table};
use crate::sys;

/// The longest learning window: decision 11's examples are 15 minutes.
pub const MAX_WINDOW: u64 = 60 * 60;
/// New names one window keeps; past it, names are blocked as without one.
pub const MAX_LEARNED: usize = 512;
/// The largest control file read; the owner's rules are a few hundred bytes.
const MAX_FILE: u64 = 256 << 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Log,
    Block,
}

impl Mode {
    pub fn word(self) -> &'static str {
        match self {
            Mode::Log => "log",
            Mode::Block => "block",
        }
    }
}

/// What a file looked like when last read, to notice a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime: i64,
    mtime_nsec: i64,
    ino: u64,
    mode: u32,
    uid: u32,
}

impl Stamp {
    fn of(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            mtime: meta.mtime(),
            mtime_nsec: meta.mtime_nsec(),
            ino: meta.ino(),
            mode: meta.mode(),
            uid: meta.uid(),
        }
    }
}

/// A control file's contents if it may be trusted, else why it doesn't count.
fn read_trusted(path: &Path, meta: &Metadata) -> Result<String, String> {
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let euid = sys::euid();
    if meta.uid() != euid {
        return Err(format!(
            "{} is owned by uid {}, not {euid}",
            path.display(),
            meta.uid()
        ));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{} is writable by others ({:o})",
            path.display(),
            meta.mode() & 0o777
        ));
    }
    if meta.len() > MAX_FILE {
        return Err(format!("{} is over {MAX_FILE} bytes", path.display()));
    }
    fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))
}

/// One watched file: the last stamp seen, so it is read only on a change.
#[derive(Debug, Default)]
struct Watched {
    seen: Option<Option<Stamp>>,
}

enum Change {
    Same,
    Gone,
    Now(Result<String, String>),
}

impl Watched {
    fn poll(&mut self, path: &Path) -> Change {
        // symlink_metadata: a link is not a regular file, so it never counts.
        let meta = fs::symlink_metadata(path).ok();
        let stamp = meta.as_ref().map(Stamp::of);
        if self.seen == Some(stamp) {
            return Change::Same;
        }
        self.seen = Some(stamp);
        match meta {
            None => Change::Gone,
            Some(meta) => Change::Now(read_trusted(path, &meta)),
        }
    }
}

/// A switch file: `on`, `on <boot id>` (this boot only, so a reboot ends a
/// trial), or `off`, in the words given; a trial for another boot is `off`.
fn parse_switch<T: Copy>(
    text: &str,
    boot_id: Option<&str>,
    (on_word, on): (&str, T),
    (off_word, off): (&str, T),
) -> Result<T, String> {
    let mut words = text.split_whitespace();
    let value = match words.next() {
        Some(w) if w == on_word => on,
        Some(w) if w == off_word => off,
        Some(other) => return Err(format!("unknown word {other:?}")),
        None => return Err("empty".to_owned()),
    };
    match (words.next(), words.next()) {
        (None, _) => Ok(value),
        (Some(id), None) if Some(id) == boot_id => Ok(value),
        // For another boot: the trial it was written for is over.
        (Some(_), None) => Ok(off),
        (Some(_), Some(_)) => Err("more than a word and a boot id".to_owned()),
    }
}

/// What the mode file says, given this boot's id.
pub fn parse_mode(text: &str, boot_id: Option<&str>) -> Result<Mode, String> {
    parse_switch(text, boot_id, ("block", Mode::Block), ("log", Mode::Log))
}

/// What the TLS file says (G3): `tls`, `tls <boot id>` or `clear`.
pub fn parse_tls(text: &str, boot_id: Option<&str>) -> Result<bool, String> {
    parse_switch(text, boot_id, ("tls", true), ("clear", false))
}

/// A learning window, `<start> <end>`, at most `MAX_WINDOW` long.
pub fn parse_window(text: &str) -> Result<(u64, u64), String> {
    let mut words = text.split_whitespace();
    let (Some(start), Some(end), None) = (words.next(), words.next(), words.next()) else {
        return Err("not \"<start> <end>\"".to_owned());
    };
    let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) else {
        return Err("not two numbers".to_owned());
    };
    if end <= start || end - start > MAX_WINDOW {
        return Err(format!("not a window of 1 s to {MAX_WINDOW} s"));
    }
    Ok((start, end))
}

/// The owner's controls as last read, and the names the open window learned.
#[derive(Debug)]
pub struct Control {
    /// From `--mode`, when there is no mode file.
    default_mode: Mode,
    /// The mode the files ask for.
    wanted: Mode,
    /// Set when blocking was asked for but the rules would block a name
    /// Guard must resolve: Guard keeps to logging.
    pub refused: Option<String>,
    pub owner: Table,
    window: Option<(u64, u64)>,
    pub learned: BTreeSet<String>,
    /// Whether the window was open at the last poll, to say when it closes.
    was_learning: bool,
    boot_id: Option<String>,
    watch_mode: Watched,
    watch_owner: Watched,
    watch_learn: Watched,
    /// From `--tls`, when there is no TLS file.
    default_tls: bool,
    /// Whether lookups go to the TLS server (G3), as the files ask.
    tls: bool,
    watch_tls: Watched,
}

impl Control {
    pub fn new(default_mode: Mode, default_tls: bool) -> Self {
        Self {
            default_tls,
            tls: default_tls,
            watch_tls: Watched::default(),
            default_mode,
            wanted: default_mode,
            refused: None,
            owner: Table::default(),
            window: None,
            learned: BTreeSet::new(),
            was_learning: false,
            boot_id: sys::boot_id(),
            watch_mode: Watched::default(),
            watch_owner: Watched::default(),
            watch_learn: Watched::default(),
        }
    }

    /// The mode Guard runs in: blocking only when asked for and safe.
    pub fn mode(&self) -> Mode {
        if self.refused.is_some() {
            Mode::Log
        } else {
            self.wanted
        }
    }

    /// Whether the owner's switch sends lookups over TLS.
    pub fn tls(&self) -> bool {
        self.tls
    }

    /// Reads the TLS file again if it changed; returns what to say.
    pub fn poll_tls(&mut self, path: &Path) -> Option<String> {
        let (tls, why) = match self.watch_tls.poll(path) {
            Change::Same => return None,
            Change::Gone => (self.default_tls, None),
            Change::Now(Ok(text)) => match parse_tls(&text, self.boot_id.as_deref()) {
                Ok(tls) => (tls, None),
                Err(why) => (self.default_tls, Some(format!("{}: {why}", path.display()))),
            },
            Change::Now(Err(why)) => (self.default_tls, Some(format!("TLS file not read: {why}"))),
        };
        let changed = tls != self.tls;
        self.tls = tls;
        let word = if tls { "tls" } else { "clear" };
        match why {
            Some(why) => Some(format!("{why}; upstream {word}")),
            None => changed.then(|| format!("upstream {word}, as the TLS file says")),
        }
    }

    pub fn learning(&self, now: u64) -> bool {
        self.window
            .is_some_and(|(start, end)| start <= now && now < end)
    }

    /// The window's end while it is open, else 0.
    pub fn learning_until(&self, now: u64) -> u64 {
        match self.window {
            Some((_, end)) if self.learning(now) => end,
            _ => 0,
        }
    }

    /// Whether `name`, which no rule covers, may be learned now; remembers it.
    pub fn learn(&mut self, name: &str, now: u64) -> bool {
        if !self.learning(now) {
            return false;
        }
        if self.learned.contains(name) {
            return true;
        }
        if self.learned.len() >= MAX_LEARNED {
            return false;
        }
        self.learned.insert(name.to_owned());
        true
    }

    /// Reads whichever files changed; returns what Guard should say about it.
    pub fn poll(
        &mut self,
        rules: &Rules,
        mode_file: Option<&Path>,
        owner_file: Option<&Path>,
        learn_file: Option<&Path>,
        now: u64,
    ) -> Vec<String> {
        let mut said = Vec::new();
        let mut recheck = false;
        let before = (self.mode(), self.refused.clone());
        if let Some(path) = owner_file {
            match self.watch_owner.poll(path) {
                Change::Same => {}
                Change::Gone => {
                    if self.owner.len() > 0 {
                        said.push(format!("{} is gone: no owner rules", path.display()));
                    }
                    self.owner = Table::default();
                    recheck = true;
                }
                Change::Now(Err(why)) => {
                    said.push(format!(
                        "owner rules not read: {why}; the earlier ones stay"
                    ));
                }
                Change::Now(Ok(text)) => {
                    let owner = rules::parse_owner(&text);
                    match rules.unsafe_for(&owner) {
                        Some(name) => said.push(format!(
                            "owner rules refused: with them Guard would block {name}; the earlier ones stay"
                        )),
                        None => {
                            said.push(format!("owner rules: {} from {}", owner.len(), path.display()));
                            self.owner = owner;
                            recheck = true;
                        }
                    }
                }
            }
        }
        if let Some(path) = mode_file {
            let wanted = match self.watch_mode.poll(path) {
                Change::Same => None,
                Change::Gone => Some(self.default_mode),
                Change::Now(Ok(text)) => match parse_mode(&text, self.boot_id.as_deref()) {
                    Ok(mode) => Some(mode),
                    Err(why) => {
                        said.push(format!(
                            "{}: {why}; mode {}",
                            path.display(),
                            self.default_mode.word()
                        ));
                        Some(self.default_mode)
                    }
                },
                Change::Now(Err(why)) => {
                    said.push(format!(
                        "mode file not read: {why}; mode {}",
                        self.default_mode.word()
                    ));
                    Some(self.default_mode)
                }
            };
            if let Some(wanted) = wanted {
                recheck |= wanted != self.wanted;
                self.wanted = wanted;
            }
        }
        if recheck || self.refused.is_some() {
            self.refused = match self.wanted {
                Mode::Block => rules.unsafe_for(&self.owner).map(str::to_owned),
                Mode::Log => None,
            };
            if (self.mode(), self.refused.clone()) != before {
                match &self.refused {
                    Some(name) => said.push(format!(
                        "blocking refused: the rules don't allow {name}; logging only"
                    )),
                    None => said.push(format!("mode {}", self.mode().word())),
                }
            }
        }
        if let Some(path) = learn_file {
            match self.watch_learn.poll(path) {
                Change::Same => {}
                Change::Gone => self.window = None,
                Change::Now(Ok(text)) => match parse_window(&text) {
                    Ok(window) => {
                        if self.window != Some(window) {
                            self.learned.clear();
                        }
                        self.window = Some(window);
                    }
                    Err(why) => {
                        said.push(format!("{}: {why}; no learning window", path.display()));
                        self.window = None;
                    }
                },
                Change::Now(Err(why)) => {
                    said.push(format!("learning window not read: {why}"));
                    self.window = None;
                }
            }
        }
        let learning = self.learning(now);
        if learning && !self.was_learning {
            said.push(format!(
                "learning window open until {}",
                self.learning_until(now)
            ));
        } else if !learning && self.was_learning {
            said.push(format!(
                "learning window closed: {} new names learned",
                self.learned.len()
            ));
        }
        self.was_learning = learning;
        said
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lumen-guard-control-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn store_rules() -> Rules {
        Rules::parse(
            "lgtvsdp.com\nlgeapi.com\nnetflix.com\namazon.com\nplex.tv\ngoogleapis.com\n\
             youtube.com\nspotify.com\nlumen.invalid\n",
            "",
            "snu.lge.com\n",
            &[],
        )
    }

    #[test]
    fn modes_and_boot_ids() {
        let id = Some("abc");
        assert_eq!(parse_mode("block\n", id), Ok(Mode::Block));
        assert_eq!(parse_mode("  log ", id), Ok(Mode::Log));
        assert_eq!(parse_mode("block abc\n", id), Ok(Mode::Block));
        // A trial for an earlier boot is over.
        assert_eq!(parse_mode("block old\n", id), Ok(Mode::Log));
        assert_eq!(parse_mode("block abc\n", None), Ok(Mode::Log));
        assert!(parse_mode("loud", id).is_err());
        assert!(parse_mode("", id).is_err());
        assert!(parse_mode("block abc extra", id).is_err());
    }

    #[test]
    fn the_tls_switch_and_boot_ids() {
        let id = Some("abc");
        assert_eq!(parse_tls("tls\n", id), Ok(true));
        assert_eq!(parse_tls("clear", id), Ok(false));
        assert_eq!(parse_tls("tls abc", id), Ok(true));
        assert_eq!(parse_tls("tls old", id), Ok(false));
        assert!(parse_tls("block", id).is_err());
        assert!(parse_tls("", id).is_err());
    }

    #[test]
    fn follows_the_tls_file() {
        let dir = temp_dir("tls");
        let path = dir.join("tls");
        let mut c = Control::new(Mode::Log, false);
        assert_eq!(c.poll_tls(&path), None);
        assert!(!c.tls());
        write(&path, "tls\n");
        assert!(c.poll_tls(&path).unwrap().contains("upstream tls"));
        assert!(c.tls());
        assert_eq!(c.poll_tls(&path), None);
        // Writable by others: it doesn't count, and --tls says.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(c.poll_tls(&path).unwrap().contains("writable by others"));
        assert!(!c.tls());
        write(&path, "tls\n");
        c.poll_tls(&path);
        assert!(c.tls());
        fs::remove_file(&path).unwrap();
        assert!(c.poll_tls(&path).unwrap().contains("upstream clear"));
        assert!(!c.tls());
        // --tls on and no file: on; a file saying clear turns it off.
        let mut c = Control::new(Mode::Log, true);
        assert!(c.tls());
        write(&path, "clear\n");
        c.poll_tls(&path);
        assert!(!c.tls());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows() {
        assert_eq!(parse_window("100 1000\n"), Ok((100, 1000)));
        assert!(parse_window("100 100").is_err());
        assert!(parse_window("100 50").is_err());
        assert!(parse_window(&format!("0 {}", MAX_WINDOW + 1)).is_err());
        assert!(parse_window("a b").is_err());
        assert!(parse_window("1 2 3").is_err());
    }

    #[test]
    fn follows_the_mode_file_and_refuses_unsafe_blocking() {
        let dir = temp_dir("mode");
        let (mode, owner) = (dir.join("mode"), dir.join("owner.txt"));
        let rules = store_rules();
        let mut c = Control::new(Mode::Log, false);
        let poll = |c: &mut Control| c.poll(&rules, Some(&mode), Some(&owner), None, 0);
        assert!(poll(&mut c).is_empty());
        assert_eq!(c.mode(), Mode::Log);
        write(&mode, "block\n");
        let said = poll(&mut c);
        assert_eq!(c.mode(), Mode::Block, "{said:?}");
        // An owner file that would block the store is refused whole.
        write(&owner, "!=us.lgtvsdp.com\n=new.example\n");
        let said = poll(&mut c);
        assert!(said.iter().any(|s| s.contains("refused")), "{said:?}");
        assert_eq!(c.owner.len(), 0);
        assert_eq!(c.mode(), Mode::Block);
        write(&owner, "=new.example\n");
        poll(&mut c);
        assert_eq!(c.owner.len(), 1);
        // Removing the mode file goes back to --mode.
        fs::remove_file(&mode).unwrap();
        poll(&mut c);
        assert_eq!(c.mode(), Mode::Log);
        // Rules that can't keep the store refuse blocking.
        let empty = Rules::parse("", "", "", &[]);
        write(&mode, "block");
        let said = c.poll(&empty, Some(&mode), Some(&owner), None, 0);
        assert_eq!(c.mode(), Mode::Log);
        assert!(c.refused.is_some(), "{said:?}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_others_can_write_do_not_count() {
        let dir = temp_dir("perms");
        let mode = dir.join("mode");
        write(&mode, "block\n");
        fs::set_permissions(&mode, fs::Permissions::from_mode(0o666)).unwrap();
        let rules = store_rules();
        let mut c = Control::new(Mode::Log, false);
        let said = c.poll(&rules, Some(&mode), None, None, 0);
        assert_eq!(c.mode(), Mode::Log);
        assert!(
            said.iter().any(|s| s.contains("writable by others")),
            "{said:?}"
        );
        // A link to a good file is not a regular file.
        let target = dir.join("target");
        write(&target, "block\n");
        fs::remove_file(&mode).unwrap();
        std::os::unix::fs::symlink(&target, &mode).unwrap();
        let said = c.poll(&rules, Some(&mode), None, None, 0);
        assert_eq!(c.mode(), Mode::Log);
        assert!(
            said.iter().any(|s| s.contains("not a regular file")),
            "{said:?}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_window_learns_new_names_until_it_ends() {
        let dir = temp_dir("learn");
        let learn = dir.join("learn");
        let rules = store_rules();
        let mut c = Control::new(Mode::Block, false);
        write(&learn, "1000 1900\n");
        // Before the window (the clock is behind): nothing learned.
        c.poll(&rules, None, None, Some(&learn), 999);
        assert!(!c.learn("a.example", 999));
        let said = c.poll(&rules, None, None, Some(&learn), 1000);
        assert!(
            said.iter().any(|s| s.contains("open until 1900")),
            "{said:?}"
        );
        assert_eq!(c.learning_until(1000), 1900);
        assert!(c.learn("a.example", 1000));
        assert!(c.learn("a.example", 1001));
        assert!(c.learn("b.example", 1500));
        let said = c.poll(&rules, None, None, Some(&learn), 1900);
        assert!(said.iter().any(|s| s.contains("2 new names")), "{said:?}");
        assert!(!c.learn("c.example", 1900));
        assert_eq!(c.learned.len(), 2);
        // A new window starts a new list.
        write(&learn, "2000 2100\n");
        c.poll(&rules, None, None, Some(&learn), 2000);
        assert!(c.learned.is_empty());
        for i in 0..MAX_LEARNED {
            assert!(c.learn(&format!("n{i}.example"), 2001));
        }
        assert!(!c.learn("one-too-many.example", 2001));
        // Removing the file ends the window at once.
        fs::remove_file(&learn).unwrap();
        c.poll(&rules, None, None, Some(&learn), 2002);
        assert!(!c.learning(2002));
        fs::remove_dir_all(&dir).unwrap();
    }
}
