//! DNS over TLS (RFC 7858) to the TLS server, stage G3: Quad9 as
//! `dns.quad9.net` on port 853, with rustls (TLS 1.3, ring) on plain
//! blocking sockets, no async runtime (decision 11, P4).
//!
//! The UDP and TCP loops hand each lookup the TLS route takes to a small
//! queue; `WORKERS` threads take them. Each worker keeps one connection open
//! and reuses it, one query at a time, so at most `WORKERS` connections are
//! open; a connection idle for `IDLE` is closed. A query on a reused
//! connection the server has closed meanwhile is asked again on a new one.
//!
//! When the TLS server can't be reached, or won't complete a checked
//! handshake, within `HANDSHAKE` (and never later than `CLEAR_RESERVE` before
//! the lookup's deadline), the worker notes the failure (upstream.rs: the
//! next address, then a backoff) and asks the router in clear instead, so the
//! asker still gets its answer. No query is ever written to a server whose
//! certificate failed: rustls sends nothing of ours before the handshake is
//! done.

use std::collections::VecDeque;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{CertificateError, ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::args::Config;
use crate::guard::{Guard, Throttle};
use crate::sys::{self, Interest, Poller};
use crate::upstream::{Failure, Route, Why};
use crate::wire;

/// Worker threads, and so TLS connections open at once at most. A lookup
/// over a connection already open takes one round trip.
pub const WORKERS: usize = 3;
/// The workers' stacks: rustls and ring need little, and buffers are heap.
pub const STACK: usize = 256 << 10;
/// Lookups waiting for a worker; more are dropped, and the asker retries.
const QUEUE: usize = 64;
/// A connection unused this long is closed (RFC 7858 asks clients to close
/// idle ones rather than leave it to the server).
const IDLE: Duration = Duration::from_secs(15);
/// The longest wait for a new connection and its handshake.
const HANDSHAKE: Duration = Duration::from_millis(1500);
/// Time kept back from a lookup's deadline for the fallback in clear.
const CLEAR_RESERVE: Duration = Duration::from_millis(1200);
/// The longest a reply to a TCP asker may block a worker.
const REPLY_WAIT: Duration = Duration::from_secs(2);
/// How often an idle worker looks for a stop request.
const TICK: Duration = Duration::from_secs(1);

/// What a worker answers.
pub enum Reply {
    /// An asker over UDP, from Guard's listening socket; `limit` is the
    /// longest answer it takes (wire::udp_limit).
    Udp {
        sock: Arc<UdpSocket>,
        client: SocketAddr,
        limit: usize,
    },
    /// An asker over TCP, its connection handed over by the TCP loop.
    Tcp(TcpStream),
}

pub struct Job {
    pub msg: Vec<u8>,
    pub reply: Reply,
    pub deadline: Instant,
}

/// The lookups waiting for a worker.
pub struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        }
    }

    /// Queues a lookup; false (and the lookup dropped) when the queue is full.
    pub fn push(&self, job: Job) -> bool {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if jobs.len() >= QUEUE {
            return false;
        }
        jobs.push_back(job);
        drop(jobs);
        self.ready.notify_one();
        true
    }

    fn pop(&self, wait: Duration) -> Option<Job> {
        let jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        let (mut jobs, _) = self
            .ready
            .wait_timeout_while(jobs, wait, |jobs| jobs.is_empty())
            .unwrap_or_else(|p| p.into_inner());
        jobs.pop_front()
    }
}

/// The TLS client: rustls's configuration, the name to check, the servers.
pub struct Tls {
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
    pub servers: Vec<SocketAddrV4>,
    /// TLS connections open now, for the status file.
    pub open: AtomicUsize,
}

