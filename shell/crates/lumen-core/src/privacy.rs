//! What the Control Center's Privacy panel says (docs/specs/privacy.md, G5;
//! decision 12): Lumen Guard's mode and today's counts, and how privacy
//! stage S1 stands. It only reads: loosening anything comes from the
//! owner's phone or PC, never from the TV.
//!
//! The shell reads it all in one go, as root, through Homebrew Channel's
//! `exec` running the script the IPK ships for it, `lumen-privacy-read.sh`
//! (`lumen-shell`'s `luna.rs`): Guard's status file, then the `INPUT`
//! chain, then the four jobs S1 stops, each part after a marker line. This
//! module turns that text into the panel's lines, judging each part as
//! `lumen-ctl`'s `privacy.state` does.

use std::fmt::Write as _;

/// The line before the `INPUT` chain's listing.
pub const RULES_MARK: &str = "--- rules";
/// The line before the jobs' `initctl status` lines.
pub const JOBS_MARK: &str = "--- jobs";
/// The jobs S1 stops, in its order (privacy.md, S1).
pub const JOBS: [&str; 4] = ["crashreportd", "rdxd", "uploadd", "remotelogger"];
/// Guard rewrites its status every 10 s: one older than this means it has
/// stopped.
pub const GUARD_STALE_SECS: u64 = 60;
/// The longest mode word kept from the status file.
const MAX_MODE: usize = 16;

/// Guard's status file, the parts the panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard {
    /// `log` (G1: it only logs) or `block` (G2), as the file says.
    pub mode: String,
    pub lookups_today: u64,
    /// Names refused today (G2), and those G1 would have refused.
    pub blocked_today: u64,
    pub would_block_today: u64,
    /// Whether the file was rewritten within `GUARD_STALE_SECS` of the read.
    pub running: bool,
}

/// Everything the panel shows, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// `None` without a status file: Guard is not installed, or never ran.
    pub guard: Option<Guard>,
    /// Whether the rule rejecting tcp 11111 from the LAN is in; `None` if
    /// the chain could not be listed.
    pub port_closed: Option<bool>,
    /// How many of `JOBS` run; `None` if `initctl` printed nothing for
    /// any of them.
    pub jobs_running: Option<usize>,
}

/// Where the panel's reading stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Reading {
    /// Not asked for yet.
    #[default]
    Unread,
    /// Asked for; the answer is on its way.
    Reading,
    Read(State),
    /// The read failed, and why, short.
    Failed(String),
}

/// One line of the panel: a label on the left, its value on the right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub label: &'static str,
    pub value: String,
    /// Something the owner may want to look at: the value is drawn in the
    /// warning colour.
    pub warn: bool,
}

/// Reads the command's output, `now` seconds after the epoch (the TV's
/// clock, to judge whether Guard is still writing its file).
pub fn parse(out: &str, now: u64) -> State {
    let (status, rest) = split_once_line(out, RULES_MARK);
    let (rules, jobs) = split_once_line(rest.unwrap_or(""), JOBS_MARK);
    State {
        guard: guard(status, now),
        port_closed: rest.map(|_| rejects_11111(rules)),
        jobs_running: jobs.and_then(|text| {
            let named = JOBS.iter().filter(|job| named(text, job)).count();
            (named > 0).then(|| JOBS.iter().filter(|job| running(text, job)).count())
        }),
    }
}

/// `text` up to the line `mark`, and what follows it, if the mark is there.
fn split_once_line<'a>(text: &'a str, mark: &str) -> (&'a str, Option<&'a str>) {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end() == mark {
            return (&text[..at], Some(&text[at + line.len()..]));
        }
        at += line.len();
    }
    (text, None)
}

/// Guard's `key value` lines, if they hold at least its mode and counts.
fn guard(text: &str, now: u64) -> Option<Guard> {
    let field = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            (k == key).then(|| v.trim())
        })
    };
    let number = |key: &str| field(key).and_then(|v| v.parse::<u64>().ok());
    let mode: String = field("mode")?
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(MAX_MODE)
        .collect();
    let updated = number("updated")?;
    Some(Guard {
        mode,
        lookups_today: number("lookups_today")?,
        blocked_today: number("blocked_today").unwrap_or(0),
        would_block_today: number("would_block_today").unwrap_or(0),
        running: now.saturating_sub(updated) <= GUARD_STALE_SECS,
    })
}

