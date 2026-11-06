//! lumend's log: `/var/lib/lumen/log/lumend.log`, the run before kept as
//! `lumend.log.1`, as the boot hooks keep theirs (CONTRIBUTING.md). Since
//! the logic is no longer readable in a hook, the log says what each part
//! did (decision 14's cost).
//!
//! Each line starts with the UTC time and the seconds since boot: the B9
//! has no clock of its own and starts every boot at 2019-01-01 until LG's
//! server sets the time, so only the second number orders a boot's lines
//! for sure. The log goes nowhere when `/var/lib/lumen` or its `log` folder
//! is not root's alone; the work goes on unlogged, as the hooks do.

use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fsat::{Dir, check_private};
use crate::paths::{LOG_DIR_NAME, LOG_NAME, Root, VAR_LUMEN};

/// Lines past this are dropped, and counted in the last one.
const MAX_BYTES: u64 = 4 << 20;
/// The longest line kept; the rest is cut.
const MAX_LINE: usize = 600;

enum Out {
    File {
        file: File,
        written: u64,
        dropped: u64,
    },
    Stdout,
    Nowhere,
}

static OUT: OnceLock<Mutex<Out>> = OnceLock::new();

/// `say!("...", ...)`: one line in the log.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::log::line(&format!($($arg)*))
    };
}

/// Log lines to stdout: `lumend try`, `undo`, `check` and the tests.
pub fn to_stdout() {
    let _ = OUT.set(Mutex::new(Out::Stdout));
}

/// Opens this run's log file, after the run before's is moved to `.1`.
/// On failure the log goes nowhere and the reason comes back.
pub fn to_file(root: &Root) -> Result<(), String> {
    match open(root) {
        Ok(file) => {
            let _ = OUT.set(Mutex::new(Out::File {
                file,
                written: 0,
                dropped: 0,
            }));
            Ok(())
        }
        Err(e) => {
            let _ = OUT.set(Mutex::new(Out::Nowhere));
            Err(e)
        }
    }
}

fn open(root: &Root) -> Result<File, String> {
    let me = crate::sys::os::euid();
    let parent = root.path(VAR_LUMEN);
    let parent_name = parent.file_name().unwrap_or(OsStr::new("lumen")).to_owned();
    let above = Dir::open_path(parent.parent().ok_or("no folder above /var/lib/lumen")?)
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    if above
        .stat_opt(&parent_name)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        above
            .mkdir(&parent_name, 0o755)
            .map_err(|e| format!("making {VAR_LUMEN}: {e}"))?;
    }
    let lumen = above
        .open_dir(&parent_name)
        .map_err(|e| format!("{VAR_LUMEN}: {e}"))?;
    check_private(&lumen.meta().map_err(|e| e.to_string())?, me, VAR_LUMEN)?;
    let log_dir = OsStr::new(LOG_DIR_NAME);
    if lumen
        .stat_opt(log_dir)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        lumen
            .mkdir(log_dir, 0o700)
            .map_err(|e| format!("making the log folder: {e}"))?;
    }
    let logs = lumen
        .open_dir(log_dir)
        .map_err(|e| format!("the log folder: {e}"))?;
    check_private(
        &logs.meta().map_err(|e| e.to_string())?,
        me,
        "the log folder",
    )?;
    let name = OsStr::new(LOG_NAME);
    let old = format!("{LOG_NAME}.1");
    if let Some(meta) = logs.stat_opt(name).map_err(|e| e.to_string())? {
        if !meta.is_file() {
            return Err(format!("{LOG_NAME} is not a plain file"));
        }
        logs.rename(name, OsStr::new(&old))
            .map_err(|e| format!("keeping the last run's log: {e}"))?;
    }
    logs.create(name, 0o600)
        .map_err(|e| format!("making {LOG_NAME}: {e}"))
}

/// Seconds since the TV booted, from `/proc/uptime`.
pub fn uptime() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/uptime").ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// `secs` since 1970 as `YYYY-MM-DDTHH:MM:SSZ` (Howard Hinnant's
/// civil-from-days).
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// The current time as a log line starts it.
pub fn stamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    match uptime() {
        Some(up) => format!("{} +{up:.1}s", utc(now)),
        None => utc(now),
    }
}

/// `text` on one line of printable characters, cut at [`MAX_LINE`].
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(MAX_LINE)
        .collect()
}

/// One line, with its stamp.
pub fn line(text: &str) {
    let line = format!("{} {}\n", stamp(), clean(text));
    let Some(out) = OUT.get() else {
        return;
    };
    let mut out = out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &mut *out {
        Out::File {
            file,
            written,
            dropped,
        } => {
            if *written + line.len() as u64 > MAX_BYTES {
                *dropped += 1;
                return;
            }
            // One write a line, so a crash never leaves half of one behind
            // another.
            if file.write_all(line.as_bytes()).is_ok() {
                *written += line.len() as u64;
            } else {
                *dropped += 1;
            }
        }
        Out::Stdout => {
            let _ = std::io::stdout().write_all(line.as_bytes());
        }
        Out::Nowhere => {}
    }
}

/// The last line of a run: how many lines the cap dropped, if any.
pub fn close() {
    let Some(out) = OUT.get() else {
        return;
    };
    let mut out = out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Out::File { file, dropped, .. } = &mut *out {
        if *dropped > 0 {
            let _ = writeln!(
                file,
                "{} {dropped} lines dropped: the log was full",
                stamp()
            );
        }
        let _ = file.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        // The B9's clock at boot, before LG's server sets it.
        assert_eq!(utc(1_546_300_800), "2019-01-01T00:00:00Z");
        assert_eq!(utc(1_793_939_385), "2026-11-06T04:29:45Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn cleans_control_characters_and_cuts() {
        assert_eq!(clean("a\nb\tc\u{1b}[31m"), "a?b?c?[31m");
        assert_eq!(clean(&"x".repeat(2_000)).len(), MAX_LINE);
    }

    #[test]
    fn opens_the_log_in_a_private_folder_and_keeps_the_last_run() {
        let base = crate::fsat::scratch("log");
        let root = Root::at(&base);
        std::fs::create_dir_all(root.path("/var/lib")).unwrap();
        let first = open(&root).unwrap();
        drop(first);
        std::fs::write(root.path("/var/lib/lumen/log/lumend.log"), b"run one\n").unwrap();
        let _second = open(&root).unwrap();
        assert_eq!(
            std::fs::read(root.path("/var/lib/lumen/log/lumend.log.1")).unwrap(),
            b"run one\n"
        );
        // A link where the log was: refused, never followed.
        std::fs::remove_file(root.path("/var/lib/lumen/log/lumend.log")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.path("/var/lib/lumen/log/lumend.log"))
            .unwrap();
        assert!(open(&root).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn refuses_a_log_folder_others_can_write() {
        use std::os::unix::fs::PermissionsExt;
        let base = crate::fsat::scratch("log-shared");
        let root = Root::at(&base);
        std::fs::create_dir_all(root.path("/var/lib/lumen/log")).unwrap();
        std::fs::set_permissions(
            root.path("/var/lib/lumen/log"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        assert!(open(&root).unwrap_err().contains("writable by others"));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