impl Tls {
    /// TLS 1.3 only, checked against Mozilla's roots (webpki-roots), or
    /// against the one DER certificate in `--tls-ca` instead.
    pub fn new(config: &Config) -> Result<Self, String> {
        let mut roots = RootCertStore::empty();
        match &config.tls_ca {
            Some(path) => {
                let der = fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
                roots
                    .add(CertificateDer::from(der))
                    .map_err(|err| format!("{}: {err}", path.display()))?;
            }
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let client = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|err| err.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = ServerName::try_from(config.tls_name.clone())
            .map_err(|err| format!("--tls-name {}: {err}", config.tls_name))?;
        Ok(Self {
            config: Arc::new(client),
            name,
            servers: config.tls_servers.clone(),
            open: AtomicUsize::new(0),
        })
    }
}

/// One open TLS connection; closing it says so to the server (close_notify).
struct Conn<'g> {
    stream: StreamOwned<ClientConnection, TcpStream>,
    last: Instant,
    open: &'g AtomicUsize,
}

impl Drop for Conn<'_> {
    fn drop(&mut self) {
        self.open.fetch_sub(1, Ordering::Relaxed);
        self.stream.conn.send_close_notify();
        let _ = self
            .stream
            .sock
            .set_write_timeout(Some(Duration::from_millis(100)));
        let _ = self.stream.conn.write_tls(&mut self.stream.sock);
    }
}

/// A worker: takes lookups off the queue until Guard stops.
pub fn work(guard: &Guard) {
    let mut conn: Option<Conn<'_>> = None;
    let mut clear_errors = Throttle::new("clear fallback");
    while !sys::stopping() {
        let job = guard.queue.pop(TICK);
        if conn.as_ref().is_some_and(|c| c.last.elapsed() >= IDLE) {
            conn = None;
        }
        if let Some(job) = job {
            serve(guard, &mut conn, job, &mut clear_errors);
        }
    }
}

/// How a TLS exchange ended without an answer.
enum Outcome {
    /// The deadline passed: nothing goes back, as with a silent router.
    Late,
    /// No time left for a new connection: the lookup goes in clear, and the
    /// server isn't blamed.
    NoTime,
    /// The server failed: noted, and the lookup goes in clear.
    Failed(usize, Failure),
}

fn serve<'g>(guard: &'g Guard, conn: &mut Option<Conn<'g>>, job: Job, clear_errors: &mut Throttle) {
    if Instant::now() >= job.deadline {
        guard.upstream_timeouts(1);
        return;
    }
    // Again here: the clock or the backoff may have changed while it waited.
    let route = guard.route();
    if route == Route::Tls {
        match exchange(guard, conn, &job) {
            Ok(answer) => {
                guard.tls_answered();
                reply(job.reply, &answer, true);
                return;
            }
            Err(Outcome::Late) => {
                guard.upstream_timeouts(1);
                return;
            }
            Err(Outcome::NoTime) => {}
            Err(Outcome::Failed(server, failure)) => guard.tls_failed(server, failure),
        }
        guard.went_clear(Route::Fallback(Why::Down));
    } else {
        guard.went_clear(route);
    }
    match ask_clear(guard, &job) {
        Ok(Some(answer)) => reply(job.reply, &answer, false),
        Ok(None) => guard.upstream_timeouts(1),
        Err(err) => {
            guard.upstream_error();
            clear_errors.warn(&err);
        }
    }
}

