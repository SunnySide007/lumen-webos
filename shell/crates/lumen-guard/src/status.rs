//! Guard's counts and the status file: plain `key value` lines, rewritten
//! whole (a temporary file renamed over the old one) every ten seconds and at
//! exit, for `tools/guard.sh status` and the shell's Privacy panel (G5).
//! Every value is one word, so a reader splits each line once.

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::net::SocketAddrV4;
use std::path::Path;

use crate::control::Mode;
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
    /// Lookups the rules don't allow: blocked in block mode, forwarded and
    /// only logged in log mode.
    pub would_block: u64,
    /// Lookups Guard answered itself instead of forwarding.
    pub blocked: u64,
    /// Lookups allowed only because a learning window was open.
    pub learned: u64,
    /// Lookups that went to the router in clear while the TLS switch was on
    /// (G3's fallback).
    pub fallback: u64,
}

impl Counts {
    fn add(&mut self, verdict: Verdict, replied: bool) {
        self.lookups += 1;
        match verdict {
            Verdict::Block => self.would_block += 1,
            Verdict::Learn => self.learned += 1,
            Verdict::Allow => {}
        }
        if replied {
            self.blocked += 1;
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
    /// Answers that came over TLS.
    pub tls_answers: u64,
    /// TLS connections made, and attempts that failed.
    pub tls_handshakes: u64,
    pub tls_errors: u64,
}

impl Stats {
    /// Counts one lookup on `today` (the local date; `None` if unknown);
    /// `replied` when Guard answered it itself.
    pub fn count(&mut self, verdict: Verdict, replied: bool, today: Option<Date>) {
        self.roll(today);
        self.total.add(verdict, replied);
        self.today.add(verdict, replied);
    }

    /// Counts a lookup sent in clear while the TLS switch was on.
    pub fn count_fallback(&mut self, today: Option<Date>) {
        self.roll(today);
        self.total.fallback += 1;
        self.today.fallback += 1;
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
pub struct Info<'a> {
    pub mode: Mode,
    /// The name that kept Guard from blocking, when blocking was asked for.
    pub refused: Option<&'a str>,
    pub listen: SocketAddrV4,
    pub upstream: SocketAddrV4,
    pub dscp: u8,
    pub pid: u32,
    pub started: u64,
    /// The open learning window's end, or 0.
    pub learning_until: u64,
    /// New names the latest window learned.
    pub learned: usize,
    pub owner_rules: usize,
    /// Whether the owner's switch asks for TLS (G3).
    pub tls: bool,
    /// Where a lookup goes now: upstream.rs's `Route::word`.
    pub route: &'static str,
    /// The TLS server a new connection goes to, and its name.
    pub tls_server: SocketAddrV4,
    pub tls_name: &'a str,
    /// TLS connections open now.
    pub tls_open: usize,
    /// When the current fallback began, or 0.
    pub fallback_since: u64,
}

pub fn render(info: &Info<'_>, stats: &Stats, log_dropped: u64, now: u64) -> String {
    let mut out = String::new();
    let day = stats
        .day
        .map_or_else(|| "unknown".to_owned(), |d| d.to_string());
    let _ = write!(
        out,
        "version {}\nmode {}\nblock_refused {}\nlisten {}\nupstream {}\ndscp {}\npid {}\n\
         started {}\nupdated {}\n\
         lookups {}\nwould_block {}\nblocked {}\nlearned {}\nday {}\nlookups_today {}\n\
         would_block_today {}\nblocked_today {}\nlearned_today {}\n\
         learning_until {}\nlearned_names {}\nowner_rules {}\n\
         upstream_errors {}\nupstream_timeouts {}\nlog_dropped {}\n",
        env!("CARGO_PKG_VERSION"),
        info.mode.word(),
        info.refused.unwrap_or("none"),
        info.listen,
        info.upstream,
        info.dscp,
        info.pid,
        info.started,
        now,
        stats.total.lookups,
        stats.total.would_block,
        stats.total.blocked,
        stats.total.learned,
        day,
        stats.today.lookups,
        stats.today.would_block,
        stats.today.blocked,
        stats.today.learned,
        info.learning_until,
        info.learned,
        info.owner_rules,
        stats.upstream_errors,
        stats.upstream_timeouts,
        log_dropped,
    );
    let _ = write!(
        out,
        "tls {}\nroute {}\ntls_server {}\ntls_name {}\ntls_open {}\ntls_answers {}\n\
         tls_handshakes {}\ntls_errors {}\nfallback {}\nfallback_today {}\nfallback_since {}\n",
        if info.tls { "on" } else { "off" },
        info.route,
        info.tls_server,
        info.tls_name,
        info.tls_open,
        stats.tls_answers,
        stats.tls_handshakes,
        stats.tls_errors,
        stats.total.fallback,
        stats.today.fallback,
        info.fallback_since,
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

    fn info() -> Info<'static> {
        Info {
            mode: Mode::Block,
            refused: None,
            listen: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 2), 53),
            upstream: SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 53),
            dscp: 7,
            pid: 42,
            started: 1_793_318_400,
            learning_until: 0,
            learned: 0,
            owner_rules: 0,
            tls: false,
            route: "clear",
            tls_server: SocketAddrV4::new(Ipv4Addr::new(9, 9, 9, 9), 853),
            tls_name: "dns.quad9.net",
            tls_open: 0,
            fallback_since: 0,
        }
    }

