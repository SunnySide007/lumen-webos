//! The two programs lumend starts, `initctl` and `iptables` (S1), and how.
//!
//! * Each is found once, in `/sbin`, `/usr/sbin`, `/bin` or `/usr/bin`, and
//!   used only if it and every folder above it (through any link, to the
//!   file itself) are root's and writable by no one else: a file only root
//!   can change.
//! * It runs with fixed arguments, without a shell, with stdin from
//!   /dev/null, an environment of only `PATH`, and a deadline; its output is
//!   read on threads of its own and capped. As lumen-ctl's `bus::spawn`.
//! * This is the one place in the crate that starts a program (clippy
//!   refuses `Command::new` elsewhere).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::fsat::Meta;

/// How long one run may take.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: usize = 64 << 10;
const CHILD_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";
const WAIT_TICK: Duration = Duration::from_millis(5);
const SEARCH: [&str; 4] = ["/sbin", "/usr/sbin", "/bin", "/usr/bin"];

/// The programs lumend may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Initctl,
    Iptables,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Self::Initctl => "initctl",
            Self::Iptables => "iptables",
        }
    }
}

/// What a finished program left.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Starts programs; the tests give a fake.
pub trait Runner: Send + Sync {
    fn run(&self, program: Program, args: &[&str]) -> Result<Output, String>;
}

/// Whether a file or folder only root can change: root's, and not
/// writable by its group or others.
fn roots_alone(meta: &Meta) -> bool {
    meta.uid == 0 && !meta.shared_write()
}

/// Every folder from `/` down to `path`'s parent, and `path` itself, by
/// `lstat`: all root's alone, none a link.
fn chain_ok(path: &Path) -> bool {
    let mut at = PathBuf::from("/");
    for part in path.components().skip(1) {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if !m.file_type().is_symlink() && roots_alone(&meta_of(&m)) => {}
            _ => return false,
        }
    }
    true
}

fn meta_of(m: &std::fs::Metadata) -> Meta {
    use std::os::unix::fs::MetadataExt;
    Meta {
        mode: m.mode(),
        uid: m.uid(),
        gid: m.gid(),
        nlink: m.nlink(),
        dev: m.dev(),
        ino: m.ino(),
        size: m.size(),
        mtime: m.mtime(),
        mtime_nsec: m.mtime_nsec(),
    }
}

/// Where `name` is, checked as the header says: the path to run (its own
/// name kept, since multi-call binaries read it) and the file it leads to.
pub fn find(name: &str) -> Result<(PathBuf, PathBuf), String> {
    for dir in SEARCH {
        let path = Path::new(dir).join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let real = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file_ok = std::fs::metadata(&real)
            .map(|m| m.is_file() && meta_of(&m).mode & 0o111 != 0)
            .unwrap_or(false);
        // The folder the name is in, whatever it leads to, and the file.
        let dir_real = std::fs::canonicalize(dir).unwrap_or_default();
        if file_ok && chain_ok(&real) && chain_ok(&dir_real) {
            return Ok((path, real));
        }
        return Err(format!(
            "{} is not a program only root can change",
            path.display()
        ));
    }
    Err(format!("no {name} in {}", SEARCH.join(", ")))
}

/// The real thing: each program found once, then `spawn`.
pub struct Spawn;

impl Runner for Spawn {
    fn run(&self, program: Program, args: &[&str]) -> Result<Output, String> {
        static INITCTL: OnceLock<Result<PathBuf, String>> = OnceLock::new();
        static IPTABLES: OnceLock<Result<PathBuf, String>> = OnceLock::new();
        let cell = match program {
            Program::Initctl => &INITCTL,
            Program::Iptables => &IPTABLES,
        };
        let path = cell
            .get_or_init(|| find(program.name()).map(|(path, _)| path))
            .clone()?;
        spawn(&path, args, RUN_TIMEOUT)
    }
}

fn read_capped(mut pipe: impl Read, max: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut buf = [0u8; 4096];
    let mut over = false;
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
                over |= n > room;
            }
        }
    }
    (kept, over)
}

