//! lumen-ctl: the TV's control binary for Lumen Remote (docs/specs/remote.md,
//! R2). It runs as root, only while an SSH session runs it: the PC's key sits
//! in `authorized_keys` with `restrict,command="…/lumen-ctl serve"`, so that
//! key reaches this and nothing else.
//!
//! It speaks the protocol of `remote/ui/src/protocol.ts` on stdin and stdout:
//! fixed verbs, typed and range-checked arguments, no shell and no paths. A
//! request's values reach luna only after they have been checked and rebuilt
//! into a payload the code owns (`lg.rs`), and LG's state comes back as
//! events (`watch.rs`).

// Only `sys` may use `unsafe`; `forbid` can't be lifted for one module, so the
// rest of the crate is held to it with `deny`.
#![deny(unsafe_code)]

mod args;
mod bus;
mod fake;
mod keys;
mod lg;
mod log;
mod privacy;
mod probe;
mod protocol;
mod serve;
#[allow(unsafe_code)]
mod sys;
mod watch;

use std::io;
use std::process::ExitCode;

use crate::args::Command;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&argv) {
        Ok(Command::Serve(flags)) => {
            let text = privacy::read_owned(&flags.config, Some(sys::os::euid()), 4 << 10)
                .map(|t| String::from_utf8_lossy(&t).into_owned());
            match args::resolve(flags, text.as_deref()) {
                Ok(cfg) => serve::run(cfg),
                Err(msg) => {
                    eprintln!("lumen-ctl: {msg}");
                    ExitCode::from(2)
                }
            }
        }
        Ok(Command::Probe(which)) => {
            probe::probe(which, &mut io::stdout());
            ExitCode::SUCCESS
        }
        Ok(Command::ProbeKeys(which)) => {
            probe::probe_keys(which, &mut io::stdout());
            ExitCode::SUCCESS
        }
        Ok(Command::Help) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("lumen-ctl: {msg}\n\n{}", args::USAGE);
            ExitCode::from(2)
        }
    }
}
