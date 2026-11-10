//! The shell kept resident: decision 15's lumend layer. lumend starts the
//! shell hidden after boot's first app is up, and again if it is closed,
//! unless it said it was leaving on purpose.
//!
//! "Hidden" is the launch parameter lane L's shell reads (bus.rs): it starts
//! and draws nothing until a Home press. A shell without that change would
//! come to the front over the owner's film, so this part waits for `lumend
//! try shell` on the TV with the shell that has it. Restarts are at most
//! three in ten minutes; after that a shell that keeps dying is left alone
//! until the next boot. The shell's own deliberate exits (lane L's
//! `exitAfter`, Back on the top row) say `leaving` first.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::bus::{self, Bus, LifeEvent};
use crate::paths::{APP_ID, LG_HOME};

/// The latest the boot start waits for the first app.
pub const BOOT_WAIT: Duration = Duration::from_secs(120);
const RESTART_DELAY: Duration = Duration::from_secs(5);
/// A `leaving` this recent means a close was the shell's own.
const LEAVING_GRACE: Duration = Duration::from_secs(15);
const WINDOW: Duration = Duration::from_secs(600);
const MAX_RESTARTS: usize = 3;

/// The longest a tool's hold is believed. The B9 has no clock chip and
/// boots in 2019, so a time further ahead than this is a stale file or a
/// clock that has not been set yet, not a hold.
pub const MAX_HOLD: Duration = Duration::from_secs(900);

/// How long the hold file still holds: its first word is the unix time the
/// hold ends. Nothing when it is past, absurdly far ahead, or not a number.
pub fn hold_left(text: &str, now_unix: u64) -> Option<Duration> {
    let until: u64 = text.split_whitespace().next()?.parse().ok()?;
    let left = Duration::from_secs(until.checked_sub(now_unix)?);
    (!left.is_zero() && left <= MAX_HOLD).then_some(left)
}

/// What an event asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Nothing,
    /// Boot's first app is up: start the shell now.
    BootLaunch,
    /// The shell was closed: start it again then.
    RestartAt(Instant),
    /// SAM started the shell again while it ran.
    Relaunched {
        home_key: bool,
    },
}

#[derive(Debug, Default)]
pub struct Keeper {
    /// The boot start was made (or the shell was seen running).
    pub started: bool,
    running: bool,
    /// lumend launched it and SAM's `launch` for that has yet to come.
    awaited: bool,
    leaving: Option<Instant>,
    restarts: VecDeque<Instant>,
    pub gave_up: bool,
}

impl Keeper {
    pub fn on_event(&mut self, e: &LifeEvent, now: Instant) -> Ask {
        if e.app != APP_ID {
            let first_app = e.event == "foreground" && e.app != LG_HOME;
            if !self.started && first_app {
                return Ask::BootLaunch;
            }
            return Ask::Nothing;
        }
        match e.event.as_str() {
            "launch" | "relaunch" if self.running && !self.awaited => Ask::Relaunched {
                home_key: e.home_key,
            },
            "launch" | "relaunch" | "splash" | "preload" | "foreground" | "background" => {
                if matches!(e.event.as_str(), "launch" | "relaunch") {
                    self.awaited = false;
                }
                self.running = true;
                self.started = true;
                Ask::Nothing
            }
            "stop" | "close" => {
                self.running = false;
                self.awaited = false;
                if self
                    .leaving
                    .is_some_and(|t| now.duration_since(t) < LEAVING_GRACE)
                {
                    self.leaving = None;
                    return Ask::Nothing;
                }
                if self.gave_up {
                    return Ask::Nothing;
                }
                Ask::RestartAt(now + RESTART_DELAY)
            }
            _ => Ask::Nothing,
        }
    }

    /// Whether SAM's events say the shell runs now.
    pub fn running(&self) -> bool {
        self.running
    }

    /// The shell said it is about to exit on purpose.
    pub fn leaving(&mut self, now: Instant) {
        self.leaving = Some(now);
    }

