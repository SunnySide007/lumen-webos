//! The lookup log: `lookups.log` in the log directory, one line per lookup,
//! moved to `lookups.log.1` when the next line would take it past the size
//! limit, so the two files never hold much more than twice the limit.
//!
//! Each line is appended with one write. A line that cannot be written (a full
//! disk, a missing directory) is dropped and counted, and a partly written one
//! is cut off again, so Guard keeps forwarding whatever happens to its log.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE: &str = "lookups.log";
pub const ROTATED: &str = "lookups.log.1";
/// `--max-log-bytes`' default: 4 MiB a file.
pub const DEFAULT_MAX_BYTES: u64 = 4 << 20;

pub struct Lookups {
    path: PathBuf,
    rotated: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
    /// Lines dropped since start.
    pub dropped: u64,
}

impl Lookups {
    /// Opens `dir/lookups.log` for appending, carrying on from an earlier run.
    pub fn open(dir: &Path, max_bytes: u64) -> Self {
        let mut log = Self {
            path: dir.join(FILE),
            rotated: dir.join(ROTATED),
            max_bytes,
            file: None,
            size: 0,
            dropped: 0,
        };
        log.reopen();
        log
    }

    fn reopen(&mut self) -> bool {
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
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

    /// Appends one line, which ends in a newline.
    pub fn append(&mut self, line: &str) {
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-guard-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rotates_before_the_line_that_would_pass_the_limit() {
        let dir = temp_dir("rotate");
        let mut log = Lookups::open(&dir, 30);
        log.append("1000 A one.example allow\n"); // 25 bytes
        log.append("1001 A two.example allow\n"); // would make 50
        assert_eq!(
            fs::read_to_string(dir.join(ROTATED)).unwrap(),
            "1000 A one.example allow\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            "1001 A two.example allow\n"
        );
        // A second rotation replaces the old file: never more than two.
        log.append("1002 A six.example allow\n");
        assert_eq!(
            fs::read_to_string(dir.join(ROTATED)).unwrap(),
            "1001 A two.example allow\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            "1002 A six.example allow\n"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        assert_eq!(log.dropped, 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn carries_on_from_an_earlier_run() {
        let dir = temp_dir("reopen");
        fs::write(dir.join(FILE), "999 A old.example allow\n").unwrap();
        let mut log = Lookups::open(&dir, 1 << 20);
        log.append("1000 A new.example would-block\n");
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            "999 A old.example allow\n1000 A new.example would-block\n"
        );
        // The size it found counts toward the limit.
        let mut log = Lookups::open(&dir, 60);
        log.append("1001 A next.example allow\n");
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            "1001 A next.example allow\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_directory_drops_lines_and_recovers() {
        let dir = temp_dir("missing");
        let gone = dir.join("not-yet");
        let mut log = Lookups::open(&gone, 1 << 20);
        log.append("1000 A a.example allow\n");
        log.append("1001 A b.example allow\n");
        assert_eq!(log.dropped, 2);
        fs::create_dir(&gone).unwrap();
        log.append("1002 A c.example allow\n");
        assert_eq!(
            fs::read_to_string(gone.join(FILE)).unwrap(),
            "1002 A c.example allow\n"
        );
        assert_eq!(log.dropped, 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_full_disk_drops_lines_without_failing() {
        // /dev/full takes the open and fails every write with ENOSPC.
        if !Path::new("/dev/full").exists() {
            return;
        }
        let mut log = Lookups {
            path: PathBuf::from("/dev/full"),
            rotated: PathBuf::from("/dev/null"),
            max_bytes: u64::MAX,
            file: None,
            size: 0,
            dropped: 0,
        };
        log.reopen();
        log.append("1000 A a.example allow\n");
        log.append("1001 A b.example allow\n");
        assert_eq!(log.dropped, 2);
    }
}
