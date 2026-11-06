//! The socket's two checks, for both sides: who is at the other end, by the
//! kernel's word (`SO_PEERCRED`), not by anything they send.
//!
//! The shell connects with [`connect_checked`], which refuses a peer that is
//! not root; `lumend` calls [`peer_uid`] on each connection it accepts and
//! refuses any uid but the shell's jail uid.

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// The uid of the process at the other end of `stream`, as the kernel
/// recorded it when the connection was made.
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the fd is a live socket borrowed from `stream`; `cred` and
    // `len` are valid for the kernel to write, and `len` says how much room
    // `cred` has.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::other("SO_PEERCRED gave a short answer"));
    }
    Ok(cred.uid)
}

/// Connects to `path` and keeps the connection only if `expect_uid` holds
/// the other end (root, for the shell), with `timeout` on every read and
/// write after that.
pub fn connect_checked(path: &Path, expect_uid: u32, timeout: Duration) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(path)?;
    let uid = peer_uid(&stream)?;
    if uid != expect_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the socket's other end is uid {uid}, not {expect_uid}"),
        ));
    }
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn own_uid() -> u32 {
        // SAFETY: geteuid takes nothing and cannot fail.
        unsafe { libc::geteuid() }
    }

    #[test]
    fn reads_the_peer_uid_both_ways() {
        let (a, b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), own_uid());
        assert_eq!(peer_uid(&b).unwrap(), own_uid());
    }

    #[test]
    fn refuses_a_peer_of_another_uid() {
        let dir = std::env::temp_dir().join(format!("lumend-proto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let me = own_uid();
        let ok = connect_checked(&path, me, Duration::from_secs(1));
        assert!(ok.is_ok());
        let wrong = connect_checked(&path, me.wrapping_add(1), Duration::from_secs(1));
        assert_eq!(wrong.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        drop(listener);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