/// Asks over the open connection, or a new one; the answer, unchanged.
fn exchange<'g>(
    guard: &'g Guard,
    slot: &mut Option<Conn<'g>>,
    job: &Job,
) -> Result<Vec<u8>, Outcome> {
    if let Some(conn) = slot.as_mut() {
        match ask(&mut conn.stream, &job.msg, job.deadline) {
            Ok(answer) => {
                conn.last = Instant::now();
                return Ok(answer);
            }
            Err(Asked::Late) => {
                *slot = None;
                return Err(Outcome::Late);
            }
            // Closed by the server since its last use, most likely: once more
            // on a new connection.
            Err(Asked::Broken(_)) => *slot = None,
        }
    }
    let tls = &guard.tls;
    let server = guard.tls_server();
    let addr = tls.servers[server];
    let now = Instant::now();
    let by = job
        .deadline
        .checked_sub(CLEAR_RESERVE)
        .unwrap_or(now)
        .min(now + HANDSHAKE);
    if by <= now + Duration::from_millis(50) {
        return Err(Outcome::NoTime);
    }
    let failed = |text: String, clock: bool| Outcome::Failed(server, Failure { text, clock });
    let sock = connect(addr, guard.config.dscp, by - now)
        .map_err(|err| failed(format!("connect {addr}: {err}"), false))?;
    let mut tls_conn = ClientConnection::new(Arc::clone(&tls.config), tls.name.clone())
        .map_err(|err| failed(format!("{addr}: {err}"), false))?;
    let mut sock = sock;
    while tls_conn.is_handshaking() {
        let left = by.saturating_duration_since(Instant::now());
        let step = set_timeouts(&sock, left).and_then(|()| tls_conn.complete_io(&mut sock));
        if let Err(err) = step {
            let clock = refused_by_clock(&err);
            let text = if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
                format!(
                    "handshake with {addr}: no answer within {} ms",
                    HANDSHAKE.as_millis()
                )
            } else {
                format!("handshake with {addr} as {}: {err}", guard.config.tls_name)
            };
            return Err(failed(text, clock));
        }
    }
    guard.tls_handshake();
    tls.open.fetch_add(1, Ordering::Relaxed);
    let conn = slot.insert(Conn {
        stream: StreamOwned::new(tls_conn, sock),
        last: Instant::now(),
        open: &tls.open,
    });
    match ask(&mut conn.stream, &job.msg, job.deadline) {
        Ok(answer) => Ok(answer),
        Err(Asked::Late) => {
            *slot = None;
            Err(Outcome::Late)
        }
        Err(Asked::Broken(err)) => {
            *slot = None;
            Err(failed(format!("{addr}: {err}"), false))
        }
    }
}

enum Asked {
    Late,
    Broken(io::Error),
}

/// One query and its answer over a TLS connection, with RFC 7858's two-byte
/// length on both. The answer must carry the query's ID.
fn ask(
    stream: &mut StreamOwned<ClientConnection, TcpStream>,
    msg: &[u8],
    deadline: Instant,
) -> Result<Vec<u8>, Asked> {
    let classify = |err: io::Error| match err.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => Asked::Late,
        _ => Asked::Broken(err),
    };
    let left = || deadline.saturating_duration_since(Instant::now());
    if left().is_zero() {
        return Err(Asked::Late);
    }
    let mut framed = Vec::with_capacity(2 + msg.len());
    framed.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    framed.extend_from_slice(msg);
    set_timeouts(&stream.sock, left()).map_err(Asked::Broken)?;
    stream.write_all(&framed).map_err(classify)?;
    stream.flush().map_err(classify)?;
    let mut len = [0; 2];
    set_timeouts(&stream.sock, left()).map_err(classify)?;
    stream.read_exact(&mut len).map_err(classify)?;
    let mut answer = vec![0; usize::from(u16::from_be_bytes(len))];
    set_timeouts(&stream.sock, left()).map_err(classify)?;
    stream.read_exact(&mut answer).map_err(classify)?;
    if wire::id(&answer) != wire::id(msg) || wire::is_query(&answer) {
        return Err(Asked::Broken(io::Error::new(
            ErrorKind::InvalidData,
            "an answer with another ID",
        )));
    }
    Ok(answer)
}

/// Read and write timeouts on a blocking socket; `Duration::ZERO` would
/// mean none, so the least is a millisecond.
fn set_timeouts(sock: &TcpStream, left: Duration) -> io::Result<()> {
    let left = left.max(Duration::from_millis(1));
    sock.set_read_timeout(Some(left))?;
    sock.set_write_timeout(Some(left))
}

/// Whether a failed handshake was a certificate the clock made invalid.
fn refused_by_clock(err: &io::Error) -> bool {
    let Some(inner) = err.get_ref() else {
        return false;
    };
    matches!(
        inner.downcast_ref::<rustls::Error>(),
        Some(rustls::Error::InvalidCertificate(
            CertificateError::Expired
                | CertificateError::ExpiredContext { .. }
                | CertificateError::NotValidYet
                | CertificateError::NotValidYetContext { .. }
        ))
    )
}

