//! lumen-ctl's log, `--log` (default `/var/lib/lumen/remote/lumen-ctl.log`):
//! one line per request and per reply, and a line when a session starts and
//! ends. It never holds the text of `text.insert` or `toast`, nor a request
//! line as it came; requests are logged as the typed verb rebuilt them.
//!
//! As lumen-guard's lookup log: each line is appended with one write, the
//! file moves to `<log>.1` when the next line would take it past 1 MiB, and a
//! line that cannot be written is dropped and counted. Every session (one
//! per SSH connection) appends to the same file; the pid tells them apart.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sys;

pub const LOG_FILE: &str = "/var/lib/lumen/remote/lumen-ctl.log";
pub const MAX_BYTES: u64 = 1 << 20;

pub struct Log {
    path: PathBuf,
    rotated: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
    pid: u32,
    /// Lines dropped since start.
    pub dropped: u64,
}

impl Log {
    /// Opens `path` for appending; its folder is not created (install makes it).
    pub fn open(path: &Path, max_bytes: u64) -> Self {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        let mut log = Self {
            path: path.to_owned(),
            rotated: PathBuf::from(rotated),
            max_bytes,
            file: None,
            size: 0,
            pid: std::process::id(),
            dropped: 0,
        };
        log.reopen();
        log
    }

    fn reopen(&mut self) -> bool {
        let opened = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(sys::os::OPEN_NO_FOLLOW)
            .open(&self.path);
        match opened {
            Ok(file) => {
                self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
                self.file = Some(file);
                true
            }
            Err(_) => {
                self.file = None;
                false
            }
        }
    }

    /// Appends `text` as one line: the time, the pid, then the text with any
    /// control character made a space.
    pub fn line(&mut self, text: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let text: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let line = format!(
            "{}.{:03} {} {text}\n",
            now.as_secs(),
            now.subsec_millis(),
            self.pid
        );
        self.append(&line);
    }

    fn append(&mut self, line: &str) {
        let len = line.len() as u64;
        if self.file.is_some() && self.size > 0 && self.size + len > self.max_bytes {
            self.file = None;
            if fs::rename(&self.path, &self.rotated).is_err() {
                self.dropped += 1;
                return;
            }
        }
        if self.file.is_none() && !self.reopen() {
            self.dropped += 1;
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        match file.write_all(line.as_bytes()) {
            Ok(()) => self.size += len,
            Err(_) => {
                let _ = file.set_len(self.size);
                self.dropped += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-ctl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rotates_before_the_line_that_would_pass_the_limit() {
        let dir = temp_dir("log-rotate");
        let path = dir.join("lumen-ctl.log");
        let mut log = Log::open(&path, 60);
        log.line("> 1 volume.set {\"level\":20}");
        let first = fs::read_to_string(&path).unwrap();
        assert!(
            first.ends_with(" > 1 volume.set {\"level\":20}\n"),
            "{first}"
        );
        assert!(first.contains(&format!(" {} ", std::process::id())));
        log.line("< 1 ok 12 ms");
        assert_eq!(
            fs::read_to_string(dir.join("lumen-ctl.log.1")).unwrap(),
            first
        );
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .ends_with(" < 1 ok 12 ms\n")
        );
        // Never more than two files.
        log.line("> 2 volume.up");
        log.line("< 2 ok 3 ms");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        assert_eq!(log.dropped, 0);
        // Only the owner reads it.
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_is_one_line() {
        let dir = temp_dir("log-line");
        let path = dir.join("log");
        let mut log = Log::open(&path, MAX_BYTES);
        log.line("< 3 failed: two\nlines\r");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.ends_with("failed: two lines \n"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_folder_drops_lines_and_recovers() {
        let dir = temp_dir("log-missing");
        let gone = dir.join("remote");
        let mut log = Log::open(&gone.join("lumen-ctl.log"), MAX_BYTES);
        log.line("start");
        assert_eq!(log.dropped, 1);
        fs::create_dir(&gone).unwrap();
        log.line("next");
        assert!(
            fs::read_to_string(gone.join("lumen-ctl.log"))
                .unwrap()
                .ends_with(" next\n")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_link_in_place_of_the_log_is_not_followed() {
        let dir = temp_dir("log-link");
        let target = dir.join("elsewhere");
        fs::write(&target, "").unwrap();
        let path = dir.join("lumen-ctl.log");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let mut log = Log::open(&path, MAX_BYTES);
        log.line("start");
        assert_eq!(log.dropped, 1);
        assert_eq!(fs::read_to_string(&target).unwrap(), "");
        fs::remove_dir_all(&dir).unwrap();
    }
}
