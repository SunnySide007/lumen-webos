//! Guard's counts and the status file: plain `key value` lines, rewritten
//! whole (a temporary file renamed over the old one) every ten seconds and at
//! exit, for `tools/guard.sh status` and the shell's Privacy panel (G5).

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::net::SocketAddrV4;
use std::path::Path;

use crate::rules::Verdict;

/// A calendar date in the TV's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub lookups: u64,
    pub would_block: u64,
}

impl Counts {
    fn add(&mut self, verdict: Verdict) {
        self.lookups += 1;
        if verdict == Verdict::WouldBlock {
            self.would_block += 1;
        }
    }
}

#[derive(Debug, Default)]
pub struct Stats {
    pub total: Counts,
    /// Since midnight of `day`.
    pub today: Counts,
    pub day: Option<Date>,
    /// Queries Guard could not send upstream, or had no room for.
    pub upstream_errors: u64,
    /// Queries the upstream did not answer within 3 s.
    pub upstream_timeouts: u64,
}

impl Stats {
    /// Counts one lookup on `today` (the local date; `None` if unknown).
    pub fn count(&mut self, verdict: Verdict, today: Option<Date>) {
        self.roll(today);
        self.total.add(verdict);
        self.today.add(verdict);
    }

    /// Starts a new day's counts when the date has changed.
    pub fn roll(&mut self, today: Option<Date>) {
        if today != self.day {
            self.today = Counts::default();
            self.day = today;
        }
    }
}

/// What the status file says besides the counts.
pub struct Info {
    pub listen: SocketAddrV4,
    pub upstream: SocketAddrV4,
    pub dscp: u8,
    pub pid: u32,
    pub started: u64,
}

pub fn render(info: &Info, stats: &Stats, log_dropped: u64, now: u64) -> String {
    let mut out = String::new();
    let day = stats
        .day
        .map_or_else(|| "unknown".to_owned(), |d| d.to_string());
    let _ = write!(
        out,
        "version {}\nmode log\nlisten {}\nupstream {}\ndscp {}\npid {}\nstarted {}\nupdated {}\n\
         lookups {}\nwould_block {}\nday {}\nlookups_today {}\nwould_block_today {}\n\
         upstream_errors {}\nupstream_timeouts {}\nlog_dropped {}\n",
        env!("CARGO_PKG_VERSION"),
        info.listen,
        info.upstream,
        info.dscp,
        info.pid,
        info.started,
        now,
        stats.total.lookups,
        stats.total.would_block,
        day,
        stats.today.lookups,
        stats.today.would_block,
        stats.upstream_errors,
        stats.upstream_timeouts,
        log_dropped,
    );
    out
}

/// Replaces `path` with `text` in one step: readers see the old file or the
/// new one, never half of it.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const OCT_30: Date = Date {
        year: 2026,
        month: 10,
        day: 30,
    };
    const OCT_31: Date = Date {
        year: 2026,
        month: 10,
        day: 31,
    };

    #[test]
    fn today_starts_again_at_midnight() {
        let mut stats = Stats::default();
        stats.count(Verdict::Allow, Some(OCT_30));
        stats.count(Verdict::WouldBlock, Some(OCT_30));
        assert_eq!(
            stats.today,
            Counts {
                lookups: 2,
                would_block: 1
            }
        );
        stats.count(Verdict::Allow, Some(OCT_31));
        assert_eq!(
            stats.today,
            Counts {
                lookups: 1,
                would_block: 0
            }
        );
        assert_eq!(
            stats.total,
            Counts {
                lookups: 3,
                would_block: 1
            }
        );
        // A status write after midnight with no lookup yet shows zero.
        stats.roll(Some(Date {
            year: 2026,
            month: 11,
            day: 1,
        }));
        assert_eq!(stats.today, Counts::default());
        assert_eq!(stats.total.lookups, 3);
    }

    #[test]
    fn renders_key_value_lines() {
        let mut stats = Stats::default();
        stats.count(Verdict::WouldBlock, Some(OCT_30));
        stats.upstream_timeouts = 2;
        let info = Info {
            listen: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 2), 53),
            upstream: SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 53),
            dscp: 7,
            pid: 42,
            started: 1_793_318_400,
        };
        let text = render(&info, &stats, 3, 1_793_318_410);
        for line in [
            "mode log",
            "listen 127.0.0.2:53",
            "upstream 192.0.2.1:53",
            "dscp 7",
            "pid 42",
            "started 1793318400",
            "updated 1793318410",
            "lookups 1",
            "would_block 1",
            "day 2026-10-30",
            "lookups_today 1",
            "would_block_today 1",
            "upstream_timeouts 2",
            "log_dropped 3",
        ] {
            assert!(text.lines().any(|l| l == line), "no {line:?} in\n{text}");
        }
        assert!(text.lines().all(|l| l.split(' ').count() == 2), "{text}");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn an_unknown_date_is_said_so() {
        let mut stats = Stats::default();
        stats.count(Verdict::Allow, None);
        assert_eq!(stats.today.lookups, 1);
        let info = Info {
            listen: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5353),
            upstream: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5354),
            dscp: 0,
            pid: 1,
            started: 0,
        };
        assert!(render(&info, &stats, 0, 0).contains("\nday unknown\n"));
    }

    #[test]
    fn writes_the_file_whole() {
        let dir = std::env::temp_dir().join(format!("lumen-guard-status-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status");
        write_atomic(&path, "lookups 1\n").unwrap();
        write_atomic(&path, "lookups 2\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "lookups 2\n");
        assert!(!dir.join("status.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