/// A blocking TCP connection to `addr`, marked with `dscp` from its SYN, made
/// within `wait`.
fn connect(addr: SocketAddrV4, dscp: u8, wait: Duration) -> io::Result<TcpStream> {
    let stream = sys::connect_marked(addr, dscp)?;
    let mut poller = Poller::default();
    poller.add(stream.as_fd(), Interest::Write);
    poller.wait(wait)?;
    if !poller.ready(0) {
        return Err(io::Error::new(ErrorKind::TimedOut, "timed out"));
    }
    if let Some(err) = stream.take_error()? {
        return Err(err);
    }
    stream.peer_addr()?;
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// The fallback: the router, in clear, over the asker's own protocol. The
/// answer, or `None` when none came before the deadline.
fn ask_clear(guard: &Guard, job: &Job) -> io::Result<Option<Vec<u8>>> {
    let upstream = guard.config.upstream;
    let left = || job.deadline.saturating_duration_since(Instant::now());
    let late = |err: &io::Error| matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut);
    match job.reply {
        Reply::Udp { .. } => {
            let sock = UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))?;
            if guard.config.dscp != 0 {
                sys::set_dscp(sock.as_fd(), guard.config.dscp)?;
            }
            sock.connect(upstream)?;
            sock.send(&job.msg)?;
            let mut buf = vec![0; wire::MAX_MESSAGE + 1];
            loop {
                if left().is_zero() {
                    return Ok(None);
                }
                sock.set_read_timeout(Some(left()))?;
                match sock.recv(&mut buf) {
                    Ok(n) if n > wire::MAX_MESSAGE => {
                        return Err(io::Error::other("an answer over 4,096 bytes"));
                    }
                    Ok(n)
                        if wire::id(&buf[..n]) == wire::id(&job.msg)
                            && !wire::is_query(&buf[..n]) =>
                    {
                        buf.truncate(n);
                        return Ok(Some(buf));
                    }
                    Ok(_) => {}
                    Err(err) if late(&err) => return Ok(None),
                    Err(err) => return Err(err),
                }
            }
        }
        Reply::Tcp(_) => {
            let result = (|| {
                let mut sock = connect(upstream, guard.config.dscp, left())?;
                let mut framed = (job.msg.len() as u16).to_be_bytes().to_vec();
                framed.extend_from_slice(&job.msg);
                set_timeouts(&sock, left())?;
                sock.write_all(&framed)?;
                let mut len = [0; 2];
                set_timeouts(&sock, left())?;
                sock.read_exact(&mut len)?;
                let mut answer = vec![0; usize::from(u16::from_be_bytes(len))];
                set_timeouts(&sock, left())?;
                sock.read_exact(&mut answer)?;
                Ok(answer)
            })();
            match result {
                Ok(answer) => Ok(Some(answer)),
                Err(err) if late(&err) => Ok(None),
                Err(err) => Err(err),
            }
        }
    }
}

/// Sends the answer to the asker. A TLS answer too long for a UDP asker goes
/// back truncated, so it asks again over TCP; the router's answers have
/// already been cut to fit.
fn reply(to: Reply, answer: &[u8], over_tls: bool) {
    match to {
        Reply::Udp {
            sock,
            client,
            limit,
        } => {
            let out = if over_tls && answer.len() > limit {
                wire::truncated(answer)
            } else {
                answer.to_vec()
            };
            // The asker may be gone; that is its business.
            let _ = sock.send_to(&out, client);
        }
        Reply::Tcp(mut stream) => {
            let mut framed = (answer.len() as u16).to_be_bytes().to_vec();
            framed.extend_from_slice(answer);
            let _ = stream
                .set_nonblocking(false)
                .and_then(|()| stream.set_write_timeout(Some(REPLY_WAIT)))
                .and_then(|()| stream.write_all(&framed));
        }
    }
}