    /// Starts the shell hidden; `restart` counts against the limit.
    pub fn launch(&mut self, bus: &dyn Bus, now: Instant, restart: bool) -> String {
        if restart {
            while self
                .restarts
                .front()
                .is_some_and(|t| now.duration_since(*t) > WINDOW)
            {
                self.restarts.pop_front();
            }
            if self.restarts.len() >= MAX_RESTARTS {
                self.gave_up = true;
                return format!(
                    "shell: closed {MAX_RESTARTS} times in ten minutes; not started again until the next boot"
                );
            }
            self.restarts.push_back(now);
        }
        self.started = true;
        let (uri, payload) = bus::launch_shell_hidden();
        let what = if restart {
            "started again"
        } else {
            "started after boot"
        };
        match bus::checked(bus, uri, &payload) {
            Ok(_) => {
                // Before SAM's own event says so: a second launch in the
                // meantime would be a relaunch, which shows a hidden shell.
                self.running = true;
                self.awaited = true;
                format!("shell: {what}, hidden")
            }
            Err(e) => format!("shell: FAILED to start: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::fake::FakeBus;
    use serde_json::json;

    fn ev(app: &str, event: &str) -> LifeEvent {
        LifeEvent {
            app: app.to_owned(),
            event: event.to_owned(),
            home_key: false,
        }
    }

    #[test]
    fn starts_after_boots_first_app_once() {
        let mut k = Keeper::default();
        let now = Instant::now();
        assert_eq!(k.on_event(&ev(LG_HOME, "foreground"), now), Ask::Nothing);
        assert_eq!(
            k.on_event(&ev("com.webos.app.hdmi3", "foreground"), now),
            Ask::BootLaunch
        );
        let bus = FakeBus::default();
        bus.reply(bus::LAUNCH, Ok(json!({"returnValue": true})));
        assert_eq!(
            k.launch(&bus, now, false),
            "shell: started after boot, hidden"
        );
        assert_eq!(
            bus.calls_to(bus::LAUNCH),
            [json!({"id": APP_ID, "params": {"hidden": true, "platform": "wayland"}})]
        );
        assert_eq!(k.on_event(&ev("netflix", "foreground"), now), Ask::Nothing);
    }

    #[test]
    fn restarts_a_closed_shell_but_not_one_that_left_or_keeps_dying() {
        let mut k = Keeper::default();
        let bus = FakeBus::default();
        bus.reply(bus::LAUNCH, Ok(json!({"returnValue": true})));
        let t0 = Instant::now();
        k.on_event(&ev(APP_ID, "launch"), t0);
        assert_eq!(
            k.on_event(&ev(APP_ID, "close"), t0),
            Ask::RestartAt(t0 + RESTART_DELAY)
        );
        k.on_event(&ev(APP_ID, "launch"), t0);
        k.leaving(t0);
        assert_eq!(
            k.on_event(&ev(APP_ID, "stop"), t0 + Duration::from_secs(1)),
            Ask::Nothing
        );
        for n in 0..3 {
            assert!(
                k.launch(&bus, t0 + Duration::from_secs(n), true)
                    .contains("started again")
            );
        }
        assert!(
            k.launch(&bus, t0 + Duration::from_secs(4), true)
                .contains("until the next boot")
        );
        assert!(k.gave_up);
        k.on_event(&ev(APP_ID, "launch"), t0);
        assert_eq!(k.on_event(&ev(APP_ID, "close"), t0), Ask::Nothing);
    }

    #[test]
    fn a_tools_hold_lasts_to_its_time_and_no_further_than_fifteen_minutes() {
        assert_eq!(
            hold_left("1794280120\n", 1_794_280_000),
            Some(Duration::from_secs(120))
        );
        // Past, or exactly now.
        assert_eq!(hold_left("1794280000", 1_794_280_000), None);
        assert_eq!(hold_left("1794279999", 1_794_280_000), None);
        // The TV's clock still reads 2019 after a boot: not a hold of years.
        assert_eq!(hold_left("1794280120", 1_546_300_800), None);
        assert_eq!(
            hold_left("1794280900", 1_794_280_000),
            Some(Duration::from_secs(900))
        );
        assert_eq!(hold_left("1794280901", 1_794_280_000), None);
        // Not a time.
        assert_eq!(hold_left("", 1), None);
        assert_eq!(hold_left("soon", 1), None);
        assert_eq!(hold_left("-5", 1), None);
    }

    #[test]
    fn a_shell_it_just_started_counts_as_running_before_sam_says_so() {
        let mut k = Keeper::default();
        let bus = FakeBus::default();
        bus.reply(bus::LAUNCH, Ok(json!({"returnValue": true})));
        let now = Instant::now();
        k.on_event(&ev(APP_ID, "launch"), now);
        k.on_event(&ev(APP_ID, "close"), now);
        assert!(!k.running());
        k.launch(&bus, now, true);
        assert!(k.running());
        // SAM's events for that launch are not a relaunch; the next is.
        assert_eq!(k.on_event(&ev(APP_ID, "splash"), now), Ask::Nothing);
        assert_eq!(k.on_event(&ev(APP_ID, "launch"), now), Ask::Nothing);
        assert_eq!(
            k.on_event(&ev(APP_ID, "launch"), now),
            Ask::Relaunched { home_key: false }
        );
    }

    #[test]
    fn says_whether_the_shell_runs() {
        let mut k = Keeper::default();
        let now = Instant::now();
        assert!(!k.running());
        k.on_event(&ev(APP_ID, "launch"), now);
        assert!(k.running());
        k.on_event(&ev(APP_ID, "close"), now);
        assert!(!k.running());
    }

    #[test]
    fn a_launch_while_running_is_a_relaunch() {
        let mut k = Keeper::default();
        let now = Instant::now();
        k.on_event(&ev(APP_ID, "foreground"), now);
        let mut home = ev(APP_ID, "launch");
        home.home_key = true;
        assert_eq!(k.on_event(&home, now), Ask::Relaunched { home_key: true });
    }
}
