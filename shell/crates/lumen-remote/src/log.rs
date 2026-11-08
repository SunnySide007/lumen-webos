//! lumen-remote's log: connections, pairings, WebSockets opening and
//! closing with the device's id, refusals and errors. Never a token, a
//! pairing code or anything a phone typed.
//!
//! The file is opened before root is dropped and kept open; it can't be
//! renamed afterwards (the server lives in an empty folder), so the run
//! before is moved to `.1` at start when it is over 1 MiB, and a run stops
//! writing at 1 MiB with a last line saying so.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_BYTES: u64 = 1 << 20;

pub struct Log {
    file: Option<File>,
    written: u64,
    full: bool,
    pid: u32,
}

impl Log {
    pub fn open(path: &Path) -> Self {
        if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let mut old = path.as_os_str().to_owned();
            old.push(".1");
            let _ = fs::rename(path, old);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .ok();
        let written = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        Self {
            file,
            written,
            full: false,
            pid: std::process::id(),
        }
    }

    pub fn line(&mut self, text: &str) {
        if self.full {
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let clean: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let mut line = format!(
            "{}.{:03} {} {clean}\n",
            now.as_secs(),
            now.subsec_millis(),
            self.pid
        );
        if self.written + line.len() as u64 > MAX_BYTES {
            line = format!(
                "{} {} log full: nothing more this run\n",
                now.as_secs(),
                self.pid
            );
            self.full = true;
        }
        if file.write_all(line.as_bytes()).is_ok() {
            self.written += line.len() as u64;
        }
    }
}
