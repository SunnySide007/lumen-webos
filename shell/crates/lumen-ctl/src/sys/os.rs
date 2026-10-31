//! The calls `std` lacks, from the C library:
//!
//! * SIGTERM, SIGHUP and SIGINT set a flag that `serve`'s ticker sees within
//!   50 ms, whatever the other threads are blocked in;
//! * `_exit`, so the end of a session runs no atexit handler while luna's
//!   thread may still be in a callback;
//! * the open flags that keep `privacy.state` from following a link or
//!   waiting on a pipe in `/tmp`, and `geteuid` for the owner those files
//!   must have;
//! * `getauxval` for the TV build, whose glibc 2.12 lacks it, as lumen-guard's
//!   `sys.rs` and lumen-shell's `compat.rs` give it.

use std::sync::atomic::{AtomicI32, Ordering};

/// `O_NOFOLLOW | O_NONBLOCK`, for files that others could have replaced.
pub const OPEN_NO_FOLLOW_NO_BLOCK: i32 = libc::O_NOFOLLOW | libc::O_NONBLOCK;
pub const OPEN_NO_FOLLOW: i32 = libc::O_NOFOLLOW;

static SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_stop_signal(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::SeqCst);
}

/// SIGTERM, SIGHUP (the SSH session ending) and SIGINT store their number.
/// The handler is installed whatever the parent left the signal set to, so
/// an ignored SIGHUP inherited from the SSH server still ends lumen-ctl.
pub fn catch_stop_signals() -> std::io::Result<()> {
    for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        // SAFETY: an all-zero sigaction is valid (no flags, no handler); the
        // handler set here only stores to an atomic, which is
        // async-signal-safe; both pointers outlive the calls.
        let rc = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction =
                on_stop_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(signal, &action, std::ptr::null_mut())
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// The stop signal's name, once one has come.
pub fn stop_signal() -> Option<&'static str> {
    match SIGNAL.load(Ordering::SeqCst) {
        0 => None,
        libc::SIGTERM => Some("SIGTERM"),
        libc::SIGHUP => Some("SIGHUP"),
        libc::SIGINT => Some("SIGINT"),
        _ => Some("a signal"),
    }
}

/// The effective user: lumen-ctl reads its own files only if this user owns
/// them (root on the TV).
pub fn euid() -> u32 {
    // SAFETY: geteuid takes nothing, cannot fail and touches no memory.
    unsafe { libc::geteuid() }
}

/// Ends the process at once with `code`: no atexit handlers, no unwinding.
/// Everything lumen-ctl writes is already flushed (stdout after each line,
/// the log with one write a line).
pub fn exit_now(code: i32) -> ! {
    // SAFETY: _exit takes an int and never returns; it is async-signal-safe
    // and needs nothing from the caller.
    unsafe { libc::_exit(code) }
}

/// Looks `kind` up in a raw auxiliary vector of native-endian `word`-byte
/// (type, value) pairs, stopping at `AT_NULL`. Unknown types read as 0, which
/// is what glibc returns for them. A copy of lumen-shell's `compat::find_aux`.
// Only the TV build calls it outside tests.
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
        assert_eq!(find_aux(&[], 6, 4), 0);
    }

    #[test]
    fn no_signal_has_come() {
        assert_eq!(stop_signal(), None);
    }
}
