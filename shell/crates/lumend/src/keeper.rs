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
            "launch" | "relaunch" if self.running => Ask::Relaunched {
                home_key: e.home_key,
            },
            "launch" | "relaunch" | "splash" | "preload" | "foreground" | "background" => {
                self.running = true;
                self.started = true;
                Ask::Nothing
            }
            "stop" | "close" => {
                self.running = false;
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
            Ok(_) => format!("shell: {what}, hidden"),
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
            [json!({"id": APP_ID, "params": {"hidden": true}})]
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
    fn a_launch_while_running_is_a_relaunch() {
        let mut k = Keeper::default();
        let now = Instant::now();
        k.on_event(&ev(APP_ID, "foreground"), now);
        let mut home = ev(APP_ID, "launch");
        home.home_key = true;
        assert_eq!(k.on_event(&home, now), Ask::Relaunched { home_key: true });
    }
}
