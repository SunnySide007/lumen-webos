//! The remote's keys as the compositor sends them, for the core.
//!
//! `wl_keyboard` carries Linux evdev codes (LG's XKB keycodes less 8). The
//! core reads SDL scancodes (`lumen_core::input::scancode`), as LG's SDL
//! gave them, so each code the shell acts on is mapped here. The arrows and
//! OK are the Magic Remote's raw codes on the B9 (b9.md, "Input devices");
//! BACK is LG's XKB 420 (SDL-webOS's table, research of 2026-11-05); the
//! pointer's codes are what the B9's remote node sends (1198 and 1199),
//! which the compositor may or may not pass on. A code not mapped reaches
//! the core as scancode 0 with the evdev code as its keycode, so the log
//! shows it.
//!
//! `KeyClock` works out which clock the compositor stamps keys on: its
//! milliseconds have no stated base, and the B9's input nodes stamp on
//! CLOCK_REALTIME.

use std::time::Duration;

use lumen_core::input::scancode;

/// Linux input codes (linux/input-event-codes.h).
pub mod evdev {
    pub const ESC: u32 = 1;
    pub const ENTER: u32 = 28;
    pub const KP_ENTER: u32 = 96;
    pub const UP: u32 = 103;
    pub const LEFT: u32 = 105;
    pub const RIGHT: u32 = 106;
    pub const DOWN: u32 = 108;
    pub const BACK: u32 = 158;
    /// LG's BACK: XKB 420 less 8.
    pub const LG_BACK: u32 = 412;
    /// LG's Home: XKB 781 less 8; the B9's remote node sends it.
    pub const LG_HOME: u32 = 773;
    /// The Magic Remote's pointer appearing and going (the B9's remote node).
    pub const LG_CURSOR_SHOW: u32 = 1198;
    pub const LG_CURSOR_HIDE: u32 = 1199;
    pub const BTN_LEFT: u32 = 0x110;
    pub const BTN_RIGHT: u32 = 0x111;
    pub const BTN_MIDDLE: u32 = 0x112;
}

/// The SDL scancode for an evdev key code, `None` for one the shell does not
/// know.
pub fn scancode_for(code: u32) -> Option<i32> {
    Some(match code {
        evdev::UP => scancode::UP,
        evdev::DOWN => scancode::DOWN,
        evdev::LEFT => scancode::LEFT,
        evdev::RIGHT => scancode::RIGHT,
        evdev::ENTER => scancode::RETURN,
        evdev::KP_ENTER => scancode::KP_ENTER,
        evdev::ESC => scancode::ESCAPE,
        evdev::BACK => scancode::AC_BACK,
        evdev::LG_BACK => scancode::WEBOS_BACK,
        evdev::LG_HOME => scancode::WEBOS_HOME,
        evdev::LG_CURSOR_SHOW => scancode::WEBOS_CURSOR_SHOW,
        evdev::LG_CURSOR_HIDE => scancode::WEBOS_CURSOR_HIDE,
        _ => return None,
    })
}

/// SDL's button number for a pointer button: 1 left, 2 middle, 3 right.
pub fn mouse_button(code: u32) -> Option<u8> {
    match code {
        evdev::BTN_LEFT => Some(1),
        evdev::BTN_MIDDLE => Some(2),
        evdev::BTN_RIGHT => Some(3),
        _ => None,
    }
}

/// A clock a key's stamp may be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Monotonic,
    Realtime,
}

impl Clock {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Monotonic => "CLOCK_MONOTONIC",
            Self::Realtime => "CLOCK_REALTIME",
        }
    }
}

/// An event older than this is taken to be stamped on another clock.
const MAX_AGE_MS: u32 = 10_000;

/// How long ago a stamp of `stamp_ms` was, by a clock reading `now_ms`, both
/// in wrapping milliseconds; `None` if it cannot be on that clock.
pub fn age(stamp_ms: u32, now_ms: u32) -> Option<Duration> {
    let age = now_ms.wrapping_sub(stamp_ms);
    (age <= MAX_AGE_MS).then(|| Duration::from_millis(u64::from(age)))
}

/// Which clock key stamps are on, learned from the first that fits one.
#[derive(Debug, Default)]
pub struct KeyClock {
    found: Option<Clock>,
    /// The first stamp on neither clock has been said.
    warned: bool,
}

