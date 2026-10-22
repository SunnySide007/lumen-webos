//! Minimal logger. Every line goes to stderr and, when it could be opened, to a
//! log file, since a native app launched on the TV has no console to read.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: Mutex<Option<File>> = Mutex::new(None);

/// Opens (appending) the first of `paths` that will open. A log that lands on
/// a fallback starts by saying why the paths before it would not open, since
/// a native app's stderr goes nowhere on the TV. If none opens, stderr is the
/// only output rather than failing the app.
pub fn init(paths: &[PathBuf]) {
    let (opened, reasons) = open_first(paths);
    let file = opened.map(|(mut file, path)| {
        for reason in &reasons {
            let line = stamped(format_args!(
                "log: {reason}; writing to {} instead",
                path.display()
            ));
            let _ = file.write_all(line.as_bytes());
        }
        file
    });
    if file.is_none() {
        for reason in &reasons {
            let line = stamped(format_args!("log: {reason}"));
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }
    if let Ok(mut sink) = SINK.lock() {
        *sink = file;
    }
}

/// Opens the first of `paths` that will open for appending. Also returns why
/// each one before it would not, as `<path>: <error>`.
fn open_first(paths: &[PathBuf]) -> (Option<(File, PathBuf)>, Vec<String>) {
    let mut reasons = Vec::new();
    for path in paths {
        match open_log(path) {
            Ok(file) => return (Some((file, path.clone())), reasons),
            Err(e) => reasons.push(format!("{}: {e}", path.display())),
        }
    }
    (None, reasons)
}

/// Opens `path` to append to, as a regular file only. On the TV `/tmp` is
/// shared with every other app, so on Linux the open cannot block (a FIFO left
/// at the name would hang the shell before its first frame), does not follow
/// a symlink, and gives a new log to its owner alone: it names the apps the
/// owner opens and when.
fn open_log(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "x86",
            target_arch = "x86_64"
        )
    ))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        /// `O_NONBLOCK` on Linux for ARM and x86.
        const O_NONBLOCK: i32 = 0o4000;
        /// `O_NOFOLLOW`, which Linux numbers differently on ARM and x86.
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        const O_NOFOLLOW: i32 = 0o100000;
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        const O_NOFOLLOW: i32 = 0o400000;
        options.custom_flags(O_NONBLOCK | O_NOFOLLOW).mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// One log line: the Unix time in seconds, then the message.
fn stamped(args: std::fmt::Arguments<'_>) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    format!("[{secs:.3}] {args}\n")
}

/// Writes one line, stamped, to stderr and the log file.
pub fn write(args: std::fmt::Arguments<'_>) {
    let line = stamped(args);
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Ok(mut sink) = SINK.lock()
        && let Some(file) = sink.as_mut()
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// `info!("fps {fps:.1}")` writes one timestamped line to stderr and the log file.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, removed when the guard drops.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("lumen-log-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch folder");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_first_path_that_opens_is_used() {
        let dir = Scratch::new("first");
        let paths = [dir.0.join("a.log"), dir.0.join("b.log")];
        let (opened, reasons) = open_first(&paths);
        assert_eq!(opened.map(|(_, path)| path), Some(paths[0].clone()));
        assert!(reasons.is_empty());
    }

    #[test]
    fn a_path_that_cannot_be_opened_is_skipped_and_named() {
        let dir = Scratch::new("skipped");
        // A folder never opens as a file, on Linux or Windows.
        let blocked = dir.0.join("taken");
        std::fs::create_dir(&blocked).expect("folder in the way");
        let paths = [blocked.clone(), dir.0.join("fallback.log")];
        let (opened, reasons) = open_first(&paths);
        assert_eq!(opened.map(|(_, path)| path), Some(paths[1].clone()));
        assert_eq!(reasons.len(), 1);
        assert!(
            reasons[0].starts_with(&format!("{}: ", blocked.display())),
            "{reasons:?}"
        );
    }

    #[test]
    fn when_nothing_opens_every_reason_is_kept() {
        let dir = Scratch::new("none");
        let paths = [dir.0.join("x"), dir.0.join("y")];
        for path in &paths {
            std::fs::create_dir(path).expect("folder in the way");
        }
        let (opened, reasons) = open_first(&paths);
        assert!(opened.is_none());
        assert_eq!(reasons.len(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_fifo_at_the_name_is_skipped_without_hanging() {
        let dir = Scratch::new("fifo");
        let fifo = dir.0.join("lumen-shell.log");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let paths = [fifo, dir.0.join("fallback.log")];
        let (opened, reasons) = open_first(&paths);
        assert_eq!(opened.map(|(_, path)| path), Some(paths[1].clone()));
        assert_eq!(reasons.len(), 1, "{reasons:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_symlink_at_the_name_is_not_followed() {
        let dir = Scratch::new("symlink");
        let elsewhere = dir.0.join("elsewhere");
        let link = dir.0.join("lumen-shell.log");
        std::os::unix::fs::symlink(&elsewhere, &link).expect("symlink");
        let paths = [link, dir.0.join("fallback.log")];
        let (opened, _) = open_first(&paths);
        assert_eq!(opened.map(|(_, path)| path), Some(paths[1].clone()));
        assert!(!elsewhere.exists(), "nothing was written through the link");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_new_log_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("mode");
        let path = dir.0.join("lumen-shell.log");
        let (opened, _) = open_first(std::slice::from_ref(&path));
        assert!(opened.is_some());
        let mode = std::fs::metadata(&path).expect("log").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
