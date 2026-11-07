//! The calls `std` lacks, from the C library:
//!
//! * the stop signals, blocked in every thread and taken by one with
//!   `sigwait`, so no handler runs inside another thread's work;
//! * `kill`, for LG's sender programs and for `lumend stop`;
//! * `mount` and `umount2`, for the stand-ins;
//! * `setsid` for the daemon `lumend start` leaves behind;
//! * `shutdown` of a listening socket, to end its accept thread;
//! * the uid calls of the link trial's child (`setgroups`, `setgid`,
//!   `setuid`), and `geteuid`;
//! * `getauxval` for the TV's glibc 2.12, as lumen-ctl's `os.rs` gives it.

use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// The effective user.
pub fn euid() -> u32 {
    // SAFETY: geteuid takes nothing, cannot fail and touches no memory.
    unsafe { libc::geteuid() }
}

/// The real user and group, for the link trial's check that it dropped
/// root.
pub fn real_ids() -> (u32, u32) {
    // SAFETY: getuid and getgid take nothing, cannot fail and touch no
    // memory.
    unsafe { (libc::getuid(), libc::getgid()) }
}

fn stop_set() -> libc::sigset_t {
    // SAFETY: an all-zero sigset_t is valid storage; sigemptyset makes it a
    // proper empty set before anything reads it, and sigaddset only adds
    // valid signal numbers to it.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, signal);
        }
        set
    }
}

/// Blocks SIGTERM, SIGINT and SIGHUP in this thread and every thread it
/// starts afterwards, so only [`wait_stop_signal`] sees them. Called before
/// the daemon starts its first thread.
pub fn block_stop_signals() -> io::Result<()> {
    let set = stop_set();
    // SAFETY: `set` is a valid signal set; the old mask is not wanted.
    let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }
    Ok(())
}

/// Waits for one of the stop signals and returns its name.
pub fn wait_stop_signal() -> &'static str {
    let set = stop_set();
    let mut signal: libc::c_int = 0;
    loop {
        // SAFETY: `set` is a valid signal set and `signal` valid to write.
        let rc = unsafe { libc::sigwait(&set, &mut signal) };
        if rc == 0 {
            break;
        }
    }
    match signal {
        libc::SIGTERM => "SIGTERM",
        libc::SIGINT => "SIGINT",
        libc::SIGHUP => "SIGHUP",
        _ => "a signal",
    }
}

