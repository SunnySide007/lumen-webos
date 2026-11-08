//! The app's own log, `~/.lumen/remote/app.log`: starts, links opening and
//! closing with ssh's reason, Wake-on-LAN, and the popup's timing line. It
//! stays on the PC; nothing reads it but the person. Moved to `.1` at
//! 256 KiB.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const ROTATE_AT: u64 = 256 << 10;

#[derive(Clone)]
pub struct AppLog {
    path: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl AppLog {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path: Arc::new(path),
            lock: Arc::default(),
        }
    }

    pub fn line(&self, text: &str) {
        let _held = crate::lock(&self.lock);
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(&*self.path).is_ok_and(|m| m.len() > ROTATE_AT) {
            let _ = std::fs::rename(&*self.path, self.path.with_extension("log.1"));
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&*self.path)
        {
            let _ = writeln!(f, "{secs:.3} {text}");
        }
        if cfg!(debug_assertions) {
            eprintln!("lumen-remote: {text}");
        }
    }
}