/// Runs `program` with `args` as the header says, and kills it at
/// `timeout`.
pub fn spawn(program: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    if !program.is_absolute() {
        return Err(format!("{} is not an absolute path", program.display()));
    }
    #[allow(clippy::disallowed_methods)] // the crate's one place to start a program
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        thread::Builder::new()
            .name("pipe".to_owned())
            .stack_size(64 << 10)
            .spawn(move || pipe.map_or((Vec::new(), false), |p| read_capped(p, MAX_OUTPUT)))
    };
    let out = reader(child.stdout.take().map(|p| Box::new(p) as _));
    let err = reader(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "{} gave no answer in {} ms",
                    program.display(),
                    timeout.as_millis()
                ));
            }
            Ok(None) => thread::sleep(WAIT_TICK),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("{}: {e}", program.display()));
            }
        }
    };
    // A program killed at its deadline may have left a child that holds its
    // pipes open: its readers are left to end with the pipes, not waited on.
    let status = status?;
    let join = |handle: std::io::Result<thread::JoinHandle<(Vec<u8>, bool)>>| {
        handle.ok().and_then(|h| h.join().ok()).unwrap_or_default()
    };
    let (stdout, over) = join(out);
    let (stderr, _) = join(err);
    if over {
        return Err(format!("{} wrote too much", program.display()));
    }
    Ok(Output {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Starts `program` (lumend itself, for `lumend start`) as the leader of a
/// session of its own, as the header says but with no deadline: stdin and
/// stderr from and to /dev/null, stdout piped back for its `ready` line.
pub fn start_daemon(program: &Path, args: &[&str]) -> Result<std::process::Child, String> {
    if !program.is_absolute() {
        return Err(format!("{} is not an absolute path", program.display()));
    }
    #[allow(clippy::disallowed_methods)] // the crate's one place to start a program
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::sys::os::new_session(&mut command);
    command
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))
}

/// A runner that answers from a script and records what it was asked.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeRunner {
        /// Answers by "program arg arg…" prefix, first match wins.
        pub answers: Mutex<Vec<(String, Output)>>,
        pub log: Mutex<Vec<String>>,
    }

    impl FakeRunner {
        pub fn answer(&self, prefix: &str, success: bool, stdout: &str) {
            self.answers.lock().unwrap().push((
                prefix.to_owned(),
                Output {
                    success,
                    stdout: stdout.to_owned(),
                    stderr: String::new(),
                },
            ));
        }

        pub fn ran(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: Program, args: &[&str]) -> Result<Output, String> {
            let line = format!("{} {}", program.name(), args.join(" "));
            self.log.lock().unwrap().push(line.clone());
            let answers = self.answers.lock().unwrap();
            answers
                .iter()
                .find(|(prefix, _)| line.starts_with(prefix.as_str()))
                .map(|(_, out)| out.clone())
                .ok_or_else(|| format!("no fake answer for {line}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_a_program_with_a_clean_environment() {
        let out = spawn(
            Path::new("/bin/sh"),
            &["-c", "echo $PATH; echo $HOME"],
            RUN_TIMEOUT,
        )
        .unwrap();
        assert!(out.success);
        assert_eq!(out.stdout, format!("{CHILD_PATH}\n\n"));
    }

    #[test]
    fn kills_a_program_at_its_deadline() {
        let started = Instant::now();
        let err = spawn(
            Path::new("/bin/sh"),
            &["-c", "sleep 5"],
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(err.contains("no answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn refuses_a_relative_program() {
        assert!(spawn(Path::new("sh"), &[], RUN_TIMEOUT).is_err());
    }

    #[test]
    fn finds_a_system_program_and_names_a_missing_one() {
        // sh is root's on any system the tests run on.
        if crate::sys::os::euid() == 0 || Path::new("/bin/sh").exists() {
            assert!(find("sh").is_ok() || find("sh").unwrap_err().contains("only root"));
        }
        assert!(find("no-such-program-here").unwrap_err().starts_with("no "));
    }
}