/// SIGTERM or SIGKILL to one process.
pub fn kill(pid: u32, hard: bool) -> io::Result<()> {
    let pid = libc::pid_t::try_from(pid).map_err(|_| io::Error::other("pid out of range"))?;
    if pid <= 1 {
        return Err(io::Error::other("refusing pid 0 or 1"));
    }
    let signal = if hard { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: kill takes two integers and touches no memory; a pid above 1
    // names one process, never a group or every process.
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path with a NUL byte"))
}

/// A bind mount of the file `source` over the file `target`, in init's
/// mount namespace: it lives in RAM and is gone at the next boot.
pub fn bind_mount(source: &Path, target: &Path) -> io::Result<()> {
    let (s, t) = (c_path(source)?, c_path(target)?);
    // SAFETY: both are NUL-terminated paths that outlive the call; a bind
    // mount reads no filesystem type and no data.
    let rc = unsafe {
        libc::mount(
            s.as_ptr(),
            t.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes the bind mount at `target` read-only, nosuid and nodev.
pub fn remount_read_only(target: &Path) -> io::Result<()> {
    let t = c_path(target)?;
    let flags =
        libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV;
    // SAFETY: as in bind_mount; a remount of a bind changes only its flags.
    let rc = unsafe {
        libc::mount(
            std::ptr::null(),
            t.as_ptr(),
            std::ptr::null(),
            flags,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Takes the mount at `target` away; `detach` lets a busy one go once its
/// last user ends.
pub fn unmount(target: &Path, detach: bool) -> io::Result<()> {
    let t = c_path(target)?;
    let flags = if detach { libc::MNT_DETACH } else { 0 };
    // SAFETY: a NUL-terminated path that outlives the call, and flags.
    if unsafe { libc::umount2(t.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes the program `command` starts the leader of a session of its own,
/// with no terminal, so it outlives the door that started it.
pub fn new_session(command: &mut Command) {
    // SAFETY: the closure runs in the child between fork and exec, and calls
    // only setsid, which is async-signal-safe and allocates nothing.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Wakes a thread blocked in `accept` on `listener`, which then fails: how
/// a link whose socket was replaced ends its old thread.
pub fn shutdown_listener(listener: &UnixListener) {
    // SAFETY: the fd is a live socket borrowed from `listener`; shutdown
    // touches no memory, and an error leaves the fd as it was.
    unsafe {
        libc::shutdown(listener.as_raw_fd(), libc::SHUT_RDWR);
    }
}

/// Gives up root for good in this process: no supplementary groups, then
/// `gid`, then `uid`, and fails unless the ids read back. Only the link
/// trial's child calls it, before it does anything else.
pub fn drop_to(uid: u32, gid: u32) -> io::Result<()> {
    // SAFETY: setgroups with a count of 0 reads no memory; setgid and
    // setuid take integers. The order matters: groups and gid while still
    // root.
    unsafe {
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setgid(gid) != 0
            || libc::setuid(uid) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    if real_ids() != (uid, gid) || euid() != uid {
        return Err(io::Error::other("the ids did not change"));
    }
    Ok(())
}

/// Looks `kind` up in a raw auxiliary vector of native-endian `word`-byte
/// (type, value) pairs, stopping at `AT_NULL`. Unknown types read as 0, which
/// is what glibc returns for them. A copy of lumen-ctl's `os::find_aux`.
#[cfg_attr(not(all(target_os = "linux", target_arch = "arm")), allow(dead_code))]
fn find_aux(auxv: &[u8], kind: u64, word: usize) -> u64 {
    let read = |bytes: &[u8]| -> u64 {
        let mut buf = [0u8; 8];
        if cfg!(target_endian = "little") {
            buf[..word].copy_from_slice(bytes);
            u64::from_le_bytes(buf)
        } else {
            buf[8 - word..].copy_from_slice(bytes);
            u64::from_be_bytes(buf)
        }
    };
    for pair in auxv.chunks_exact(word * 2) {
        let (key, value) = pair.split_at(word);
        match read(key) {
            0 => break,
            k if k == kind => return read(value),
            _ => {}
        }
    }
    0
}

/// webosbrew's SDK links against a glibc 2.12 sysroot, but Rust's std calls
/// `getauxval`, which glibc gained in 2.16; this reads the auxiliary vector
/// the kernel publishes instead. On 32-bit ARM `unsigned long` is 32 bits.
#[cfg(all(target_os = "linux", target_arch = "arm"))]
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(kind: std::ffi::c_ulong) -> std::ffi::c_ulong {
    match std::fs::read("/proc/self/auxv") {
        Ok(auxv) => find_aux(
            &auxv,
            u64::from(kind),
            std::mem::size_of::<std::ffi::c_ulong>(),
        ) as std::ffi::c_ulong,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_auxiliary_vector_entries() {
        let auxv: Vec<u8> = [(6u32, 4096u32), (16, 0x1F_B8D7), (0, 0), (51, 8192)]
            .iter()
            .flat_map(|(k, v)| [k.to_ne_bytes(), v.to_ne_bytes()].concat())
            .collect();
        assert_eq!(find_aux(&auxv, 16, 4), 0x1F_B8D7);
        assert_eq!(find_aux(&auxv, 6, 4), 4096);
        assert_eq!(find_aux(&auxv, 51, 4), 0);
    }

    #[test]
    fn never_signals_init_or_a_group() {
        assert!(kill(0, false).is_err());
        assert!(kill(1, true).is_err());
        assert!(kill(u32::MAX, false).is_err());
    }
}
