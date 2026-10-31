//! What the remote's keys mean, and when they fire.
//!
//! Arrows fire on press and repeat while held, on the shell's own clock (400 ms,
//! then every 120 ms), so the PC and the TV move at the same rate. SDL's own
//! repeat events never fire anything; they only show a key is still held. A
//! held key that sees neither a repeat nor a release for 1.5 s counts as
//! released, so a lost key-up cannot scroll forever.

use std::time::Duration;

use crate::focus::Dir;

/// SDL scancodes the shell uses. The arrows and Return are upstream SDL's and
/// measured the same on the B9; the webOS ones are LG's (webosbrew's SDL fork
/// names them `SDL_SCANCODE_WEBOS_*`).
pub mod scancode {
    pub const RETURN: i32 = 40;
    pub const ESCAPE: i32 = 41;
    pub const RIGHT: i32 = 79;
    pub const LEFT: i32 = 80;
    pub const DOWN: i32 = 81;
    pub const UP: i32 = 82;
    pub const KP_ENTER: i32 = 88;
    pub const AC_BACK: i32 = 270;
    /// Never reaches an app on the B9: LG's compositor takes it.
    pub const WEBOS_HOME: i32 = 384;
    pub const WEBOS_BACK: i32 = 482;
    /// The Magic Remote's pointer appeared (`SDL_SCANCODE_WEBOS_CURSOR_SHOW`
    /// in webosbrew's SDL fork). LG sends it as a key-down with no key-up.
    pub const WEBOS_CURSOR_SHOW: i32 = 484;
    /// The pointer went away (`SDL_SCANCODE_WEBOS_CURSOR_HIDE`), the same way.
    pub const WEBOS_CURSOR_HIDE: i32 = 485;
}

pub const REPEAT_DELAY: Duration = Duration::from_millis(400);
pub const REPEAT_INTERVAL: Duration = Duration::from_millis(120);
pub const STUCK_AFTER: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move(Dir),
    Select,
    Back,
}

/// An action, and whether it came from holding a key down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Press {
    pub action: Action,
    pub repeat: bool,
}

