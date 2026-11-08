//! The calls `std` lacks, from the C library, and the only `unsafe` in
//! lumen-remote:
//!
//! * dropping root for good: into an empty folder (`chroot`), no
//!   supplementary groups, then the group and the user, real, effective and
//!   saved alike, then `no_new_privs` and no core dumps (the TLS key is in
//!   memory). Each step is checked, and so is the result: setting uid 0
//!   again must fail;
//! * `poll`, which the one thread waits in;
//! * SIGTERM and SIGINT set a flag that ends the loop, and SIGHUP is
//!   ignored, so a server started by hand outlives the SSH session;
//! * TCP keepalive, so a phone that walked away frees its connection;
//! * `getauxval` for the TV build, whose glibc 2.12 lacks it; ring asks it
//!   for the CPU's features. The auxiliary vector is read before the
//!   `chroot`, since `/proc` is gone after it.

use std::ffi::CString;
use std::io;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};

fn check(rc: libc::c_int, what: &str) -> Result<(), String> {
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("{what}: {}", io::Error::last_os_error()))
    }
}

pub fn euid() -> u32 {
    // SAFETY: geteuid takes nothing, cannot fail and touches no memory.
    unsafe { libc::geteuid() }
}

/// Changes root to `jail`, which must be an empty folder only root can
/// write, and becomes `uid`:`gid` with no other group, for good.
pub fn drop_root(jail: &Path, uid: u32, gid: u32) -> Result<(), String> {
    let jail_c = CString::new(jail.as_os_str().as_bytes())
        .map_err(|_| format!("{} has a NUL", jail.display()))?;
    // SAFETY: jail_c is a valid NUL-terminated path that outlives the call.
    check(unsafe { libc::chroot(jail_c.as_ptr()) }, "chroot")?;
    std::env::set_current_dir("/").map_err(|e| format!("chdir /: {e}"))?;
    // SAFETY: a count of 0 with a null list clears the groups; no memory is
    // read.
    check(unsafe { libc::setgroups(0, std::ptr::null()) }, "setgroups")?;
    // SAFETY: plain integers in, an int out.
    check(unsafe { libc::setresgid(gid, gid, gid) }, "setresgid")?;
    // SAFETY: plain integers in, an int out.
    check(unsafe { libc::setresuid(uid, uid, uid) }, "setresuid")?;
    // SAFETY: getters with no arguments; they cannot fail.
    let (ru, eu, rg, eg) = unsafe {
        (
            libc::getuid(),
            libc::geteuid(),
            libc::getgid(),
            libc::getegid(),
        )
    };
    if (ru, eu, rg, eg) != (uid, uid, gid, gid) {
        return Err(format!("still uid {ru}/{eu}, gid {rg}/{eg}"));
    }
    // SAFETY: plain integers in, an int out; it must fail now.
    if unsafe { libc::setuid(0) } == 0 {
        return Err("setuid(0) worked after dropping root".to_owned());
    }
    no_new_privs()
}

/// No set-uid program can raise what this process may do, and nothing of
/// its memory goes into a core dump or to a debugger of the same user.
pub fn no_new_privs() -> Result<(), String> {
    // SAFETY: prctl with integer arguments only.
    let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    check(rc, "PR_SET_NO_NEW_PRIVS")?;
    // SAFETY: prctl with integer arguments only.
    let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    check(rc, "PR_SET_DUMPABLE")
}

/// Waits for `fds`' events, at most `timeout_ms`; an interrupted wait is
/// not an error and reads as no events.
pub fn poll(fds: &mut [libc::pollfd], timeout_ms: i32) -> io::Result<usize> {
    // SAFETY: the pointer and length describe `fds`, which the kernel may
    // write `revents` into for the duration of the call.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    if n < 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            return Ok(0);
        }
        return Err(e);
    }
    Ok(n as usize)
}

pub fn pollfd(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

pub const POLLIN: libc::c_short = libc::POLLIN;
pub const POLLOUT: libc::c_short = libc::POLLOUT;
pub const POLLERR_HUP: libc::c_short = libc::POLLERR | libc::POLLHUP | libc::POLLNVAL;

static SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_stop(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

/// SIGTERM and SIGINT store their number; SIGHUP is ignored.
pub fn catch_signals() -> io::Result<()> {
    for (signal, handler) in [
        (
            libc::SIGTERM,
            on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t,
        ),
        (
            libc::SIGINT,
            on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t,
        ),
        (libc::SIGHUP, libc::SIG_IGN),
    ] {
        // SAFETY: an all-zero sigaction is valid; the handler only stores
        // to an atomic, which is async-signal-safe; both pointers outlive
        // the call. No SA_RESTART, so poll returns at once.
        let rc = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut())
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn stop_signal() -> Option<&'static str> {
    match SIGNAL.load(Ordering::SeqCst) {
        0 => None,
        libc::SIGTERM => Some("SIGTERM"),
        libc::SIGINT => Some("SIGINT"),
        _ => Some("a signal"),
    }
}

/// Keepalive after 60 s of silence, every 20 s, three times: a phone gone
/// from the network frees its connection within two minutes.
pub fn keepalive(fd: RawFd) {
    let set = |level: libc::c_int, name: libc::c_int, value: libc::c_int| {
        // SAFETY: the pointer and size describe `value`, which outlives the
        // call; a failure only leaves the default.
        unsafe {
            libc::setsockopt(
                fd,
                level,
                name,
                (&raw const value).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
    };
    set(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 60);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 20);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 3);
}

static AUXV: OnceLock<Vec<u8>> = OnceLock::new();

/// Reads the auxiliary vector now, while `/proc` is still there.
pub fn read_auxv() {
    AUXV.get_or_init(|| std::fs::read("/proc/self/auxv").unwrap_or_default());
}

/// Looks `kind` up in a raw auxiliary vector of native-endian `word`-byte
/// (type, value) pairs, stopping at `AT_NULL`; a copy of lumen-ctl's.
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

/// glibc 2.16's `getauxval`, for the TV's glibc 2.12 (see lumen-ctl).
#[cfg(all(target_os = "linux", target_arch = "arm"))]
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(kind: std::ffi::c_ulong) -> std::ffi::c_ulong {
    let auxv = AUXV.get_or_init(|| std::fs::read("/proc/self/auxv").unwrap_or_default());
    find_aux(
        auxv,
        u64::from(kind),
        std::mem::size_of::<std::ffi::c_ulong>(),
    ) as std::ffi::c_ulong
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
        assert_eq!(find_aux(&auxv, 51, 4), 0);
    }
}
