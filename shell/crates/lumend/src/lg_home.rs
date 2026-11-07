//! LG's Home closed while Lumen is the TV's Home (route D; takeover/README.md),
//! as `takeover/lumen-home` closed it after boot, but on SAM's events instead
//! of a minute's sleep, and for as long as lumend runs.
//!
//! With Lumen as Home, SAM still preloads LG's Home on snapshot boots and
//! keeps it alive, and it runs unseen, as root and outside any jail, calling
//! LG's servers (privacy.md, layer 2). On each app life event that concerns
//! it, or any app coming to the front, lumend looks 10 s later (so a quick
//! trip through LG's Home isn't fought), and closes LG's Home the way LG's
//! own `home_launcher.sh` closes the home it doesn't use
//! (`closeByAppId` with `forceMode`) when all of these hold:
//!
//! * the default home is Lumen's id (it never sets it: only
//!   `tools/takeover.sh apply` does);
//! * LG's Home is running and not in front (its tile in Lumen opens it);
//! * no close in the last minute, and fewer than three in ten minutes: if
//!   keep-alive keeps bringing it back, lumend leaves it until the next
//!   boot rather than fight.
//!
//! 10 s after a close it logs whether LG's Home stayed closed, is still on
//! its way out (the same pid: the B9 took more than 10 s on 2026-10-31), or
//! came back (another pid).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::bus::{self, Bus, LifeEvent};
use crate::paths::{APP_ID, LG_HOME};

/// How long after an event lumend looks.
pub const GRACE: Duration = Duration::from_secs(10);
/// How long after a close lumend looks whether it held.
pub const VERIFY: Duration = Duration::from_secs(10);
const MIN_GAP: Duration = Duration::from_secs(60);
const WINDOW: Duration = Duration::from_secs(600);
const MAX_IN_WINDOW: usize = 3;

/// What comes after a look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Nothing,
    /// Look again then.
    LookAt(Instant),
    /// LG's Home was closed: check at that time, with the pid it had.
    VerifyAt(Instant, Option<String>),
}

#[derive(Debug, Default)]
pub struct LgHome {
    closes: VecDeque<Instant>,
    last_try: Option<Instant>,
    pub gave_up: bool,
}

/// Whether a life event is a reason to look.
pub fn wants_look(event: &LifeEvent) -> bool {
    event.app == LG_HOME || event.event == "foreground"
}

impl LgHome {
    /// One look; the lines say what was done.
    pub fn look(&mut self, bus: &dyn Bus, now: Instant) -> (Vec<String>, Next) {
        let mut lines = Vec::new();
        if self.gave_up {
            return (lines, Next::Nothing);
        }
        match bus::default_home(bus) {
            Ok(home) if home == APP_ID => {}
            Ok(home) => {
                lines.push(format!(
                    "lg-home: the default home is {home}, not Lumen; nothing done"
                ));
                return (lines, Next::Nothing);
            }
            Err(e) => {
                lines.push(format!(
                    "lg-home: the default home is unreadable ({e}); nothing done"
                ));
                return (lines, Next::Nothing);
            }
        }
        let list = match bus::running(bus) {
            Ok(list) => list,
            Err(e) => {
                lines.push(format!("lg-home: SAM's running list is unreadable: {e}"));
                return (lines, Next::Nothing);
            }
        };
        let Some(lg) = list.iter().find(|a| a.id == LG_HOME) else {
            return (lines, Next::Nothing);
        };
        match bus::foreground(bus) {
            Ok(Some(front)) if front == LG_HOME => {
                lines.push("lg-home: LG Home is in front; left alone".to_owned());
                return (lines, Next::Nothing);
            }
            Ok(_) => {}
            Err(e) => {
                lines.push(format!(
                    "lg-home: the app in front is unreadable ({e}); nothing done"
                ));
                return (lines, Next::Nothing);
            }
        }
        if let Some(last) = self.last_try
            && now < last + MIN_GAP
        {
            return (lines, Next::LookAt(last + MIN_GAP));
        }
        while self
            .closes
            .front()
            .is_some_and(|t| now.duration_since(*t) > WINDOW)
        {
            self.closes.pop_front();
        }
        if self.closes.len() >= MAX_IN_WINDOW {
            self.gave_up = true;
            lines.push(format!(
                "lg-home: LG Home came back after {MAX_IN_WINDOW} closes in ten minutes; left alone until the next boot"
            ));
            return (lines, Next::Nothing);
        }
        self.last_try = Some(now);
        let pid = lg.pid.clone();
        let (uri, payload) = bus::close_lg_home();
        match bus::checked(bus, uri, &payload) {
            Ok(_) => {
                self.closes.push_back(now);
                lines.push(format!(
                    "lg-home: closed LG Home (pid {})",
                    pid.as_deref().unwrap_or("unknown")
                ));
                (lines, Next::VerifyAt(now + VERIFY, pid))
            }
            Err(e) => {
                lines.push(format!("lg-home: FAILED to close LG Home: {e}"));
                (lines, Next::Nothing)
            }
        }
    }
}

