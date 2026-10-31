//! The TCP side, on its own thread: one query per connection (RFC 1035's
//! two-byte length, then the message), at most 32 connections at once. The
//! asker has 3 s to send its query, and the upstream 3 s to answer it; the
//! answer streams back unchanged, a buffer of up to 4,096 bytes at a time, so
//! answers longer than that still pass.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use crate::guard::{Guard, Throttle};
use crate::sys::{self, Interest, Poller};
use crate::udp::TICK;
use crate::wire::{self, HEADER, MAX_MESSAGE};

const MAX_CONNECTIONS: usize = 32;
const DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Reading the length and the query from the asker.
    Query,
    /// Waiting for the upstream connection.
    Connect,
    /// Writing the query, with its length, upstream.
    Send,
    /// Copying the answer from the upstream to the asker.
    Answer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Client,
    Upstream,
}

enum Step {
    Wait(Side, Interest),
    Close,
}

struct Conn {
    client: TcpStream,
    upstream: Option<TcpStream>,
    stage: Stage,
    /// The query while it is read and sent; then each piece of the answer.
    buf: Vec<u8>,
    /// How much of `buf` has been written on.
    sent: usize,
    /// Answer bytes read from the upstream, its length prefix included.
    relayed: usize,
    /// The answer's length with its prefix, once both prefix bytes are in.
    total: Option<usize>,
    prefix: [u8; 2],
    deadline: Instant,
    waiting: (Side, Interest),
}

impl Conn {
    fn new(client: TcpStream) -> Self {
        Self {
            client,
            upstream: None,
            stage: Stage::Query,
            buf: Vec::new(),
            sent: 0,
            relayed: 0,
            total: None,
            prefix: [0; 2],
            deadline: Instant::now() + DEADLINE,
            waiting: (Side::Client, Interest::Read),
        }
    }

    fn stream(&self, side: Side) -> Option<&TcpStream> {
        match side {
            Side::Client => Some(&self.client),
            Side::Upstream => self.upstream.as_ref(),
        }
    }

    /// Does everything it can without blocking; says what to wait for next.
    fn advance(&mut self, guard: &Guard) -> Step {
        loop {
            match self.stage {
                Stage::Query => {
                    let need = match self.buf.get(..2) {
                        Some(len) => 2 + usize::from(u16::from_be_bytes([len[0], len[1]])),
                        None => 2,
                    };
                    if self.buf.len() >= 2 && !(HEADER..=MAX_MESSAGE).contains(&(need - 2)) {
                        return Step::Close;
                    }
                    if self.buf.len() == need && need > 2 {
                        let msg = &self.buf[2..];
                        if !wire::is_query(msg) {
                            return Step::Close;
                        }
                        guard.record(msg);
                        match sys::connect_marked(guard.config.upstream, guard.config.dscp) {
                            Ok(stream) => self.upstream = Some(stream),
                            Err(_) => {
                                guard.upstream_error();
                                return Step::Close;
                            }
                        }
                        self.stage = Stage::Connect;
                        self.deadline = Instant::now() + DEADLINE;
                        continue;
                    }
                    let want = need - self.buf.len();
                    match read_into(&mut self.client, &mut self.buf, want) {
                        Ok(0) => return Step::Close,
                        Ok(_) => {}
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            return Step::Wait(Side::Client, Interest::Read);
                        }
                        Err(_) => return Step::Close,
                    }
                }
                Stage::Connect => {
                    let Some(upstream) = &self.upstream else {
                        return Step::Close;
                    };
                    // A failed connect leaves its error here; one still under
                    // way has no peer yet.
                    if !matches!(upstream.take_error(), Ok(None)) {
                        guard.upstream_error();
                        return Step::Close;
                    }
                    match upstream.peer_addr() {
                        Ok(_) => {
                            self.stage = Stage::Send;
                            self.sent = 0;
                        }
                        Err(err) if err.kind() == ErrorKind::NotConnected => {
                            return Step::Wait(Side::Upstream, Interest::Write);
                        }
                        Err(_) => {
                            guard.upstream_error();
                            return Step::Close;
                        }
                    }
                }
                Stage::Send => {
                    let Some(upstream) = self.upstream.as_mut() else {
                        return Step::Close;
                    };
                    match upstream.write(&self.buf[self.sent..]) {
                        Ok(n) => {
                            self.sent += n;
                            if self.sent == self.buf.len() {
                                self.stage = Stage::Answer;
                                self.buf.clear();
                                self.sent = 0;
                            }
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            return Step::Wait(Side::Upstream, Interest::Write);
                        }
                        Err(_) => {
                            guard.upstream_error();
                            return Step::Close;
                        }
                    }
                }
                Stage::Answer => {
                    if self.sent < self.buf.len() {
                        match self.client.write(&self.buf[self.sent..]) {
                            Ok(n) => self.sent += n,
                            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                                return Step::Wait(Side::Client, Interest::Write);
                            }
                            Err(_) => return Step::Close,
                        }
                        continue;
                    }
                    if self.total == Some(self.relayed) {
                        return Step::Close;
                    }
                    let Some(upstream) = self.upstream.as_mut() else {
                        return Step::Close;
                    };
                    // The prefix first, alone, so its two bytes are known
                    // before any of the answer is passed on.
                    let want = match self.total {
                        Some(total) => (total - self.relayed).min(MAX_MESSAGE),
                        None => 2 - self.relayed,
                    };
                    self.buf.clear();
                    self.sent = 0;
                    match read_into(upstream, &mut self.buf, want) {
                        Ok(0) => {
                            guard.upstream_error();
                            return Step::Close;
                        }
                        Ok(n) => {
                            if self.total.is_none() {
                                self.prefix[self.relayed..self.relayed + n]
                                    .copy_from_slice(&self.buf);
                                if self.relayed + n == 2 {
                                    self.total =
                                        Some(2 + usize::from(u16::from_be_bytes(self.prefix)));
                                }
                            }
                            self.relayed += n;
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            return Step::Wait(Side::Upstream, Interest::Read);
                        }
                        Err(_) => {
                            guard.upstream_error();
                            return Step::Close;
                        }
                    }
                }
            }
        }
    }
}

