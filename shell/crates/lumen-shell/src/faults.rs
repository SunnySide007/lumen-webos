//! Page faults taken by the calling thread, for the slow-frame lines
//! (docs/specs/shell-mvp.md, "Frame time"). A major fault read a page back
//! from swap or disk, which can stall a frame for longer than its budget.

use crate::timing::Faults;

/// The calling thread's page faults so far, or `None` where the count is not
/// available (a target without the layout below, or a refused call).
///
/// Only glibc on 32-bit ARM, x86 and 64-bit x86_64 and ARM: those are the
/// targets whose `struct rusage` is two `timeval`s of two `long`s, then 14
/// `long`s. musl on 32 bits (64-bit `time_t`) and x32 (64-bit counters with a
/// 32-bit `long`) lay it out differently, so they get no count.
#[cfg(all(
    target_os = "linux",
    target_env = "gnu",
    any(
        target_arch = "arm",
        target_arch = "x86",
        all(
            target_pointer_width = "64",
            any(target_arch = "aarch64", target_arch = "x86_64")
        )
    )
))]
pub fn thread_faults() -> Option<Faults> {
    use std::ffi::{c_int, c_long, c_ulong};

    /// `struct timeval`: two `long`s on these targets.
    #[repr(C)]
    struct Timeval {
        sec: c_long,
        usec: c_long,
    }

    /// `struct rusage`: two times, then 14 counters, each a `long` (read as
    /// unsigned: a count past 2^31 on 32 bits must not turn negative).
    #[repr(C)]
    struct Rusage {
        utime: Timeval,
        stime: Timeval,
        maxrss: c_ulong,
        ixrss: c_ulong,
        idrss: c_ulong,
        isrss: c_ulong,
        minflt: c_ulong,
        majflt: c_ulong,
        nswap: c_ulong,
        inblock: c_ulong,
        oublock: c_ulong,
        msgsnd: c_ulong,
        msgrcv: c_ulong,
        nsignals: c_ulong,
        nvcsw: c_ulong,
        nivcsw: c_ulong,
    }

    // 72 bytes on 32-bit ARM, 144 on x86_64: the size glibc's struct has.
    const _: () = assert!(size_of::<Rusage>() == 18 * size_of::<c_long>());

    /// Linux's `RUSAGE_THREAD` (since 2.6.26; the B9 runs 4.4).
    const RUSAGE_THREAD: c_int = 1;

    unsafe extern "C" {
        fn getrusage(who: c_int, usage: *mut Rusage) -> c_int;
    }

    let mut usage = std::mem::MaybeUninit::<Rusage>::uninit();
    // SAFETY: on the targets this is compiled for, `Rusage` matches glibc's
    // `struct rusage` field for field (and in size, checked above), so the
    // call writes exactly one of them into `usage`, which is valid for writes
    // of that size and alignment for the call.
    let ok = unsafe { getrusage(RUSAGE_THREAD, usage.as_mut_ptr()) } == 0;
    if !ok {
        return None;
    }
    // SAFETY: a call that returned 0 filled the whole struct, and every field
    // is an integer, for which any bits are valid.
    let usage = unsafe { usage.assume_init() };
    // `c_ulong` is u32 on 32-bit ARM and x86, u64 on the 64-bit targets.
    #[allow(clippy::useless_conversion)]
    let (minor, major) = (u64::from(usage.minflt), u64::from(usage.majflt));
    Some(Faults { minor, major })
}

#[cfg(not(all(
    target_os = "linux",
    target_env = "gnu",
    any(
        target_arch = "arm",
        target_arch = "x86",
        all(
            target_pointer_width = "64",
            any(target_arch = "aarch64", target_arch = "x86_64")
        )
    )
)))]
pub fn thread_faults() -> Option<Faults> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn touching_fresh_pages_shows_as_minor_faults() {
        // Larger than glibc's largest mmap threshold (32 MiB on 64 bits), so
        // the buffer is always freshly mapped, never reused from the heap.
        const SIZE: usize = 64 << 20;
        let before = thread_faults().expect("getrusage works here");
        let mut pages = vec![0u8; SIZE];
        for at in (0..SIZE).step_by(4096) {
            pages[at] = 1;
        }
        std::hint::black_box(&mut pages);
        let after = thread_faults().expect("getrusage works here");
        // Read from the wrong offset, a count that stays 0 or flat would fail.
        assert!(
            after.minor > before.minor,
            "{} then {}",
            before.minor,
            after.minor
        );
        assert!(after.major >= before.major);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn elsewhere_there_is_no_count() {
        assert_eq!(thread_faults(), None);
    }
}