/// The action a scancode stands for. Everything else is no action, including
/// the pointer codes (`WEBOS_CURSOR_SHOW` and `WEBOS_CURSOR_HIDE`), which the
/// shell only notes.
pub fn action_for(code: i32) -> Option<Action> {
    use scancode::*;
    match code {
        UP => Some(Action::Move(Dir::Up)),
        DOWN => Some(Action::Move(Dir::Down)),
        LEFT => Some(Action::Move(Dir::Left)),
        RIGHT => Some(Action::Move(Dir::Right)),
        RETURN | KP_ENTER => Some(Action::Select),
        ESCAPE | AC_BACK | WEBOS_BACK => Some(Action::Back),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Held {
    code: i32,
    action: Action,
    /// When this key last showed signs of life: its press or an SDL repeat.
    seen: Duration,
    /// The next generated repeat; only arrows have one.
    next_repeat: Option<Duration>,
}

/// Which keys are down, and the repeats they are owed.
#[derive(Debug, Default)]
pub struct Input {
    held: Vec<Held>,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// A key went down at `now`. Returns the action it fires, if any. Give
    /// each key event its own time (when it happened, not when it was read),
    /// and call `poll` at a press's or a release's time before it, so the
    /// repeats that fell due first fire, and none due after a release does.
    #[must_use]
    pub fn key_down(&mut self, code: i32, sdl_repeat: bool, now: Duration) -> Option<Press> {
        // A key quiet for STUCK_AFTER was released, even if no poll said so yet.
        self.held.retain(|h| now < h.seen + STUCK_AFTER);
        if let Some(held) = self.held.iter_mut().find(|h| h.code == code) {
            held.seen = now;
            return None;
        }
        // A repeat for a key whose press we never saw (it went down while the
        // window was in the background) starts nothing.
        if sdl_repeat {
            return None;
        }
        let action = action_for(code)?;
        let next_repeat = matches!(action, Action::Move(_)).then(|| now + REPEAT_DELAY);
        self.held.push(Held {
            code,
            action,
            seen: now,
            next_repeat,
        });
        Some(Press {
            action,
            repeat: false,
        })
    }

    /// A key came up. Releases nothing that was not down.
    pub fn key_up(&mut self, code: i32) {
        self.held.retain(|h| h.code != code);
    }

    /// Drops every held key without firing: the window lost focus or went to
    /// the background.
    pub fn release_all(&mut self) {
        self.held.clear();
    }

    pub fn is_holding(&self) -> bool {
        !self.held.is_empty()
    }

    /// Repeats due by `now`, at most one per key: after a long frame the
    /// missed ones are dropped rather than fired in a burst. Also releases
    /// keys that have gone quiet for `STUCK_AFTER`.
    #[must_use]
    pub fn poll(&mut self, now: Duration) -> Vec<Press> {
        let mut out = Vec::new();
        for held in &mut self.held {
            let stuck_at = held.seen + STUCK_AFTER;
            if let Some(due) = held.next_repeat
                && due <= now
                && due < stuck_at
            {
                out.push(Press {
                    action: held.action,
                    repeat: true,
                });
                let next = due + REPEAT_INTERVAL;
                held.next_repeat = Some(if next <= now {
                    now + REPEAT_INTERVAL
                } else {
                    next
                });
            }
        }
        self.held.retain(|h| now < h.seen + STUCK_AFTER);
        out
    }

    /// When `poll` next has something to do.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.held
            .iter()
            .flat_map(|h| [h.next_repeat, Some(h.seen + STUCK_AFTER)])
            .flatten()
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn press(action: Action) -> Option<Press> {
        Some(Press {
            action,
            repeat: false,
        })
    }

    const RIGHT: Action = Action::Move(Dir::Right);

    #[test]
    fn maps_the_b9_remote_and_the_pc_keyboard() {
        assert_eq!(action_for(82), Some(Action::Move(Dir::Up)));
        assert_eq!(action_for(81), Some(Action::Move(Dir::Down)));
        assert_eq!(action_for(80), Some(Action::Move(Dir::Left)));
        assert_eq!(action_for(79), Some(RIGHT));
        assert_eq!(action_for(40), Some(Action::Select));
        assert_eq!(action_for(88), Some(Action::Select));
        assert_eq!(action_for(482), Some(Action::Back));
        assert_eq!(action_for(270), Some(Action::Back));
        assert_eq!(action_for(41), Some(Action::Back));
    }

    #[test]
    fn ignores_pointer_keys_home_and_everything_else() {
        for code in [484, 485, scancode::WEBOS_HOME, 0, 1, 4, 257] {
            assert_eq!(action_for(code), None, "scancode {code}");
            let mut input = Input::new();
            assert_eq!(input.key_down(code, false, ms(0)), None);
            assert!(!input.is_holding());
        }
    }

    #[test]
    fn an_arrow_fires_on_press_then_repeats_while_held() {
        let mut input = Input::new();
        assert_eq!(input.key_down(scancode::RIGHT, false, ms(0)), press(RIGHT));
        assert_eq!(input.poll(ms(399)), vec![]);
        assert_eq!(input.next_deadline(), Some(ms(400)));
        let repeat = Press {
            action: RIGHT,
            repeat: true,
        };
        assert_eq!(input.poll(ms(400)), vec![repeat]);
        assert_eq!(input.next_deadline(), Some(ms(520)));
        // SDL's own repeats keep the key alive but fire nothing.
        assert_eq!(input.key_down(scancode::RIGHT, true, ms(500)), None);
        assert_eq!(input.poll(ms(520)), vec![repeat]);
        assert_eq!(input.poll(ms(640)), vec![repeat]);
        input.key_up(scancode::RIGHT);
        assert_eq!(input.poll(ms(760)), vec![]);
        assert_eq!(input.next_deadline(), None);
    }

    #[test]
    fn polled_at_the_release_a_repeat_due_after_it_never_fires() {
        let mut input = Input::new();
        let _ = input.key_down(scancode::RIGHT, false, ms(0));
        assert_eq!(input.poll(ms(400)).len(), 1);
        // Released at 510, read at 525: the repeat due at 520 is not owed.
        assert_eq!(input.poll(ms(510)), vec![]);
        input.key_up(scancode::RIGHT);
        assert_eq!(input.poll(ms(525)), vec![]);
    }

    #[test]
    fn select_and_back_never_repeat() {
        let mut input = Input::new();
        assert_eq!(
            input.key_down(scancode::RETURN, false, ms(0)),
            press(Action::Select)
        );
        assert_eq!(
            input.key_down(scancode::WEBOS_BACK, false, ms(0)),
            press(Action::Back)
        );
        assert_eq!(input.poll(ms(1000)), vec![]);
    }

    #[test]
    fn a_second_press_of_a_held_key_is_ignored() {
        let mut input = Input::new();
        assert!(input.key_down(scancode::DOWN, false, ms(0)).is_some());
        assert_eq!(input.key_down(scancode::DOWN, false, ms(50)), None);
    }

    #[test]
    fn a_release_without_a_press_is_ignored() {
        let mut input = Input::new();
        input.key_up(scancode::LEFT);
        assert!(!input.is_holding());
        assert!(input.key_down(scancode::LEFT, false, ms(0)).is_some());
    }

    #[test]
    fn a_repeat_for_a_key_never_pressed_starts_nothing() {
        let mut input = Input::new();
        assert_eq!(input.key_down(scancode::LEFT, true, ms(0)), None);
        assert!(!input.is_holding());
        assert_eq!(input.poll(ms(1000)), vec![]);
    }

    #[test]
    fn a_long_frame_fires_one_repeat_not_a_burst() {
        let mut input = Input::new();
        let _ = input.key_down(scancode::UP, false, ms(0));
        assert_eq!(input.poll(ms(900)).len(), 1);
        assert_eq!(input.next_deadline(), Some(ms(1020)));
    }

    #[test]
    fn a_key_that_goes_quiet_counts_as_released() {
        let mut input = Input::new();
        let _ = input.key_down(scancode::LEFT, false, ms(0));
        // Repeats run until the key has been quiet for STUCK_AFTER...
        let mut fired = 0;
        let mut t = 0;
        while input.is_holding() {
            t += 10;
            fired += input.poll(ms(t)).len();
            assert!(t < 3000, "never released");
        }
        assert_eq!(t, 1500);
        // ...which allows the repeats at 400, 520, ... 1480: ten of them.
        assert_eq!(fired, 10);
        assert_eq!(input.next_deadline(), None);
    }

    #[test]
    fn a_press_after_a_stuck_key_expired_fires_even_without_a_poll() {
        let mut input = Input::new();
        assert!(input.key_down(scancode::RIGHT, false, ms(0)).is_some());
        assert_eq!(
            input.key_down(scancode::RIGHT, false, ms(1600)),
            press(RIGHT)
        );
    }

    #[test]
    fn two_held_arrows_repeat_independently() {
        let mut input = Input::new();
        assert!(input.key_down(scancode::LEFT, false, ms(0)).is_some());
        assert!(input.key_down(scancode::UP, false, ms(100)).is_some());
        assert_eq!(input.poll(ms(400)).len(), 1);
        assert_eq!(input.poll(ms(500)).len(), 1);
        input.key_up(scancode::LEFT);
        assert_eq!(input.poll(ms(620)).len(), 1, "only Up is left");
    }

    #[test]
    fn sdl_repeats_keep_a_held_key_alive() {
        let mut input = Input::new();
        let _ = input.key_down(scancode::LEFT, false, ms(0));
        for t in (100..=3000).step_by(100) {
            let _ = input.key_down(scancode::LEFT, true, ms(t));
            let _ = input.poll(ms(t));
        }
        assert!(input.is_holding());
    }

    #[test]
    fn release_all_drops_everything_silently() {
        let mut input = Input::new();
        let _ = input.key_down(scancode::LEFT, false, ms(0));
        let _ = input.key_down(scancode::RETURN, false, ms(0));
        input.release_all();
        assert!(!input.is_holding());
        assert_eq!(input.poll(ms(500)), vec![]);
    }
}