/// Reads up to `want` more bytes onto the end of `buf`.
fn read_into(stream: &mut TcpStream, buf: &mut Vec<u8>, want: usize) -> io::Result<usize> {
    let start = buf.len();
    buf.resize(start + want, 0);
    let result = stream.read(&mut buf[start..]);
    buf.truncate(start + result.as_ref().map_or(0, |n| *n));
    result
}

pub fn serve(guard: &Guard, listener: &TcpListener) -> io::Result<()> {
    let mut conns: Vec<Conn> = Vec::new();
    let mut poller = Poller::default();
    let mut accept_errors = Throttle::new("tcp accept");

    while !sys::stopping() {
        let now = Instant::now();
        // Past its deadline: closed with nothing sent back. Only the upstream's
        // slowness counts as a timeout; a slow asker is its own problem.
        let mut upstream_late = 0;
        conns.retain(|c| {
            let alive = c.deadline > now;
            if !alive && c.stage != Stage::Query {
                upstream_late += 1;
            }
            alive
        });
        guard.upstream_timeouts(upstream_late);

        poller.clear();
        let listening = conns.len() < MAX_CONNECTIONS;
        if listening {
            poller.add(listener.as_fd(), Interest::Read);
        }
        let base = usize::from(listening);
        for c in &conns {
            let (side, interest) = c.waiting;
            match c.stream(side) {
                Some(stream) => poller.add(stream.as_fd(), interest),
                // Never the case: a connection waits only on a stream it has.
                None => poller.add(c.client.as_fd(), Interest::Read),
            }
        }
        let wait = conns
            .iter()
            .map(|c| c.deadline.saturating_duration_since(now))
            .min()
            .unwrap_or(TICK)
            .min(TICK);
        poller.wait(wait)?;

        for i in (0..conns.len()).rev() {
            if !poller.ready(base + i) {
                continue;
            }
            match conns[i].advance(guard) {
                Step::Wait(side, interest) => conns[i].waiting = (side, interest),
                Step::Close => {
                    conns.swap_remove(i);
                }
            }
        }

        if listening && poller.ready(0) {
            while conns.len() < MAX_CONNECTIONS {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if stream.set_nonblocking(true).is_err() {
                            continue;
                        }
                        let mut conn = Conn::new(stream);
                        if let Step::Wait(side, interest) = conn.advance(guard) {
                            conn.waiting = (side, interest);
                            conns.push(conn);
                        }
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                    Err(err) => {
                        accept_errors.warn(&err);
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
