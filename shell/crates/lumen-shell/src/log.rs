//! Minimal logger. Every line goes to stderr and, when it could be opened, to a
//! log file, since a native app launched on the TV has no console to read.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: Mutex<Option<File>> = Mutex::new(None);

/// Opens (appending) the log file. A path that cannot be opened leaves stderr
/// as the only output rather than failing the app.
pub fn init(path: &str) {
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    if let Ok(mut sink) = SINK.lock() {
        *sink = file;
    }
}

pub fn write(args: std::fmt::Arguments<'_>) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let line = format!("[{secs:.3}] {args}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Ok(mut sink) = SINK.lock()
        && let Some(file) = sink.as_mut()
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// `info!("fps {fps:.1}")` writes one timestamped line to stderr and the log file.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}
