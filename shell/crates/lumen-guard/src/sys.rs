//! The only module that calls the C library, and so the only one allowed
//! `unsafe` (the crate root denies it everywhere else). Each function wraps one
//! call `std` lacks:
//!
//! * the DSCP mark on Guard's upstream sockets (`IP_TOS`), which the TV's nat
//!   rule skips so Guard's own lookups are not sent back to it;
//! * a TCP connect with that mark on the very first packet;
//! * `poll`, so two threads can wait on many sockets without spinning;
//! * a SIGTERM handler, so Guard writes its status before it ends;
//! * the local date, for the counts since midnight.
//!
//! It also gives the TV build the one C function its old glibc lacks,
//! `getauxval`, as lumen-shell's `compat.rs` does.

use std::io;
use std::mem::size_of;
use std::net::{SocketAddrV4, TcpStream};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::status::Date;

/// Marks every packet the socket sends with `dscp` (0 to 63).
pub fn set_dscp(sock: BorrowedFd<'_>, dscp: u8) -> io::Result<()> {
    let tos = libc::c_int::from(dscp) << 2;
    // SAFETY: the descriptor is borrowed from a live socket; `tos` is a c_int
    // that outlives the call, and the length passed is its size.
    let rc = unsafe {
        libc::setsockopt(
            sock.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_TOS,
            (&raw const tos).cast(),
            size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Starts a non-blocking TCP connection to `addr`, marked with `dscp` (unless
/// 0) before the SYN leaves. Wait for the stream to be writable, then check
/// `take_error`.
pub fn connect_marked(addr: SocketAddrV4, dscp: u8) -> io::Result<TcpStream> {
    // SAFETY: socket() takes no pointers; its result is checked before use.
    let raw = unsafe {
        libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a new descriptor nothing else owns; OwnedFd takes it
    // exactly once and closes it on every path below.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if dscp != 0 {
        set_dscp(fd.as_fd(), dscp)?;
    }
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: addr.port().to_be(),
        sin_addr: libc::in_addr {
            s_addr: u32::from(*addr.ip()).to_be(),
        },
        sin_zero: [0; 8],
    };
    // SAFETY: `sin` is a valid sockaddr_in that outlives the call, the length
    // passed is its size, and `fd` is the open socket made above.
    let rc = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&raw const sin).cast(),
            size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(err);
        }
    }
    Ok(TcpStream::from(fd))
}

/// What a socket is waited on for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interest {
    Read,
    Write,
}

/// The sockets one `poll` call waits on, in the order they were added.
#[derive(Default)]
pub struct Poller {
    fds: Vec<libc::pollfd>,
}

impl Poller {
    pub fn clear(&mut self) {
        self.fds.clear();
    }

    /// Adds a socket; its index is the number added before it.
    pub fn add(&mut self, fd: BorrowedFd<'_>, interest: Interest) {
        let events = match interest {
            Interest::Read => libc::POLLIN,
            Interest::Write => libc::POLLOUT,
        };
        self.fds.push(libc::pollfd {
            fd: fd.as_raw_fd(),
            events,
            revents: 0,
        });
    }

