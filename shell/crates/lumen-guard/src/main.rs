//! Lumen Guard: a small DNS forwarder that runs as root on the TV, sees every
//! lookup the TV makes, and logs each one with the verdict the block rules
//! would give. This is stage G1 of docs/specs/privacy.md: it answers nothing
//! itself and forwards everything, unchanged.
//!
//! Two threads: UDP (and the status file) on the main thread, TCP on another.
//! The boot hook (profiles/privacy/lumen-guard) starts it, sends the TV's DNS
//! to it with a nat rule once it answers, and removes the rule when it exits.

// Only `sys` may use `unsafe`; `forbid` can't be lifted for one module, so the
// rest of the crate is held to it with `deny`.
#![deny(unsafe_code)]

mod args;
mod guard;
mod lookups;
mod rules;
mod status;
#[allow(unsafe_code)]
mod sys;
mod tcp;
mod udp;
mod wire;

use std::fs;
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, UdpSocket};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::args::{Command, Config};
use crate::guard::Guard;
use crate::rules::Rules;
use crate::wire::MAX_MESSAGE;

const PROBE_WAIT: Duration = Duration::from_secs(2);
/// The TCP thread needs little: its buffers are on the heap.
const TCP_STACK: usize = 256 << 10;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&argv) {
        Ok(Command::Run(config)) => run(config),
        Ok(Command::Probe { addr, name }) => probe(addr, &name),
        Ok(Command::Help) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("lumen-guard: {msg}\n\n{}", args::USAGE);
            ExitCode::from(2)
        }
    }
}

fn run(config: Config) -> ExitCode {
    let (rules, warnings) = Rules::load(&config.rules);
    for warning in warnings {
        say!("no rules from {warning}; counted as empty");
    }
    let _ = fs::create_dir_all(&config.log);
    let sockets = UdpSocket::bind(config.listen).and_then(|udp| {
        let tcp = TcpListener::bind(config.listen)?;
        udp.set_nonblocking(true)?;
        tcp.set_nonblocking(true)?;
        Ok((udp, tcp))
    });
    let (udp, tcp) = match sockets {
        Ok(sockets) => sockets,
        Err(err) => {
            say!("cannot listen on {}: {err}", config.listen);
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = sys::catch_stop_signals() {
        say!("cannot catch SIGTERM: {err}");
        return ExitCode::FAILURE;
    }

    let guard = Guard::new(config, rules);
    let (allow, exact, block) = guard.rules().counts();
    say!(
        "{} listening on {} (udp, tcp), upstream {}, dscp {}, mode log; rules: {allow} allow, {exact} exact allow, {block} block",
        env!("CARGO_PKG_VERSION"),
        guard.config.listen,
        guard.config.upstream,
        guard.config.dscp,
    );

    // Either loop failing ends both, so the hook removes the nat rule.
    let result = thread::scope(|scope| {
        let tcp_thread = thread::Builder::new()
            .name("tcp".to_owned())
            .stack_size(TCP_STACK)
            .spawn_scoped(scope, || {
                let result = tcp::serve(&guard, &tcp);
                sys::request_stop();
                result
            })?;
        let udp_result = udp::serve(&guard, &udp);
        sys::request_stop();
        let tcp_result = tcp_thread
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("the tcp thread panicked")));
        udp_result.and(tcp_result)
    });

    if let Err(err) = guard.write_status() {
        say!("status file: {err}");
    }
    match result {
        Ok(()) => {
            say!("stopped");
            ExitCode::SUCCESS
        }
        Err(err) => {
            say!("stopped by an error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Asks `addr` for `name` once; any answer with the query's ID counts.
fn probe(addr: SocketAddrV4, name: &str) -> ExitCode {
    match ask(addr, name) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!(
                "lumen-guard: no answer from {addr} within {} s",
                PROBE_WAIT.as_secs()
            );
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("lumen-guard: probe of {addr}: {err}");
            ExitCode::FAILURE
        }
    }
}

fn ask(addr: SocketAddrV4, name: &str) -> io::Result<bool> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    sock.connect(addr)?;
    // Not secret, just unlikely to repeat between probes.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let id = (nanos as u16) ^ (std::process::id() as u16);
    sock.send(&wire::query(id, name, 1))?;
    let deadline = Instant::now() + PROBE_WAIT;
    let mut buf = [0; MAX_MESSAGE + 1];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        sock.set_read_timeout(Some(left))?;
        match sock.recv(&mut buf) {
            Ok(n)
                if wire::id(&buf[..n]) == Some(id.to_be_bytes()) && !wire::is_query(&buf[..n]) =>
            {
                return Ok(n >= wire::HEADER);
            }
            Ok(_) => {}
            Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Ok(false);
            }
            Err(err) => return Err(err),
        }
    }
}