    #[test]
    fn renders_the_tls_upstream_and_its_fallback() {
        let mut stats = Stats::default();
        stats.count_fallback(Some(OCT_30));
        stats.count_fallback(Some(OCT_30));
        stats.count_fallback(Some(OCT_31));
        stats.tls_answers = 7;
        stats.tls_handshakes = 2;
        stats.tls_errors = 1;
        let mut info = info();
        info.tls = true;
        info.route = "fallback-down";
        info.tls_open = 1;
        info.fallback_since = 1_793_318_400;
        let text = render(&info, &stats, 0, 1_793_318_410);
        for line in [
            "tls on",
            "route fallback-down",
            "tls_server 9.9.9.9:853",
            "tls_name dns.quad9.net",
            "tls_open 1",
            "tls_answers 7",
            "tls_handshakes 2",
            "tls_errors 1",
            "fallback 3",
            "fallback_today 1",
            "fallback_since 1793318400",
        ] {
            assert!(text.lines().any(|l| l == line), "no {line:?} in\n{text}");
        }
        assert!(text.lines().all(|l| l.split(' ').count() == 2), "{text}");
    }

    #[test]
    fn today_starts_again_at_midnight() {
        let mut stats = Stats::default();
        stats.count(Verdict::Allow, false, Some(OCT_30));
        stats.count(Verdict::Block, true, Some(OCT_30));
        stats.count(Verdict::Learn, false, Some(OCT_30));
        assert_eq!(
            stats.today,
            Counts {
                lookups: 3,
                would_block: 1,
                blocked: 1,
                learned: 1,
                fallback: 0,
            }
        );
        stats.count(Verdict::Allow, false, Some(OCT_31));
        assert_eq!(
            stats.today,
            Counts {
                lookups: 1,
                ..Counts::default()
            }
        );
        assert_eq!(stats.total.lookups, 4);
        assert_eq!(stats.total.would_block, 1);
        // A status write after midnight with no lookup yet shows zero.
        stats.roll(Some(Date {
            year: 2026,
            month: 11,
            day: 1,
        }));
        assert_eq!(stats.today, Counts::default());
        assert_eq!(stats.total.lookups, 4);
    }

    #[test]
    fn renders_key_value_lines() {
        let mut stats = Stats::default();
        stats.count(Verdict::Block, true, Some(OCT_30));
        // Log mode: the rules say no, and the lookup is forwarded all the same.
        stats.count(Verdict::Block, false, Some(OCT_30));
        stats.upstream_timeouts = 2;
        let mut info = info();
        info.learning_until = 1_793_319_300;
        info.learned = 3;
        info.owner_rules = 5;
        let text = render(&info, &stats, 3, 1_793_318_410);
        for line in [
            "mode block",
            "block_refused none",
            "listen 127.0.0.2:53",
            "upstream 192.0.2.1:53",
            "dscp 7",
            "pid 42",
            "started 1793318400",
            "updated 1793318410",
            "lookups 2",
            "would_block 2",
            "blocked 1",
            "learned 0",
            "day 2026-10-30",
            "lookups_today 2",
            "would_block_today 2",
            "blocked_today 1",
            "learning_until 1793319300",
            "learned_names 3",
            "owner_rules 5",
            "upstream_timeouts 2",
            "log_dropped 3",
        ] {
            assert!(text.lines().any(|l| l == line), "no {line:?} in\n{text}");
        }
        assert!(text.lines().all(|l| l.split(' ').count() == 2), "{text}");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn a_refusal_and_an_unknown_date_are_said_so() {
        let mut stats = Stats::default();
        stats.count(Verdict::Allow, false, None);
        assert_eq!(stats.today.lookups, 1);
        let mut info = info();
        info.mode = Mode::Log;
        info.refused = Some("us.lgtvsdp.com");
        let text = render(&info, &stats, 0, 0);
        assert!(text.contains("\nday unknown\n"), "{text}");
        assert!(
            text.contains("\nmode log\nblock_refused us.lgtvsdp.com\n"),
            "{text}"
        );
        assert!(text.lines().all(|l| l.split(' ').count() == 2), "{text}");
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
