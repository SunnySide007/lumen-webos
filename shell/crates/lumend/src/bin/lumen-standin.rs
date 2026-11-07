//! lumen-standin: what runs when the hub starts one of LG's on-demand
//! senders that lumend holds down (senders.rs).
//!
//! lumend bind-mounts this file over the sender's program, so the hub starts
//! it under the program's path. It reads that path from `/proc/self/exe`,
//! finds the sender in lumend's fixed list, registers the sender's bus name
//! and offers no method: every caller gets luna's "unknown method" at once.
//! It calls nothing, opens no file of its own, writes nothing and takes no
//! argument. Run under any other path it exits at once.

#![deny(unsafe_code)]

use std::process::ExitCode;

use lumend::senders::by_program;

fn main() -> ExitCode {
    let Ok(exe) = std::fs::read_link("/proc/self/exe") else {
        return ExitCode::FAILURE;
    };
    let Some(sender) = by_program(&exe) else {
        // Not mounted over a sender: nothing to stand in for.
        return ExitCode::SUCCESS;
    };
    match lumend::sys::luna::hold_name(sender.service) {
        Ok(never) => match never {},
        Err(_) => ExitCode::FAILURE,
    }
}
