//! `privacy.state` (protocol.ts's `PrivacyState`): what Lumen's privacy work
//! did on this boot, read from the places privacy.md puts it.
//!
//! * The boot hook: whether `/var/lib/webosbrew/init.d/lumen-privacy` exists,
//!   and the end of this boot's log, `/var/lib/lumen/log/lumen-privacy.log`.
//! * The rule rejecting tcp 11111: `iptables -L INPUT -n`.
//! * LG's diagnostics jobs the hook stops: `initctl status <job>`.
//! * Lumen Guard's counts: `/var/lib/lumen/guard/status`; `null` before
//!   Guard has run.
//!
//! Each file is read only if it is a regular file owned by root, opened
//! without following a link and without blocking on a pipe someone left in
//! its place: the hooks no longer write into `/tmp`, which the apps' jails
//! share, but the check costs nothing.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::bus::Runner;
use crate::lg;
use crate::sys;

pub const IPTABLES: &str = "/usr/sbin/iptables";
/// upstart's control tool; its path on the B9 is the usual one, unread.
pub const INITCTL: &str = "/sbin/initctl";
/// The jobs `lumen-privacy` stops, in its order.
pub const JOBS: [&str; 4] = ["crashreportd", "rdxd", "uploadd", "remotelogger"];
/// How much of the hook's log is read, and how many of its lines go out.
const LOG_TAIL: u64 = 16 << 10;
const LOG_LINES: usize = 50;
const MAX_STATUS: u64 = 16 << 10;

/// Where each part is; the tests point them into a folder of their own.
pub struct Paths {
    pub hook: PathBuf,
    pub hook_log: PathBuf,
    pub guard_status: PathBuf,
    /// The owner a file must have to be read: root on the TV.
    pub owner: Option<u32>,
}

impl Paths {
    pub fn tv() -> Self {
        Self {
            hook: PathBuf::from("/var/lib/webosbrew/init.d/lumen-privacy"),
            hook_log: PathBuf::from("/var/lib/lumen/log/lumen-privacy.log"),
            guard_status: PathBuf::from("/var/lib/lumen/guard/status"),
            owner: Some(0),
        }
    }
}

/// `path` opened for reading if it is a regular file owned by `owner`: never
/// through a link (`O_NOFOLLOW`), never waiting on a pipe (`O_NONBLOCK`).
fn open_owned(path: &Path, owner: Option<u32>) -> Option<(File, u64)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(sys::os::OPEN_NO_FOLLOW_NO_BLOCK)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    (meta.file_type().is_file() && owner.is_none_or(|o| meta.uid() == o))
        .then_some((file, meta.len()))
}

/// The whole file, if `open_owned` takes it and it is at most `max` bytes.
pub fn read_owned(path: &Path, owner: Option<u32>, max: u64) -> Option<Vec<u8>> {
    let (file, len) = open_owned(path, owner)?;
    if len > max {
        return None;
    }
    let mut text = Vec::new();
    file.take(max + 1).read_to_end(&mut text).ok()?;
    (text.len() as u64 <= max).then_some(text)
}

/// The last `max` bytes of the file, if `open_owned` takes it, and whether
/// they start mid-file.
fn tail_owned(path: &Path, owner: Option<u32>, max: u64) -> Option<(Vec<u8>, bool)> {
    let (mut file, len) = open_owned(path, owner)?;
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut text = Vec::new();
    file.take(max).read_to_end(&mut text).ok()?;
    Some((text, start > 0))
}

/// The hook's last `LOG_LINES` lines, each cleaned for one line out.
fn hook_log(path: &Path, owner: Option<u32>) -> Vec<String> {
    let Some((text, cut)) = tail_owned(path, owner, LOG_TAIL) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&text);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if cut && !lines.is_empty() {
        // The first line was cut by the tail.
        lines.remove(0);
    }
    let lines: Vec<String> = lines
        .iter()
        .map(|l| lg::clean(l, lg::MAX_TEXT))
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(LOG_LINES)..].to_vec()
}

/// Whether `iptables -L INPUT -n` shows a REJECT (or DROP) of tcp 11111.
fn rejects_11111(listing: &str) -> bool {
    listing.lines().any(|l| {
        let target = l.split_whitespace().next().unwrap_or("");
        matches!(target, "REJECT" | "DROP")
            && l.split_whitespace().any(|w| w == "dpt:11111")
            && l.split_whitespace().nth(1) == Some("tcp")
    })
}

/// Whether `initctl status <job>` says the job runs: `start/running`, or on
/// its way there, as the hook itself reads it.
fn job_running(status: &str) -> bool {
    status.contains(" start/")
}