impl KeyClock {
    /// The age of a key stamped `stamp_ms`, given both clocks' readings now
    /// (in wrapping milliseconds), and a line for the log the first time a
    /// clock is found or none fits. The clock found first is tried first.
    pub fn age(
        &mut self,
        stamp_ms: u32,
        monotonic_ms: u32,
        realtime_ms: u32,
    ) -> (Option<Duration>, Option<String>) {
        let reading = |clock| match clock {
            Clock::Monotonic => monotonic_ms,
            Clock::Realtime => realtime_ms,
        };
        let order = match self.found {
            Some(Clock::Realtime) => [Clock::Realtime, Clock::Monotonic],
            _ => [Clock::Monotonic, Clock::Realtime],
        };
        for clock in order {
            if let Some(age) = age(stamp_ms, reading(clock)) {
                let line = (self.found.is_none()).then(|| {
                    format!(
                        "wayland: key times are on {}; the first was {} ms old",
                        clock.name(),
                        age.as_millis()
                    )
                });
                self.found.get_or_insert(clock);
                return (Some(age), line);
            }
        }
        let line = (!std::mem::replace(&mut self.warned, true)).then(|| {
            format!(
                "wayland: a key stamped {stamp_ms} ms fits neither clock \
                 (monotonic {monotonic_ms}, realtime {realtime_ms}): key times \
                 count from when the shell reads each key"
            )
        });
        (None, line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::input::{Action, action_for};

    #[test]
    fn the_remotes_keys_reach_the_core_as_lgs_sdl_gave_them() {
        assert_eq!(scancode_for(evdev::UP), Some(82));
        assert_eq!(scancode_for(evdev::DOWN), Some(81));
        assert_eq!(scancode_for(evdev::LEFT), Some(80));
        assert_eq!(scancode_for(evdev::RIGHT), Some(79));
        assert_eq!(scancode_for(evdev::ENTER), Some(40));
        assert_eq!(scancode_for(evdev::LG_BACK), Some(482));
        assert_eq!(scancode_for(evdev::LG_CURSOR_SHOW), Some(484));
        assert_eq!(scancode_for(evdev::LG_CURSOR_HIDE), Some(485));
        assert_eq!(
            scancode_for(evdev::LG_BACK).and_then(action_for),
            Some(Action::Back)
        );
        assert_eq!(scancode_for(174), None, "EXIT");
        assert_eq!(scancode_for(30), None, "a letter is not the remote's");
    }

    #[test]
    fn buttons_are_numbered_as_sdl_numbers_them() {
        assert_eq!(mouse_button(0x110), Some(1));
        assert_eq!(mouse_button(0x112), Some(2));
        assert_eq!(mouse_button(0x111), Some(3));
        assert_eq!(mouse_button(0x113), None);
    }

    #[test]
    fn ages_wrap_and_stop_at_ten_seconds() {
        assert_eq!(age(1000, 1025), Some(Duration::from_millis(25)));
        assert_eq!(age(u32::MAX - 4, 5), Some(Duration::from_millis(10)));
        assert_eq!(age(1001, 1000), None, "from the future");
        assert_eq!(age(0, 10_001), None);
    }

    #[test]
    fn the_clock_is_found_once_and_tried_first() {
        let mut clock = KeyClock::default();
        // Stamped on the realtime clock, which reads far from the monotonic.
        let (age, line) = clock.age(5_000_020, 1_000, 5_000_040);
        assert_eq!(age, Some(Duration::from_millis(20)));
        assert_eq!(
            line.as_deref(),
            Some("wayland: key times are on CLOCK_REALTIME; the first was 20 ms old")
        );
        let (age, line) = clock.age(5_000_100, 1_100, 5_000_103);
        assert_eq!((age, line), (Some(Duration::from_millis(3)), None));
        // A stamp on no clock: said once, then quietly read at its time.
        let (age, line) = clock.age(7, 90_000, 9_000_000);
        assert_eq!(age, None);
        assert!(line.is_some_and(|l| l.contains("fits neither clock")));
        assert_eq!(clock.age(7, 90_000, 9_000_000), (None, None));
    }

    #[test]
    fn monotonic_is_tried_first_until_a_clock_is_found() {
        let mut clock = KeyClock::default();
        let (age, line) = clock.age(500, 510, 9_999_999);
        assert_eq!(age, Some(Duration::from_millis(10)));
        assert!(line.is_some_and(|l| l.contains("CLOCK_MONOTONIC")));
    }
}
