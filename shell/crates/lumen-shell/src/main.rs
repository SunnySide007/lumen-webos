//! Lumen shell: a native home screen for rooted LG webOS TVs.
//!
//! The tvOS-style home grid of docs/specs/shell-mvp.md: SDL2 and GL here, the
//! logic (keys, focus, layout, motion, apps) in `lumen-core`.

mod app;
mod atlas;
mod capture;
mod compat;
mod draw;
mod faults;
mod frame_stats;
mod gl;
mod icons;
mod log;
mod luna;
mod render;
mod sdl;
mod text;
mod timing;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

/// Where the log goes when `LUMEN_LOG` is not set. A native app on the TV has
/// no console, so the file is the only record of a run there.
fn default_log_path() -> &'static str {
    if cfg!(target_os = "linux") {
        "/tmp/lumen-shell.log"
    } else {
        "lumen-shell.log"
    }
}

/// The log, then a fallback beside it for when the log will not open (a file
/// left by another user, say); `tools/tv.sh log` on main shows both.
fn log_paths() -> [PathBuf; 2] {
    let log =
        PathBuf::from(std::env::var("LUMEN_LOG").unwrap_or_else(|_| default_log_path().to_owned()));
    let fallback = log.with_file_name("lumen-shell.fallback.log");
    [log, fallback]
}

fn main() -> ExitCode {
    // Start-up phases count from here (the `start:` log line).
    let started = Instant::now();
    log::init(&log_paths());

    let args: Vec<String> = std::env::args().skip(1).collect();
    info!(
        "lumen-shell {} starting, args {:?}",
        env!("CARGO_PKG_VERSION"),
        args
    );

    let opts = match app::Options::from_args(&args) {
        Ok(opts) => opts,
        Err(err) => {
            info!("bad arguments: {err}");
            return ExitCode::FAILURE;
        }
    };
    match app::run(opts, started) {
        Ok(()) => {
            info!("clean exit");
            ExitCode::SUCCESS
        }
        Err(err) => {
            info!("fatal: {err}");
            ExitCode::FAILURE
        }
    }
}