/// Unix seconds as ISO 8601 in UTC (`2026-10-31T06:09:38Z`).
pub fn iso8601(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Howard Hinnant's days-to-civil, for the proleptic Gregorian calendar.
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
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Guard's status file (`key value` lines, lumen-guard's `status.rs`) as
/// `PrivacyState`'s `guard`. G1 only logs, so `blocked` is 0 until G2's
/// `blocked` line exists; `wouldBlock` beside it is what G1 would block.
fn guard(text: &str) -> Option<Value> {
    let field = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            (k == key).then(|| v.trim())
        })
    };
    let number = |key: &str| field(key).and_then(|v| v.parse::<u64>().ok());
    let started = number("started")?;
    let lookups = number("lookups")?;
    let mut g = json!({
        "since": iso8601(started),
        "lookups": lookups,
        "blocked": number("blocked").unwrap_or(0),
    });
    if let Some(n) = number("would_block") {
        g["wouldBlock"] = json!(n);
    }
    if let Some(mode) = field("mode") {
        g["mode"] = json!(lg::clean(mode, 16));
    }
    if let Some(updated) = number("updated") {
        g["updated"] = json!(iso8601(updated));
    }
    Some(g)
}

/// Reads it all. Nothing here fails the verb: a part that can't be read
/// reads as "no", with the reason beside it for the log and the curious.
pub fn read(paths: &Paths, runner: &dyn Runner) -> Value {
    let installed = std::fs::symlink_metadata(&paths.hook).is_ok_and(|m| m.file_type().is_file());
    let mut port = json!({ "rejected": false });
    match runner.run(IPTABLES, &["-L", "INPUT", "-n"]) {
        Ok(out) if out.success => port["rejected"] = json!(rejects_11111(&out.stdout)),
        Ok(out) => port["error"] = json!(lg::clean(&out.stderr, lg::MAX_TEXT)),
        Err(e) => port["error"] = json!(lg::clean(&e, lg::MAX_TEXT)),
    }
    let jobs: Vec<Value> = JOBS
        .iter()
        .map(|&name| {
            let running = runner
                .run(INITCTL, &["status", name])
                .is_ok_and(|out| out.success && job_running(&out.stdout));
            json!({ "name": name, "running": running })
        })
        .collect();
    let guard = read_owned(&paths.guard_status, paths.owner, MAX_STATUS)
        .and_then(|t| guard(&String::from_utf8_lossy(&t)));
    json!({
        "hook": { "installed": installed, "log": hook_log(&paths.hook_log, paths.owner) },
        "port11111": port,
        "jobs": jobs,
        "guard": guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Output;
    use crate::bus::tests::{FakeRunner, printed};
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-ctl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn me(dir: &Path) -> Option<u32> {
        Some(fs::metadata(dir).unwrap().uid())
    }

    const LISTING: &str = "Chain INPUT (policy ACCEPT)\n\
        target     prot opt source               destination\n\
        REJECT     tcp  --  0.0.0.0/0            0.0.0.0/0            tcp dpt:11111 reject-with icmp-port-unreachable\n";

    fn tv_like() -> FakeRunner {
        FakeRunner::new(|program, args| match (program, args) {
            (IPTABLES, ["-L", "INPUT", "-n"]) => printed(LISTING),
            (INITCTL, ["status", "rdxd"]) => printed("rdxd start/running, process 3177\n"),
            (INITCTL, ["status", job]) => printed(&format!("{job} stop/waiting\n")),
            _ => Err(format!("unexpected {program} {args:?}")),
        })
    }

    #[test]
    fn reads_the_state_from_its_files_and_commands() {
        let dir = temp_dir("privacy");
        let paths = Paths {
            hook: dir.join("lumen-privacy"),
            hook_log: dir.join("lumen-privacy.log"),
            guard_status: dir.join("status"),
            owner: me(&dir),
        };
        fs::write(&paths.hook, "#!/bin/sh\n").unwrap();
        fs::write(
            &paths.hook_log,
            "Sat Oct 31 05:00:34 UTC 2026 lumen-privacy: start\ntcp 11111 rejected from the LAN\n\
             Sat Oct 31 05:00:34 UTC 2026 stopped crashreportd\x1b[0m\n\n",
        )
        .unwrap();
        fs::write(
            &paths.guard_status,
            "version 0.0.1\nmode log\nstarted 1793318400\nupdated 1793318410\nlookups 4812\nwould_block 37\n",
        )
        .unwrap();
        let runner = tv_like();
        let state = read(&paths, &runner);
        assert_eq!(
            state,
            json!({
                "hook": { "installed": true, "log": [
                    "Sat Oct 31 05:00:34 UTC 2026 lumen-privacy: start",
                    "tcp 11111 rejected from the LAN",
                    "Sat Oct 31 05:00:34 UTC 2026 stopped crashreportd [0m",
                ] },
                "port11111": { "rejected": true },
                "jobs": [
                    { "name": "crashreportd", "running": false },
                    { "name": "rdxd", "running": true },
                    { "name": "uploadd", "running": false },
                    { "name": "remotelogger", "running": false },
                ],
                "guard": {
                    "since": "2026-10-30T00:00:00Z", "lookups": 4812, "blocked": 0,
                    "wouldBlock": 37, "mode": "log", "updated": "2026-10-30T00:00:10Z",
                },
            })
        );
        // The commands are the three reads, with fixed arguments.
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls[0],
            (
                IPTABLES.to_owned(),
                vec!["-L".into(), "INPUT".into(), "-n".into()]
            )
        );
        assert!(
            calls[1..]
                .iter()
                .all(|(p, a)| p == INITCTL && a[0] == "status")
        );
        drop(calls);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn before_guard_and_the_hook_it_says_so() {
        let dir = temp_dir("privacy-none");
        let paths = Paths {
            hook: dir.join("lumen-privacy"),
            hook_log: dir.join("lumen-privacy.log"),
            guard_status: dir.join("status"),
            owner: me(&dir),
        };
        let runner = FakeRunner::new(|program, _| {
            if program == IPTABLES {
                Ok(Output {
                    success: false,
                    stdout: String::new(),
                    stderr: "iptables: Another app is currently holding the xtables lock.\n".into(),
                })
            } else {
                Err("no initctl".into())
            }
        });
        let state = read(&paths, &runner);
        assert_eq!(state["hook"], json!({ "installed": false, "log": [] }));
        assert_eq!(state["guard"], Value::Null);
        assert_eq!(state["port11111"]["rejected"], json!(false));
        assert!(
            state["port11111"]["error"]
                .as_str()
                .unwrap()
                .contains("xtables lock")
        );
        assert!(
            state["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|j| j["running"] == json!(false))
        );
        // A status file without counts is no Guard either.
        fs::write(&paths.guard_status, "mode log\n").unwrap();
        assert_eq!(read(&paths, &runner)["guard"], Value::Null);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_link_a_pipe_or_someone_elses_file_is_not_read() {
        let dir = temp_dir("privacy-tmp");
        let secret = dir.join("secret");
        fs::write(&secret, "root:x:0:0\n").unwrap();
        let link = dir.join("lumen-privacy.log");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert_eq!(hook_log(&link, me(&dir)), Vec::<String>::new());
        assert!(read_owned(&link, me(&dir), 1 << 10).is_none());
        // The file itself is read, by its owner only.
        assert!(read_owned(&secret, me(&dir), 1 << 10).is_some());
        let other = me(&dir).map(|uid| uid.wrapping_add(1));
        assert!(read_owned(&secret, other, 1 << 10).is_none());
        // Too big for read_owned.
        assert!(read_owned(&secret, me(&dir), 4).is_none());
        // A FIFO: refused without waiting for a writer.
        let fifo = dir.join("fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        if made.is_ok_and(|s| s.success()) {
            assert!(read_owned(&fifo, None, 1 << 10).is_none());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_the_last_lines_of_a_long_log() {
        let dir = temp_dir("privacy-long");
        let log = dir.join("log");
        let text: String = (0..3000).map(|i| format!("line {i}\n")).collect();
        fs::write(&log, text).unwrap();
        let lines = hook_log(&log, me(&dir));
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines.last().unwrap(), "line 2999");
        assert_eq!(lines[0], "line 2950");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_iptables_and_initctl_as_the_tv_prints_them() {
        assert!(rejects_11111(LISTING));
        assert!(!rejects_11111(
            "ACCEPT     tcp  --  0.0.0.0/0  0.0.0.0/0  tcp dpt:11111\n"
        ));
        assert!(!rejects_11111(
            "REJECT     tcp  --  0.0.0.0/0  0.0.0.0/0  tcp dpt:111110\n"
        ));
        assert!(!rejects_11111(
            "REJECT     udp  --  0.0.0.0/0  0.0.0.0/0  udp dpt:11111\n"
        ));
        assert!(job_running("crashreportd start/running, process 3187"));
        assert!(job_running("uploadd start/pre-start"));
        assert!(!job_running("uploadd stop/waiting"));
    }

    #[test]
    fn writes_iso_dates() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_793_318_400), "2026-10-30T00:00:00Z");
        assert_eq!(iso8601(1_793_426_978), "2026-10-31T06:09:38Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(4_107_542_399), "2100-02-28T23:59:59Z");
    }
}