/// What SAM's list says 10 s after a close.
pub fn verify(bus: &dyn Bus, pid: Option<&str>) -> String {
    let list = match bus::running(bus) {
        Ok(list) => list,
        Err(e) => return format!("lg-home: SAM didn't answer 10 s later: {e}"),
    };
    let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
    let running = crate::log::clean(&ids.join(" "));
    match (list.iter().find(|a| a.id == LG_HOME), pid) {
        (None, _) => format!("lg-home: LG Home stayed closed 10 s later; running: {running}"),
        (Some(now), Some(was)) if now.pid.as_deref() == Some(was) => {
            format!("lg-home: LG Home still exiting 10 s later (pid {was}); running: {running}")
        }
        (Some(now), Some(was)) if now.pid.is_some() => format!(
            "lg-home: LG Home is back 10 s later as pid {} (was {was}); running: {running}",
            now.pid.as_deref().unwrap_or("?")
        ),
        _ => format!("lg-home: LG Home still listed 10 s later, pid unknown; running: {running}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::fake::FakeBus;
    use serde_json::json;

    fn tv(home: &str, running: serde_json::Value, front: &str) -> FakeBus {
        let bus = FakeBus::default();
        bus.reply(
            bus::GET_SETTINGS,
            Ok(json!({"returnValue": true, "settings": {"defaultApps": {"home": home}}})),
        );
        bus.reply(
            bus::RUNNING,
            Ok(json!({"returnValue": true, "running": running})),
        );
        bus.reply(
            bus::FOREGROUND,
            Ok(json!({"returnValue": true, "appId": front})),
        );
        bus.reply(bus::CLOSE, Ok(json!({"returnValue": true})));
        bus
    }

    fn with_lg() -> serde_json::Value {
        json!([{"id": LG_HOME, "processid": "1736"}, {"id": "com.webos.app.hdmi3", "processid": "900"}])
    }

    #[test]
    fn closes_lg_home_when_lumen_is_home_and_lg_is_behind() {
        let bus = tv(APP_ID, with_lg(), "com.webos.app.hdmi3");
        let mut part = LgHome::default();
        let now = Instant::now();
        let (lines, next) = part.look(&bus, now);
        assert_eq!(lines, ["lg-home: closed LG Home (pid 1736)"]);
        assert_eq!(next, Next::VerifyAt(now + VERIFY, Some("1736".to_owned())));
        assert_eq!(
            bus.calls_to(bus::CLOSE),
            [json!({"id": LG_HOME, "forceMode": true})]
        );
    }

    #[test]
    fn never_acts_unless_lumen_is_the_default_home() {
        let bus = tv(LG_HOME, with_lg(), "netflix");
        let (lines, next) = LgHome::default().look(&bus, Instant::now());
        assert!(lines[0].contains("not Lumen"));
        assert_eq!(next, Next::Nothing);
        assert!(bus.calls_to(bus::CLOSE).is_empty());
    }

    #[test]
    fn leaves_lg_home_in_front_and_does_nothing_when_unsure() {
        let bus = tv(APP_ID, with_lg(), LG_HOME);
        let (lines, _) = LgHome::default().look(&bus, Instant::now());
        assert!(lines[0].contains("in front; left alone"));
        let bus = tv(APP_ID, with_lg(), "x");
        bus.replies.lock().unwrap().remove(bus::FOREGROUND);
        let (lines, _) = LgHome::default().look(&bus, Instant::now());
        assert!(lines[0].contains("unreadable"));
        assert!(bus.calls_to(bus::CLOSE).is_empty());
    }

    #[test]
    fn says_nothing_when_lg_home_isnt_running() {
        let bus = tv(APP_ID, json!([{"id": "netflix"}]), "netflix");
        let (lines, next) = LgHome::default().look(&bus, Instant::now());
        assert!(lines.is_empty());
        assert_eq!(next, Next::Nothing);
    }

    #[test]
    fn waits_a_minute_between_closes_and_gives_up_after_three() {
        let bus = tv(APP_ID, with_lg(), "netflix");
        let mut part = LgHome::default();
        let t0 = Instant::now();
        part.look(&bus, t0);
        let (_, next) = part.look(&bus, t0 + Duration::from_secs(5));
        assert_eq!(next, Next::LookAt(t0 + MIN_GAP));
        part.look(&bus, t0 + Duration::from_secs(61));
        part.look(&bus, t0 + Duration::from_secs(122));
        let (lines, next) = part.look(&bus, t0 + Duration::from_secs(183));
        assert!(
            lines[0].contains("left alone until the next boot"),
            "{lines:?}"
        );
        assert_eq!(next, Next::Nothing);
        assert!(part.gave_up);
        assert_eq!(bus.calls_to(bus::CLOSE).len(), 3);
    }

    #[test]
    fn tells_a_slow_exit_from_a_restart() {
        let bus = FakeBus::default();
        bus.reply(
            bus::RUNNING,
            Ok(json!({"returnValue": true, "running": with_lg()})),
        );
        assert!(verify(&bus, Some("1736")).contains("still exiting"));
        assert!(verify(&bus, Some("12")).contains("back 10 s later as pid 1736 (was 12)"));
        let bus = FakeBus::default();
        bus.reply(
            bus::RUNNING,
            Ok(json!({"returnValue": true, "running": []})),
        );
        assert!(verify(&bus, Some("1736")).contains("stayed closed"));
    }

    #[test]
    fn looks_on_lg_homes_events_and_any_foreground() {
        let e = |app: &str, event: &str| LifeEvent {
            app: app.to_owned(),
            event: event.to_owned(),
            home_key: false,
        };
        assert!(wants_look(&e(LG_HOME, "launch")));
        assert!(wants_look(&e("netflix", "foreground")));
        assert!(!wants_look(&e("netflix", "background")));
    }
}
