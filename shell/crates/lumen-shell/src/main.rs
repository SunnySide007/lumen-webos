//! Lumen shell: a native home screen for rooted LG webOS TVs.
//!
//! Phase 0 ships `hello-gles`: it proves the toolchain, SDL2 loading, a GLES2
//! context and the frame budget on the real set before any UI is built on top.

mod app;
mod capture;
mod compat;
mod font;
mod frame_stats;
mod gl;
mod log;
mod render;
mod sdl;

use std::process::ExitCode;

/// Where the log goes when `LUMEN_LOG` is not set. A native app on the TV has
/// no console, so the file is the only record of a run there.
fn default_log_path() -> &'static str {
    if cfg!(target_os = "linux") {
        "/tmp/lumen-shell.log"
    } else {
        "lumen-shell.log"
    }
}

fn main() -> ExitCode {
    let log_path = std::env::var("LUMEN_LOG").unwrap_or_else(|_| default_log_path().to_owned());
    log::init(&log_path);

    let args: Vec<String> = std::env::args().skip(1).collect();
    info!(
        "lumen-shell {} starting, args {:?}",
        env!("CARGO_PKG_VERSION"),
        args
    );

    match app::run(app::Options::from_args(&args)) {
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
