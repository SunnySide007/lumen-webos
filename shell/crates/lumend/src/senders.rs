//! S2, redesigned for what the survey of 2026-11-06 found (b9.md,
//! "Platform"): LG's ACR program is no setting to switch off. It is
//! `acr2`, a dynamic luna service: the hub starts `/usr/sbin/acr2` whenever
//! anything calls `com.webos.service.acr`, and acr2 then asks for the
//! device's ids, the network and the settings. So it is held down as a
//! program started on demand:
//!
//! 1. **A stand-in in its place.** `lumen-standin` (src/bin) is bind-mounted
//!    over the program, read-only, in RAM: gone at the next boot, nothing
//!    written to a system partition (docs/safety.md, rule 1). When the hub
//!    starts the program, the stand-in runs instead. It registers the
//!    service's name (the hub allows it: it runs from the program's path,
//!    which acr2's own role names) and offers no method, so every call gets
//!    luna's "unknown method" at once and no caller waits on a timeout. It
//!    calls nothing, opens nothing and writes nothing.
//! 2. **LG's copy stopped** if one runs: any process whose executable is at
//!    the program's path but is not the stand-in gets SIGTERM, then SIGKILL
//!    two seconds later.
//! 3. **A watch, not a poll:** lumend asks the hub to say when the service
//!    connects (`com.palm.bus/signal/registerServerStatus`, which calls the
//!    hub, never the service) and checks then that the stand-in, not LG's
//!    program, answered, mounting it again if the mount went.
//!
//! Each sender is its own switch (`sender <name> on` in lumend.conf). Only
//! acr2 is listed: the survey read its service file. UEI's `iconnectivity`
//! (decision 11), `admanager`, `statisticsmanager` and the other on-demand
//! services privacy.md names each need a read-only look at how they start
//! before they join the list (the owner's call; see the lane's report).

use std::io;
use std::path::{Path, PathBuf};

use crate::fsat::Meta;
use crate::paths::STANDIN;

/// One of LG's programs held down by the stand-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sender {
    /// Its switch in lumend.conf.
    pub name: &'static str,
    /// The program the hub starts.
    pub program: &'static str,
    /// The bus name the program registers, which the stand-in takes.
    pub service: &'static str,
}

pub const SENDERS: &[Sender] = &[Sender {
    name: "acr2",
    program: "/usr/sbin/acr2",
    service: "com.webos.service.acr",
}];

/// The sender whose program is at `path`: how the stand-in knows which name
/// to take.
pub fn by_program(path: &Path) -> Option<&'static Sender> {
    SENDERS.iter().find(|s| Path::new(s.program) == path)
}

/// One running process, as `/proc` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    /// Where its executable is (`/proc/<pid>/exe`'s link).
    pub exe: PathBuf,
    /// The executable's device and inode.
    pub id: (u64, u64),
}

/// What holding a sender down touches; the tests give a fake.
pub trait System: Send + Sync {
    /// `lstat`.
    fn stat(&self, path: &Path) -> io::Result<Meta>;
    fn bind(&self, source: &Path, target: &Path) -> io::Result<()>;
    fn remount_read_only(&self, target: &Path) -> io::Result<()>;
    fn unmount(&self, target: &Path, detach: bool) -> io::Result<()>;
    fn processes(&self) -> Vec<Proc>;
    fn kill(&self, pid: u32, hard: bool) -> io::Result<()>;
}

/// The TV itself.
pub struct Tv;

