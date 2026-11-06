//! inotify, for the file-mode watch and the kill switch: the kernel tells
//! `lumend` when a mode changes or a file appears, so nothing is polled.
//! Watches are added through `/proc/self/fd/N` of a folder `lumend` holds
//! open, so a link swapped in by name can't move one.

use std::ffi::{CString, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

pub const IN_ATTRIB: u32 = libc::IN_ATTRIB;
pub const IN_CREATE: u32 = libc::IN_CREATE;
pub const IN_MOVED_TO: u32 = libc::IN_MOVED_TO;
pub const IN_MOVED_FROM: u32 = libc::IN_MOVED_FROM;
pub const IN_DELETE: u32 = libc::IN_DELETE;
pub const IN_ONLYDIR: u32 = libc::IN_ONLYDIR;
pub const IN_Q_OVERFLOW: u32 = libc::IN_Q_OVERFLOW;

/// One event, its name copied out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub wd: i32,
    pub mask: u32,
    pub name: Option<OsString>,
}

/// An inotify instance; the thread that reads it and those that add or
/// remove watches may differ.
pub struct Inotify {
    file: File,
}

impl Inotify {
    pub fn new() -> io::Result<Self> {
        // SAFETY: inotify_init1 takes flags and returns a new fd or -1.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the fd is new and owned by nothing else.
        let file = unsafe { File::from_raw_fd(fd) };
        Ok(Self { file })
    }

    /// Watches the folder at `path` for `mask`; the watch's descriptor.
    pub fn add(&self, path: &Path, mask: u32) -> io::Result<i32> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in a path"))?;
        // SAFETY: the inotify fd is live; the path is NUL-terminated and
        // outlives the call.
        let wd = unsafe { libc::inotify_add_watch(self.file.as_raw_fd(), c.as_ptr(), mask) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(wd)
    }

    pub fn remove(&self, wd: i32) {
        // SAFETY: integers only; a stale wd is refused with EINVAL, which
        // is fine.
        unsafe {
            libc::inotify_rm_watch(self.file.as_raw_fd(), wd);
        }
    }

    /// Blocks until events come, and returns them.
    pub fn read(&self) -> io::Result<Vec<Event>> {
        let mut buf = vec![0u8; 16 << 10];
        let n = (&self.file).read(&mut buf)?;
        Ok(parse(&buf[..n]))
    }
}

/// Splits what one `read` gave into events: `struct inotify_event` is four
/// native-endian 32-bit words (wd, mask, cookie, len), then `len` bytes of
/// NUL-padded name.
pub fn parse(mut buf: &[u8]) -> Vec<Event> {
    let word = |b: &[u8]| -> u32 { u32::from_ne_bytes([b[0], b[1], b[2], b[3]]) };
    let mut events = Vec::new();
    while buf.len() >= 16 {
        let wd = word(&buf[0..4]) as i32;
        let mask = word(&buf[4..8]);
        let len = word(&buf[12..16]) as usize;
        let Some(raw) = buf.get(16..16 + len) else {
            break;
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let name = (end > 0).then(|| OsString::from_vec(raw[..end].to_vec()));
        events.push(Event { wd, mask, name });
        buf = &buf[16 + len..];
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(wd: i32, mask: u32, name: &[u8], len: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(wd as u32).to_ne_bytes());
        v.extend_from_slice(&mask.to_ne_bytes());
        v.extend_from_slice(&0u32.to_ne_bytes());
        v.extend_from_slice(&(len as u32).to_ne_bytes());
        let mut n = name.to_vec();
        n.resize(len, 0);
        v.extend_from_slice(&n);
        v
    }

    #[test]
    fn parses_events_with_and_without_names() {
        let mut buf = raw(1, IN_ATTRIB, b"", 0);
        buf.extend(raw(2, IN_CREATE, b"lumen_disable", 16));
        let events = parse(&buf);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, None);
        assert_eq!(events[1].wd, 2);
        assert_eq!(
            events[1].name.as_deref(),
            Some(std::ffi::OsStr::new("lumen_disable"))
        );
    }

    #[test]
    fn stops_at_a_cut_event() {
        let mut buf = raw(1, IN_ATTRIB, b"abc", 16);
        buf.truncate(20);
        assert!(parse(&buf).is_empty());
    }

    #[test]
    fn sees_a_file_made_in_a_watched_folder() {
        let dir = std::env::temp_dir().join(format!("lumend-inotify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ino = Inotify::new().unwrap();
        let wd = ino.add(&dir, IN_CREATE | IN_ONLYDIR).unwrap();
        std::fs::write(dir.join("x"), b"").unwrap();
        let events = ino.read().unwrap();
        assert!(
            events
                .iter()
                .any(|e| e.wd == wd && e.name.as_deref() == Some("x".as_ref()))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
