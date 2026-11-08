//! lumen-remote: Lumen Remote's door for the owner's phone (docs/specs/
//! remote.md, R4). It serves the web front-end and the protocol's
//! WebSocket over HTTPS on one LAN port, to paired devices only, and
//! passes their requests to `lumen-ctl serve --link web`, its child.
//!
//! In order, before it reads anything from the network:
//!
//! 1. reads its certificate, its key and the whole front-end into memory;
//! 2. binds its port;
//! 3. starts lumen-ctl as root, over a socket pair (its stdin and stdout);
//! 4. changes root into an empty folder and becomes an unprivileged user
//!    with no groups, for good (`sys::drop_root`).
//!
//! Then one thread serves everything from `poll` (`server.rs`). When
//! lumen-ctl ends, lumen-remote ends; when lumen-remote ends, lumen-ctl
//! reads the end of its stdin and ends too.

#![deny(unsafe_code)]

mod args;
mod http;
mod hub;
mod log;
mod server;
#[allow(unsafe_code)]
mod sys;
mod tls;
mod web;
mod ws;

use std::fs::{self, DirBuilder};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use crate::args::{Args, Parsed};
use crate::log::Log;

/// A file root wrote and others can't change: when running as root, the
/// certificate, the key and lumen-ctl must be root's, not writable by
/// others, and not a link; the key not readable by others either.
fn read_root_file(path: &Path, secret: bool, max: u64) -> Result<Vec<u8>, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    check_owned(path, &meta, secret)?;
    if meta.len() > max {
        return Err(format!("{} is over {max} bytes", path.display()));
    }
    let mut buf = Vec::new();
    file.take(max)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(buf)
}

fn check_owned(path: &Path, meta: &fs::Metadata, secret: bool) -> Result<(), String> {
    if !meta.is_file() {
        return Err(format!("{} is not a plain file", path.display()));
    }
    if sys::euid() == 0 {
        let bad = if secret { 0o077 } else { 0o022 };
        if meta.uid() != 0 || meta.mode() & bad != 0 {
            return Err(format!(
                "{} must be root's and mode {} at most",
                path.display(),
                if secret { "0600" } else { "0755" }
            ));
        }
    }
    Ok(())
}

/// The empty folder to change root into, made if it isn't there.
fn jail(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_err() {
        DirBuilder::new()
            .mode(0o711)
            .create(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{} must be a folder only root can write",
            path.display()
        ));
    }
    if fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .next()
        .is_some()
    {
        return Err(format!("{} must be empty", path.display()));
    }
    Ok(())
}

/// lumen-ctl as a child, its stdin and stdout one end of a socket pair.
/// It gets no environment but a PATH, and no other descriptor (std opens
/// every one close-on-exec).
fn start_ctl(a: &Args) -> Result<UnixStream, String> {
    let meta = fs::symlink_metadata(&a.ctl).map_err(|e| format!("{}: {e}", a.ctl.display()))?;
    check_owned(&a.ctl, &meta, false)?;
    let (mine, theirs) = UnixStream::pair().map_err(|e| format!("socketpair: {e}"))?;
    let out = theirs.try_clone().map_err(|e| format!("socketpair: {e}"))?;
    Command::new(&a.ctl)
        .args(["serve", "--link", "web"])
        .args(&a.ctl_args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .stdout(Stdio::from(OwnedFd::from(out)))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", a.ctl.display()))?;
    Ok(mine)
}

fn run(a: Args) -> Result<u8, String> {
    sys::read_auxv();
    let root = sys::euid() == 0;
    if root && a.no_drop {
        return Err(
            "--no-drop is for tests as a user; as root lumen-remote always drops".to_owned(),
        );
    }
    if !root && !a.no_drop {
        return Err("not root: it can't drop root, so it needs --no-drop (tests only)".to_owned());
    }
    let mut log = Log::open(&a.log);
    let cert = read_root_file(&a.cert, false, 64 << 10)?;
    let key = read_root_file(&a.key, true, 16 << 10)?;
    let tls = tls::config(&cert, &key)?;
    let site = web::Site::load(&a.web)?;
    if root {
        jail(&a.jail)?;
    }
    let listener = TcpListener::bind(a.listen).map_err(|e| format!("{}: {e}", a.listen))?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;
    let ctl = start_ctl(&a)?;
    if root {
        sys::drop_root(&a.jail, a.user.0, a.user.1)?;
    } else {
        sys::no_new_privs()?;
    }
    sys::catch_signals().map_err(|e| format!("signals: {e}"))?;
    log.line(&format!(
        "start: lumen-remote {} on {bound} for {} from {}, {} files, {}",
        env!("CARGO_PKG_VERSION"),
        a.names.join(", "),
        a.allow
            .iter()
            .map(|n| format!("{}/{}", std::net::Ipv4Addr::from(n.addr), n.bits))
            .collect::<Vec<_>>()
            .join(", "),
        site.len(),
        if root {
            format!("as {}:{} in {}", a.user.0, a.user.1, a.jail.display())
        } else {
            "not dropping (--no-drop)".to_owned()
        }
    ));
    // The one line on stdout: where it listens, for whoever started it.
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "lumen-remote: listening on {bound}");
    let _ = stdout.flush();
    let cfg = server::Config {
        names: a.names.clone(),
        port: bound.port(),
        allow: a.allow.clone(),
        tls,
        site,
        pair_csp: web::pair_csp(),
    };
    Ok(server::run(cfg, listener, ctl, &mut log))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&argv) {
        Ok(Parsed::Help) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Parsed::Run(a)) => match run(*a) {
            Ok(code) => ExitCode::from(code),
            Err(e) => {
                eprintln!("lumen-remote: {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("lumen-remote: {e}\n\n{}", args::USAGE);
            ExitCode::from(2)
        }
    }
}