impl System for Tv {
    fn stat(&self, path: &Path) -> io::Result<Meta> {
        let parent = path.parent().ok_or_else(|| io::Error::other("no parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("no name"))?;
        crate::fsat::Dir::open_path(parent)?.stat(name)
    }

    fn bind(&self, source: &Path, target: &Path) -> io::Result<()> {
        crate::sys::os::bind_mount(source, target)
    }

    fn remount_read_only(&self, target: &Path) -> io::Result<()> {
        crate::sys::os::remount_read_only(target)
    }

    fn unmount(&self, target: &Path, detach: bool) -> io::Result<()> {
        crate::sys::os::unmount(target, detach)
    }

    fn processes(&self) -> Vec<Proc> {
        use std::os::unix::fs::MetadataExt;
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let me = std::process::id();
        dir.filter_map(|e| {
            let name = e.ok()?.file_name();
            let pid: u32 = name.to_str()?.parse().ok()?;
            if pid == me {
                return None;
            }
            let link = format!("/proc/{pid}/exe");
            let exe = std::fs::read_link(&link).ok()?;
            let meta = std::fs::metadata(&link).ok()?;
            Some(Proc {
                pid,
                exe,
                id: (meta.dev(), meta.ino()),
            })
        })
        .collect()
    }

    fn kill(&self, pid: u32, hard: bool) -> io::Result<()> {
        crate::sys::os::kill(pid, hard)
    }
}

/// The stand-in's file, checked: root's, executable, writable by no one
/// else.
pub fn standin(sys: &dyn System) -> Result<Meta, String> {
    let m = sys
        .stat(Path::new(STANDIN))
        .map_err(|e| format!("{STANDIN}: {e}"))?;
    if !m.is_file() || m.uid != 0 || m.shared_write() || m.mode & 0o100 == 0 {
        return Err(format!("{STANDIN} is not root's own program"));
    }
    Ok(m)
}

/// LG's copies: at the program's path, but not the stand-in.
fn originals(sender: &Sender, sys: &dyn System, standin: (u64, u64)) -> Vec<u32> {
    sys.processes()
        .into_iter()
        .filter(|p| p.exe == Path::new(sender.program) && p.id != standin)
        .map(|p| p.pid)
        .collect()
}

/// The stand-in mounted over the program, and LG's running copies sent
/// SIGTERM. Returns what was done, a line each.
pub fn hold(sender: &Sender, sys: &dyn System) -> Result<Vec<String>, String> {
    let s = standin(sys)?;
    let program = Path::new(sender.program);
    let target = sys
        .stat(program)
        .map_err(|e| format!("{}: {e}", sender.program))?;
    let mut lines = Vec::new();
    if target.id() == s.id() {
        lines.push(format!("{}: the stand-in is in place already", sender.name));
    } else {
        if !target.is_file() || target.uid != 0 {
            return Err(format!(
                "{} is not a root-owned program; left alone",
                sender.program
            ));
        }
        sys.bind(Path::new(STANDIN), program)
            .map_err(|e| format!("mounting the stand-in over {}: {e}", sender.program))?;
        if let Err(e) = sys.remount_read_only(program) {
            lines.push(format!("{}: the mount stays writable: {e}", sender.name));
        }
        let now = sys.stat(program).map(|m| m.id());
        if now.as_ref().ok() != Some(&s.id()) {
            let _ = sys.unmount(program, true);
            return Err(format!(
                "{}: the stand-in didn't show at {} after the mount; taken away",
                sender.name, sender.program
            ));
        }
        lines.push(format!(
            "{}: the stand-in is mounted over {}",
            sender.name, sender.program
        ));
    }
    lines.extend(stop_originals(sender, sys, false)?);
    Ok(lines)
}

/// SIGTERM (or, `hard`, SIGKILL) to LG's running copies.
pub fn stop_originals(
    sender: &Sender,
    sys: &dyn System,
    hard: bool,
) -> Result<Vec<String>, String> {
    let s = standin(sys)?;
    let signal = if hard { "SIGKILL" } else { "SIGTERM" };
    Ok(originals(sender, sys, s.id())
        .into_iter()
        .map(|pid| match sys.kill(pid, hard) {
            Ok(()) => format!("{}: {signal} to LG's copy, pid {pid}", sender.name),
            Err(e) => format!("{}: FAILED to signal pid {pid}: {e}", sender.name),
        })
        .collect())
}

/// Whether the stand-in still shows at the program's path.
pub fn held(sender: &Sender, sys: &dyn System) -> bool {
    match (standin(sys), sys.stat(Path::new(sender.program))) {
        (Ok(s), Ok(t)) => s.id() == t.id(),
        _ => false,
    }
}

/// The undo: the stand-in's processes for this program stopped, the mount
/// taken away, and LG's program checked to show again.
pub fn release(sender: &Sender, sys: &dyn System) -> Vec<String> {
    let mut lines = Vec::new();
    let program = Path::new(sender.program);
    let standin_id = standin(sys).map(|m| m.id()).ok();
    if let Some(id) = standin_id {
        for p in sys.processes() {
            if p.exe == program && p.id == id {
                let _ = sys.kill(p.pid, true);
                lines.push(format!(
                    "{}: stopped the stand-in, pid {}",
                    sender.name, p.pid
                ));
            }
        }
    }
    let mut n = 0;
    while n < 4 && standin_id.is_some() && held(sender, sys) {
        match sys
            .unmount(program, false)
            .or_else(|_| sys.unmount(program, true))
        {
            Ok(()) => n += 1,
            Err(e) => {
                lines.push(format!("{}: FAILED to unmount: {e}", sender.name));
                break;
            }
        }
    }
    lines.push(match (n, held(sender, sys)) {
        (_, true) => format!("{}: the stand-in is STILL mounted", sender.name),
        (0, false) => format!("{}: no stand-in was mounted", sender.name),
        (_, false) => format!("{}: unmounted; LG's program shows again", sender.name),
    });
    lines
}

/// A system in memory: files by path, mounts as a map, processes listed.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct FakeSystem {
        pub files: Mutex<HashMap<PathBuf, Meta>>,
        /// target → the file it showed before the mount.
        pub mounts: Mutex<HashMap<PathBuf, Meta>>,
        pub procs: Mutex<Vec<Proc>>,
        pub killed: Mutex<Vec<(u32, bool)>>,
    }

    pub fn file(ino: u64, mode: u32) -> Meta {
        Meta {
            mode: 0o100_000 | mode,
            uid: 0,
            gid: 0,
            nlink: 1,
            dev: 1,
            ino,
            size: 1,
            mtime: 0,
            mtime_nsec: 0,
        }
    }

