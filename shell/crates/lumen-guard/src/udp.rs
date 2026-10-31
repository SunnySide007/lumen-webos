//! The UDP side, on the main thread. Each query goes upstream on a socket of
//! its own (a fresh source port, the DSCP mark), and the first datagram back
//! with the query's ID goes to the asker unchanged. No answer within 3 s and
//! nothing is sent back: the asker retries. The loop also rewrites the status
//! file every ten seconds.

use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use crate::guard::{Guard, Throttle};
use crate::sys::{self, Interest, Poller};
use crate::wire::{self, MAX_MESSAGE};

/// Queries waiting for the upstream at once; more are dropped (and retried).
const MAX_PENDING: usize = 256;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(3);
const STATUS_EVERY: Duration = Duration::from_secs(10);
/// The longest wait in `poll`, so a stop request is seen within it.
pub const TICK: Duration = Duration::from_secs(1);
/// Datagrams read from the listen socket per wake-up.
const BATCH: usize = 64;

struct Pending {
    sock: UdpSocket,
    client: SocketAddr,
    id: [u8; 2],
    deadline: Instant,
}

enum Upstream {
    Waiting,
    Answered,
    Failed,
}

pub fn serve(guard: &Guard, listen: &UdpSocket) -> io::Result<()> {
    let mut pending: Vec<Pending> = Vec::new();
    // One byte over the limit, to tell a message at the limit from a longer one.
    let mut buf = vec![0; MAX_MESSAGE + 1];
    let mut poller = Poller::default();
    let mut next_status = Instant::now();
    let mut status_errors = Throttle::new("status file");
    let mut receive_errors = Throttle::new("udp receive");
    let mut upstream_errors = Throttle::new("udp upstream");

    while !sys::stopping() {
        let now = Instant::now();
        if now >= next_status {
            if let Err(err) = guard.write_status() {
                status_errors.warn(&err);
            }
            next_status = now + STATUS_EVERY;
        }
        let before = pending.len();
        pending.retain(|p| p.deadline > now);
        guard.upstream_timeouts((before - pending.len()) as u64);

        poller.clear();
        poller.add(listen.as_fd(), Interest::Read);
        for p in &pending {
            poller.add(p.sock.as_fd(), Interest::Read);
        }
        let first_deadline = pending
            .iter()
            .map(|p| p.deadline)
            .min()
            .unwrap_or(next_status);
        let wait = next_status
            .min(first_deadline)
            .saturating_duration_since(now)
            .min(TICK);
        poller.wait(wait)?;

        // Answers first: pending[i] was polled at index i + 1. Going backwards,
        // swap_remove only moves entries already looked at.
        for i in (0..pending.len()).rev() {
            if !poller.ready(i + 1) {
                continue;
            }
            match relay_answer(listen, &pending[i], &mut buf) {
                Upstream::Waiting => {}
                Upstream::Answered => {
                    pending.swap_remove(i);
                }
                Upstream::Failed => {
                    guard.upstream_error();
                    pending.swap_remove(i);
                }
            }
        }

        if poller.ready(0) {
            for _ in 0..BATCH {
                let (n, client) = match listen.recv_from(&mut buf) {
                    Ok(got) => got,
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        receive_errors.warn(&err);
                        break;
                    }
                };
                let msg = &buf[..n];
                if n > MAX_MESSAGE || !wire::is_query(msg) {
                    continue;
                }
                guard.record(msg);
                if pending.len() >= MAX_PENDING {
                    guard.upstream_error();
                    continue;
                }
                match send_upstream(guard, msg) {
                    Ok(sock) => pending.push(Pending {
                        sock,
                        client,
                        id: [msg[0], msg[1]],
                        deadline: Instant::now() + UPSTREAM_TIMEOUT,
                    }),
                    Err(err) => {
                        guard.upstream_error();
                        upstream_errors.warn(&err);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Sends the query upstream from a new socket, marked before its first packet.
fn send_upstream(guard: &Guard, msg: &[u8]) -> io::Result<UdpSocket> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    if guard.config.dscp != 0 {
        sys::set_dscp(sock.as_fd(), guard.config.dscp)?;
    }
    sock.connect(guard.config.upstream)?;
    sock.set_nonblocking(true)?;
    sock.send(msg)?;
    Ok(sock)
}

/// Passes the upstream's answer to the asker, unchanged. Anything else that
/// arrives (another ID, too long to carry) is dropped.
fn relay_answer(listen: &UdpSocket, p: &Pending, buf: &mut [u8]) -> Upstream {
    loop {
        match p.sock.recv(buf) {
            Ok(n) if wire::id(&buf[..n]) != Some(p.id) => {}
            Ok(n) if n > MAX_MESSAGE => return Upstream::Failed,
            Ok(n) => {
                // The asker may be gone; that is its business.
                let _ = listen.send_to(&buf[..n], p.client);
                return Upstream::Answered;
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return Upstream::Waiting,
            // Connection refused: nothing listens upstream.
            Err(_) => return Upstream::Failed,
        }
    }
}
