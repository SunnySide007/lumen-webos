//! What the UDP and TCP loops share: the configuration, the rules and the
//! owner's controls, the lookup log and the counts, and Guard's own messages
//! on stderr (which the boot hook sends to /var/lib/lumen/log/lumen-guard.log).

use std::fmt;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::args::Config;
use crate::control::{Control, Mode};
use crate::lookups::Lookups;
use crate::rules::{Rules, Verdict};
use crate::status::{self, Info, Stats};
use crate::sys;
use crate::wire::{self, Type};

/// How long an asker may keep Guard's sinkhole answer: a name the owner
/// allows, or a learning window opens for, resolves within a minute.
pub const BLOCK_TTL: u32 = 60;

/// What to do with a query.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Forward,
    /// Answer the asker with these bytes; nothing goes upstream.
    Reply(Vec<u8>),
}

pub struct Guard {
    pub config: Config,
    rules: Rules,
    control: Mutex<Control>,
    lookups: Mutex<Lookups>,
    stats: Mutex<Stats>,
    started: u64,
}

impl Guard {
    pub fn new(config: Config, rules: Rules) -> Self {
        let lookups = Lookups::open(&config.log, config.max_log_bytes);
        let control = Control::new(config.mode);
        Self {
            config,
            rules,
            control: Mutex::new(control),
            lookups: Mutex::new(lookups),
            stats: Mutex::new(Stats::default()),
            started: unix_now(),
        }
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    pub fn mode(&self) -> Mode {
        lock(&self.control).mode()
    }

    /// Reads the owner's control files again where they changed, and says
    /// what that changed. The UDP loop calls it every second.
    pub fn poll_control(&self) {
        let said = lock(&self.control).poll(
            &self.rules,
            self.config.mode_file.as_deref(),
            self.config.owner.as_deref(),
            self.config.learn.as_deref(),
            unix_now(),
        );
        for line in said {
            crate::say!("{line}");
        }
    }

    /// Decides, logs and counts one query. In log mode everything goes
    /// upstream (G1); in block mode only a standard query with one question
    /// for a name the rules allow, or the open window learns, does.
    pub fn handle(&self, msg: &[u8]) -> Action {
        let now = unix_now();
        let (mode, verdict, question) = {
            let mut control = lock(&self.control);
            let mode = control.mode();
            let question = wire::first_question(msg).ok().flatten();
            let verdict = match &question {
                Some(q) if wire::opcode(msg) == 0 && wire::question_count(msg) == 1 => {
                    let learning = control.learning(now);
                    match self.rules.verdict(&q.name, &control.owner, learning) {
                        Verdict::Learn if !control.learn(&q.name, now) => Verdict::Block,
                        verdict => verdict,
                    }
                }
                _ => Verdict::Block,
            };
            (mode, verdict, question)
        };
        let word = match (verdict, mode) {
            (Verdict::Allow, _) => "allow",
            (Verdict::Learn, _) => "learn",
            (Verdict::Block, Mode::Block) => "block",
            (Verdict::Block, Mode::Log) => "would-block",
        };
        let line = match &question {
            Some(q) => format!("{now} {} {} {word}\n", Type(q.qtype), q.name),
            None => format!("{now} - - {word}\n"),
        };
        lock(&self.lookups).append(&line);
        let action = match (verdict, mode) {
            (Verdict::Block, Mode::Block) => Action::Reply(match &question {
                _ if wire::opcode(msg) != 0 => wire::error_answer(msg, wire::REFUSED),
                Some(q) if wire::question_count(msg) == 1 && !q.compressed => {
                    wire::sinkhole_answer(msg, q, BLOCK_TTL)
                }
                _ => wire::error_answer(msg, wire::FORMERR),
            }),
            _ => Action::Forward,
        };
        let replied = matches!(action, Action::Reply(_));
        lock(&self.stats).count(verdict, replied, sys::local_date(now));
        action
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
        let (mode, refused, learning_until, learned, owner_rules) = {
            let control = lock(&self.control);
            (
                control.mode(),
                control.refused.clone(),
                control.learning_until(now),
                control.learned.len(),
                control.owner.len(),
            )
        };
        let text = {
            let mut stats = lock(&self.stats);
            stats.roll(sys::local_date(now));
            let info = Info {
                mode,
                refused: refused.as_deref(),
                listen: self.config.listen,
                upstream: self.config.upstream,
                dscp: self.config.dscp,
                pid: std::process::id(),
                started: self.started,
                learning_until,
                learned,
                owner_rules,
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