/// As `lumen-ctl`'s: a `REJECT` or `DROP` line for tcp with `dpt:11111`.
fn rejects_11111(listing: &str) -> bool {
    listing.lines().any(|l| {
        let mut words = l.split_whitespace();
        matches!(words.next(), Some("REJECT" | "DROP"))
            && words.next() == Some("tcp")
            && l.split_whitespace().any(|w| w == "dpt:11111")
    })
}

/// `initctl status`'s line for `job`, if it printed one.
fn job_line<'a>(text: &'a str, job: &str) -> Option<&'a str> {
    text.lines()
        .find(|l| l.split_whitespace().next() == Some(job))
}

fn named(text: &str, job: &str) -> bool {
    job_line(text, job).is_some()
}

/// As `lumen-ctl`'s, and the hook's own reading: `job` runs if its line says
/// ` start/` (running, or on its way). A job upstart does not know, which
/// prints no line, does not run.
fn running(text: &str, job: &str) -> bool {
    job_line(text, job).is_some_and(|l| l.contains(" start/"))
}

/// `1234567` as `1,234,567`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The panel's lines for `reading`.
pub fn lines(reading: &Reading) -> Vec<Line> {
    let line = |label, value: &str, warn| Line {
        label,
        value: value.to_owned(),
        warn,
    };
    match reading {
        Reading::Unread | Reading::Reading => vec![
            line("Lumen Guard", "Reading…", false),
            line("Today", "", false),
            line("LG's reporting", "", false),
            line("Port 11111", "", false),
        ],
        Reading::Failed(why) => vec![line("Lumen Guard", &format!("Can't read: {why}"), true)],
        Reading::Read(state) => {
            let mut out = Vec::new();
            match &state.guard {
                None => {
                    out.push(line("Lumen Guard", "Not installed", true));
                    out.push(line("Today", "No lookups counted", false));
                }
                Some(g) => {
                    let (mode, warn) = match (g.running, g.mode.as_str()) {
                        (false, _) => ("Not running".to_owned(), true),
                        (true, "block") => ("Blocking".to_owned(), false),
                        (true, "log") => ("Logging only".to_owned(), false),
                        (true, other) => (other.to_owned(), false),
                    };
                    out.push(line("Lumen Guard", &mode, warn));
                    let mut today = String::new();
                    let (n, verb) = if g.mode == "block" {
                        (g.blocked_today, "blocked")
                    } else {
                        (g.would_block_today, "would be blocked")
                    };
                    let _ = write!(
                        today,
                        "{} of {} lookups {verb}",
                        thousands(n),
                        thousands(g.lookups_today)
                    );
                    out.push(line("Today", &today, false));
                }
            }
            let (jobs, warn) = match state.jobs_running {
                Some(0) => ("All stopped".to_owned(), false),
                Some(n) => (format!("{n} of {} jobs running", JOBS.len()), true),
                None => ("Unknown".to_owned(), true),
            };
            out.push(line("LG's reporting", &jobs, warn));
            let (port, warn) = match state.port_closed {
                Some(true) => ("Closed", false),
                Some(false) => ("Open", true),
                None => ("Unknown", true),
            };
            out.push(line("Port 11111", port, warn));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the command prints on a B9 with G2 blocking and S1 in place,
    /// in the shapes `lumen-guard`'s status file, `iptables -L INPUT -n`
    /// and `initctl status` have.
    const B9: &str = "version 0.2.0\nmode block\nblock_refused none\nlisten 127.0.0.2:53\n\
        upstream 192.168.86.1:53\ndscp 7\npid 1234\nstarted 1794100000\nupdated 1794172190\n\
        lookups 51234\nwould_block 0\nblocked 4021\nlearned 0\nday 2026-11-08\n\
        lookups_today 6789\nwould_block_today 0\nblocked_today 512\nlearned_today 0\n\
        learning_until 0\nlearned_names 0\nowner_rules 3\nupstream_errors 0\n\
        upstream_timeouts 2\nlog_dropped 0\n\
        --- rules\n\
        Chain INPUT (policy ACCEPT)\n\
        target     prot opt source               destination\n\
        REJECT     tcp  --  0.0.0.0/0            0.0.0.0/0            tcp dpt:11111 reject-with icmp-port-unreachable\n\
        --- jobs\n\
        crashreportd stop/waiting\nrdxd stop/waiting\nuploadd stop/waiting\n\
        remotelogger start/running, process 912\n";

    #[test]
    fn the_b9s_answer_reads_as_the_panel_says_it() {
        let state = parse(B9, 1_794_172_200);
        let guard = state.guard.clone().expect("Guard");
        assert_eq!(guard.mode, "block");
        assert_eq!((guard.lookups_today, guard.blocked_today), (6789, 512));
        assert!(guard.running);
        assert_eq!(state.port_closed, Some(true));
        assert_eq!(state.jobs_running, Some(1));
        let shown: Vec<(&str, String, bool)> = lines(&Reading::Read(state))
            .into_iter()
            .map(|l| (l.label, l.value, l.warn))
            .collect();
        assert_eq!(
            shown,
            [
                ("Lumen Guard", "Blocking".to_owned(), false),
                ("Today", "512 of 6,789 lookups blocked".to_owned(), false),
                ("LG's reporting", "1 of 4 jobs running".to_owned(), true),
                ("Port 11111", "Closed".to_owned(), false),
            ]
        );
    }

    #[test]
    fn g1_logs_and_a_quiet_file_means_guard_stopped() {
        let g1 = "mode log\nupdated 100\nlookups_today 10\nwould_block_today 4\n";
        let state = parse(g1, 120);
        let lines = lines(&Reading::Read(state));
        assert_eq!(lines[0].value, "Logging only");
        assert_eq!(lines[1].value, "4 of 10 lookups would be blocked");
        let old = parse(g1, 100 + GUARD_STALE_SECS + 1);
        let lines = super::lines(&Reading::Read(old));
        assert_eq!(
            (lines[0].value.as_str(), lines[0].warn),
            ("Not running", true)
        );
    }

    #[test]
    fn nothing_there_reads_as_not_installed_and_unknown() {
        let state = parse("", 0);
        assert_eq!(
            state,
            State {
                guard: None,
                port_closed: None,
                jobs_running: None
            }
        );
        let lines = lines(&Reading::Read(state));
        assert_eq!(lines[0].value, "Not installed");
        assert_eq!(lines[2].value, "Unknown");
        assert_eq!(lines[3].value, "Unknown");
        // Jobs all stopped, as the hook leaves them; one unknown to upstart
        // prints nothing and does not run.
        let jobs = "--- rules\n--- jobs\ncrashreportd stop/waiting\nuploadd stop/waiting\n";
        assert_eq!(parse(jobs, 0).jobs_running, Some(0));
        assert_eq!(lines_of(jobs)[2].value, "All stopped");
        // A file without its counts is no Guard either.
        assert_eq!(parse("mode block\nupdated 5\n", 5).guard, None);
    }

    #[test]
    fn the_rule_and_the_jobs_are_judged_as_lumen_ctl_judges_them() {
        assert!(!rejects_11111(
            "REJECT tcp -- 0.0.0.0/0 0.0.0.0/0 tcp dpt:111110"
        ));
        assert!(!rejects_11111(
            "REJECT udp -- 0.0.0.0/0 0.0.0.0/0 udp dpt:11111"
        ));
        assert!(rejects_11111(
            "DROP tcp -- 0.0.0.0/0 0.0.0.0/0 tcp dpt:11111"
        ));
        assert!(!running("uploadd stop/waiting", "uploadd"));
        assert!(running("uploadd start/running, process 3", "uploadd"));
        assert!(running("uploadd start/pre-start", "uploadd"));
        assert!(!running("initctl: Unknown job: uploadd", "uploadd"));
        // The mode is cleaned before it is shown.
        let odd = parse("mode b\u{1b}[2Jlock\nupdated 1\nlookups_today 0\n", 1);
        assert_eq!(odd.guard.map(|g| g.mode).as_deref(), Some("b2Jlock"));
    }

    fn lines_of(out: &str) -> Vec<Line> {
        lines(&Reading::Read(parse(out, 0)))
    }

    #[test]
    fn counts_read_with_their_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn reading_and_failing_say_so() {
        assert_eq!(lines(&Reading::Reading)[0].value, "Reading…");
        let failed = lines(&Reading::Failed("no Homebrew Channel".to_owned()));
        assert_eq!(failed.len(), 1);
        assert!(failed[0].warn && failed[0].value.contains("no Homebrew Channel"));
    }
}