    impl FakeSystem {
        pub fn new() -> Self {
            let mut files = HashMap::new();
            files.insert(PathBuf::from(STANDIN), file(100, 0o700));
            files.insert(PathBuf::from("/usr/sbin/acr2"), file(7, 0o755));
            Self {
                files: Mutex::new(files),
                mounts: Mutex::new(HashMap::new()),
                procs: Mutex::new(Vec::new()),
                killed: Mutex::new(Vec::new()),
            }
        }
    }

    impl Default for FakeSystem {
        fn default() -> Self {
            Self::new()
        }
    }

    impl System for FakeSystem {
        fn stat(&self, path: &Path) -> io::Result<Meta> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .copied()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }

        fn bind(&self, source: &Path, target: &Path) -> io::Result<()> {
            let src = self.stat(source)?;
            let before = self.stat(target)?;
            self.mounts
                .lock()
                .unwrap()
                .entry(target.to_path_buf())
                .or_insert(before);
            self.files.lock().unwrap().insert(target.to_path_buf(), src);
            Ok(())
        }

        fn remount_read_only(&self, _: &Path) -> io::Result<()> {
            Ok(())
        }

        fn unmount(&self, target: &Path, _: bool) -> io::Result<()> {
            let before = self
                .mounts
                .lock()
                .unwrap()
                .remove(target)
                .ok_or_else(|| io::Error::other("not mounted"))?;
            self.files
                .lock()
                .unwrap()
                .insert(target.to_path_buf(), before);
            Ok(())
        }

        fn processes(&self) -> Vec<Proc> {
            self.procs.lock().unwrap().clone()
        }

        fn kill(&self, pid: u32, hard: bool) -> io::Result<()> {
            self.killed.lock().unwrap().push((pid, hard));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn acr2() -> &'static Sender {
        by_program(Path::new("/usr/sbin/acr2")).unwrap()
    }

    #[test]
    fn mounts_the_stand_in_and_stops_lgs_copy_only() {
        let sys = FakeSystem::new();
        sys.procs.lock().unwrap().extend([
            Proc {
                pid: 900,
                exe: PathBuf::from("/usr/sbin/acr2"),
                id: (1, 7),
            },
            Proc {
                pid: 901,
                exe: PathBuf::from("/usr/sbin/acr2"),
                id: (1, 100),
            },
            Proc {
                pid: 902,
                exe: PathBuf::from("/usr/sbin/other"),
                id: (1, 7),
            },
        ]);
        let lines = hold(acr2(), &sys).unwrap();
        assert!(
            lines[0].contains("mounted over /usr/sbin/acr2"),
            "{lines:?}"
        );
        assert!(held(acr2(), &sys));
        assert_eq!(*sys.killed.lock().unwrap(), [(900, false)]);
        // Twice is fine: the mount is seen and kept.
        let again = hold(acr2(), &sys).unwrap();
        assert!(again[0].contains("in place already"));
        assert_eq!(sys.mounts.lock().unwrap().len(), 1);
    }

    #[test]
    fn release_stops_the_stand_in_and_shows_lgs_program_again() {
        let sys = FakeSystem::new();
        hold(acr2(), &sys).unwrap();
        sys.procs.lock().unwrap().push(Proc {
            pid: 950,
            exe: PathBuf::from("/usr/sbin/acr2"),
            id: (1, 100),
        });
        let lines = release(acr2(), &sys);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("stopped the stand-in, pid 950"))
        );
        assert!(lines.last().unwrap().contains("shows again"));
        assert!(!held(acr2(), &sys));
        assert_eq!(sys.stat(Path::new("/usr/sbin/acr2")).unwrap().ino, 7);
        assert!(
            release(acr2(), &sys)
                .last()
                .unwrap()
                .contains("no stand-in")
        );
    }

    #[test]
    fn refuses_a_stand_in_others_could_change() {
        let sys = FakeSystem::new();
        sys.files
            .lock()
            .unwrap()
            .insert(PathBuf::from(STANDIN), file(100, 0o777));
        assert!(hold(acr2(), &sys).is_err());
        assert!(sys.mounts.lock().unwrap().is_empty());
    }

    #[test]
    fn leaves_a_program_that_isnt_roots_alone() {
        let sys = FakeSystem::new();
        let mut m = file(7, 0o755);
        m.uid = 5000;
        sys.files
            .lock()
            .unwrap()
            .insert(PathBuf::from("/usr/sbin/acr2"), m);
        assert!(hold(acr2(), &sys).unwrap_err().contains("left alone"));
    }

    #[test]
    fn every_sender_has_an_absolute_program_and_a_service() {
        for s in SENDERS {
            assert!(s.program.starts_with('/'));
            assert!(s.service.contains('.'));
            assert_eq!(by_program(Path::new(s.program)), Some(s));
        }
        assert_eq!(by_program(Path::new("/usr/sbin/sam")), None);
    }
}