    /// Waits until a socket is ready, the timeout passes, or a signal comes.
    pub fn wait(&mut self, timeout: Duration) -> io::Result<()> {
        // Rounded up: a deadline 0.4 ms away must not become a 0 ms spin.
        let ms = libc::c_int::try_from(timeout.as_nanos().div_ceil(1_000_000))
            .unwrap_or(libc::c_int::MAX);
        // SAFETY: the pointer and length describe `self.fds`, initialised
        // pollfd structs that stay borrowed, unmoved, for the whole call.
        let rc = unsafe { libc::poll(self.fds.as_mut_ptr(), self.fds.len() as libc::nfds_t, ms) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Whether the socket at `index` is ready, or has an error or hang-up the
    /// next read or write will report.
    pub fn ready(&self, index: usize) -> bool {
        self.fds.get(index).is_some_and(|p| p.revents != 0)
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// SIGTERM and SIGINT set the stop flag. No SA_RESTART: a thread in `poll`
/// wakes at once, and the other within its next tick.
pub fn catch_stop_signals() -> io::Result<()> {
    for signal in [libc::SIGTERM, libc::SIGINT] {
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
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Asks both loops to end, as SIGTERM does.
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn stopping() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// The date at `unix` seconds in the TV's time zone (`TZ`, else
/// `/etc/localtime`), or `None` when the C library cannot say.
pub fn local_date(unix: u64) -> Option<Date> {
    let time = libc::time_t::try_from(unix).ok()?;
    // SAFETY: an all-zero tm is a valid value for localtime_r to overwrite;
    // both pointers are to locals that outlive the call, and localtime_r is
    // the thread-safe form.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            return None;
        }
        tm
    };
    Some(Date {
        year: tm.tm_year + 1900,
        month: u8::try_from(tm.tm_mon + 1).ok()?,
        day: u8::try_from(tm.tm_mday).ok()?,
    })
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
        Ok(auxv) => {
            find_aux(&auxv, u64::from(kind), size_of::<std::ffi::c_ulong>()) as std::ffi::c_ulong
        }
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, UdpSocket};
    use std::time::Instant;

    #[test]
    fn marks_a_udp_socket() {
        let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        set_dscp(sock.as_fd(), 7).unwrap();
        set_dscp(sock.as_fd(), 63).unwrap();
    }

    #[test]
    fn connects_a_marked_stream() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = match listener.local_addr().unwrap() {
            std::net::SocketAddr::V4(a) => a,
            std::net::SocketAddr::V6(_) => unreachable!(),
        };
        let mut stream = connect_marked(addr, 7).unwrap();
        let mut poller = Poller::default();
        poller.add(stream.as_fd(), Interest::Write);
        poller.wait(Duration::from_secs(2)).unwrap();
        assert!(poller.ready(0));
        assert!(stream.take_error().unwrap().is_none());
        let (mut peer, _) = listener.accept().unwrap();
        stream.write_all(b"hi").unwrap();
        let mut got = [0; 2];
        peer.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hi");
    }

    #[test]
    fn a_refused_connect_reports_its_error() {
        // Bind a port, then free it, so nothing listens there.
        let addr = {
            let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            match l.local_addr().unwrap() {
                std::net::SocketAddr::V4(a) => a,
                std::net::SocketAddr::V6(_) => unreachable!(),
            }
        };
        let stream = connect_marked(addr, 7).unwrap();
        let mut poller = Poller::default();
        poller.add(stream.as_fd(), Interest::Write);
        poller.wait(Duration::from_secs(2)).unwrap();
        assert!(poller.ready(0));
        assert!(stream.take_error().unwrap().is_some());
    }

    #[test]
    fn poll_waits_for_data_or_the_timeout() {
        let a = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let b = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut poller = Poller::default();
        poller.add(a.as_fd(), Interest::Read);
        poller.add(b.as_fd(), Interest::Read);
        let start = Instant::now();
        poller.wait(Duration::from_millis(50)).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(45));
        assert!(!poller.ready(0) && !poller.ready(1));

        a.send_to(b"x", b.local_addr().unwrap()).unwrap();
        poller.clear();
        poller.add(a.as_fd(), Interest::Read);
        poller.add(b.as_fd(), Interest::Read);
        poller.wait(Duration::from_secs(2)).unwrap();
        assert!(!poller.ready(0));
        assert!(poller.ready(1));
        assert!(!poller.ready(2));
    }

    #[test]
    fn knows_the_local_date() {
        let date = local_date(1_793_318_400).unwrap();
        assert!(date.year == 2026 && date.month == 10);
        assert!((29..=31).contains(&date.day), "{date}");
    }

    #[test]
    fn finds_auxiliary_vector_entries() {
        let auxv: Vec<u8> = [(6u32, 4096u32), (16, 0x1F_B8D7), (0, 0), (51, 8192)]
            .iter()
            .flat_map(|(k, v)| [k.to_ne_bytes(), v.to_ne_bytes()].concat())
            .collect();
        assert_eq!(find_aux(&auxv, 16, 4), 0x1F_B8D7);
        assert_eq!(find_aux(&auxv, 6, 4), 4096);
        // After AT_NULL, and absent: 0.
        assert_eq!(find_aux(&auxv, 51, 4), 0);
        assert_eq!(find_aux(&[], 6, 4), 0);
    }
}
