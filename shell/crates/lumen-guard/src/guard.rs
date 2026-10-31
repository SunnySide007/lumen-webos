//! What the UDP and TCP loops share: the configuration, the rules, the lookup
//! log and the counts, and Guard's own messages on stderr (which the boot hook
//! sends to /var/lib/lumen/log/lumen-guard.log).

use std::fmt;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::args::Config;
use crate::lookups::Lookups;
use crate::rules::{Rules, Verdict};
use crate::status::{self, Info, Stats};
use crate::sys;
use crate::wire::{self, Type};

pub struct Guard {
    pub config: Config,
    rules: Rules,
    lookups: Mutex<Lookups>,
    stats: Mutex<Stats>,
    started: u64,
}

impl Guard {
    pub fn new(config: Config, rules: Rules) -> Self {
        let lookups = Lookups::open(&config.log, config.max_log_bytes);
        Self {
            config,
            rules,
            lookups: Mutex::new(lookups),
            stats: Mutex::new(Stats::default()),
            started: unix_now(),
        }
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Logs and counts one query. A query whose question can't be read is
    /// still forwarded, so it is logged too, with `-` for its type and name.
    pub fn record(&self, msg: &[u8]) {
        let now = unix_now();
        let question = wire::first_question(msg).ok().flatten();
        let (line, verdict) = match &question {
            Some(q) => {
                let verdict = self.rules.verdict(&q.name);
                (
                    format!("{now} {} {} {}\n", Type(q.qtype), q.name, verdict.word()),
                    verdict,
                )
            }
            None => (
                format!("{now} - - {}\n", Verdict::Allow.word()),
                Verdict::Allow,
            ),
        };
        lock(&self.lookups).append(&line);
        lock(&self.stats).count(verdict, sys::local_date(now));
    }

    pub fn upstream_error(&self) {
        lock(&self.stats).upstream_errors += 1;
    }

    pub fn upstream_timeouts(&self, n: u64) {
        if n > 0 {
            lock(&self.stats).upstream_timeouts += n;
        }
    }

    /// Rewrites the status file; a failure is reported at most once a minute
    /// by the caller's throttle.
    pub fn write_status(&self) -> std::io::Result<()> {
        let now = unix_now();
        let dropped = lock(&self.lookups).dropped;
        let text = {
            let mut stats = lock(&self.stats);
            stats.roll(sys::local_date(now));
            let info = Info {
                listen: self.config.listen,
                upstream: self.config.upstream,
                dscp: self.config.dscp,
                pid: std::process::id(),
                started: self.started,
            };
            status::render(&info, &stats, dropped, now)
        };
        status::write_atomic(&self.config.status, &text)
    }
}

/// A poisoned lock only means another thread panicked mid-update; the counts
/// are still worth having.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One timestamped line on stderr.
pub fn say(args: fmt::Arguments<'_>) {
    eprintln!("[{}] lumen-guard: {args}", unix_now());
}

#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => { $crate::guard::say(format_args!($($arg)*)) };
}

/// Keeps a recurring error to one stderr line a minute.
pub struct Throttle {
    what: &'static str,
    next: Option<Instant>,
    quiet: u64,
}

impl Throttle {
    const EVERY: Duration = Duration::from_secs(60);

    pub const fn new(what: &'static str) -> Self {
        Self {
            what,
            next: None,
            quiet: 0,
        }
    }

    pub fn warn(&mut self, err: &dyn fmt::Display) {
        let now = Instant::now();
        if self.next.is_some_and(|next| now < next) {
            self.quiet += 1;
            return;
        }
        if self.quiet > 0 {
            say(format_args!(
                "{}: {err} ({} more since the last line)",
                self.what, self.quiet
            ));
        } else {
            say(format_args!("{}: {err}", self.what));
        }
        self.quiet = 0;
        self.next = Some(now + Self::EVERY);
    }
}
