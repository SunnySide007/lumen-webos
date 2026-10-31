//! Scripted keys for automation: `--keys "R,R,D,OK"`. Each key becomes a
//! press and a release at fixed times, delivered as scancodes through the same
//! input path as real keys.

use std::time::Duration;

use crate::input::scancode;

/// When the first key goes down: after the first frames have settled.
pub const START: Duration = Duration::from_millis(500);
/// How long each key is held.
pub const HOLD: Duration = Duration::from_millis(60);
/// From one key's release to the next key's press.
pub const GAP: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub at: Duration,
    pub scancode: i32,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    events: Vec<KeyEvent>,
    next: usize,
}

fn token_scancode(token: &str) -> Option<i32> {
    match token.to_ascii_uppercase().as_str() {
        "U" => Some(scancode::UP),
        "D" => Some(scancode::DOWN),
        "L" => Some(scancode::LEFT),
        "R" => Some(scancode::RIGHT),
        "OK" => Some(scancode::RETURN),
        "BACK" => Some(scancode::WEBOS_BACK),
        _ => None,
    }
}

impl Script {
    /// Parses a comma-separated list of `U D L R OK BACK`, in any case.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut events = Vec::new();
        let mut at = START;
        for token in spec.split(',').map(str::trim) {
            let code = token_scancode(token).ok_or_else(|| {
                format!("unknown key {token:?} in --keys; use U, D, L, R, OK or BACK")
            })?;
            events.push(KeyEvent {
                at,
                scancode: code,
                down: true,
            });
            events.push(KeyEvent {
                at: at + HOLD,
                scancode: code,
                down: false,
            });
            at += HOLD + GAP;
        }
        Ok(Self { events, next: 0 })
    }

    /// Events due by `now`, in order; each is returned once.
    #[must_use]
    pub fn due(&mut self, now: Duration) -> &[KeyEvent] {
        let start = self.next;
        while self.next < self.events.len() && self.events[self.next].at <= now {
            self.next += 1;
        }
        &self.events[start..self.next]
    }

    /// When the next event is due, or `None` once all have been played.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.events.get(self.next).map(|e| e.at)
    }

    /// Every key has been pressed and released.
    pub fn is_done(&self) -> bool {
        self.next == self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn parses_every_token_in_any_case() {
        let script = Script::parse("u, d,L,r ,ok,Back").expect("valid");
        let downs: Vec<i32> = script
            .events
            .iter()
            .filter(|e| e.down)
            .map(|e| e.scancode)
            .collect();
        assert_eq!(downs, vec![82, 81, 80, 79, 40, 482]);
    }

    #[test]
    fn keys_are_spaced_on_a_fixed_timeline() {
        let script = Script::parse("R,D").expect("valid");
        let times: Vec<(u64, bool)> = script
            .events
            .iter()
            .map(|e| (e.at.as_millis() as u64, e.down))
            .collect();
        assert_eq!(
            times,
            vec![(500, true), (560, false), (860, true), (920, false)]
        );
    }

    #[test]
    fn rejects_unknown_and_empty_tokens() {
        assert!(Script::parse("R,X").unwrap_err().contains("\"X\""));
        assert!(Script::parse("R,,D").is_err());
        assert!(Script::parse("").is_err());
        assert!(Script::parse("HOME").is_err(), "HOME never reaches the app");
    }

    #[test]
    fn plays_each_event_once_in_order() {
        let mut script = Script::parse("R,OK").expect("valid");
        assert_eq!(script.next_deadline(), Some(ms(500)));
        assert!(script.due(ms(499)).is_empty());
        let first: Vec<bool> = script.due(ms(560)).iter().map(|e| e.down).collect();
        assert_eq!(first, vec![true, false]);
        assert!(script.due(ms(560)).is_empty());
        assert_eq!(script.next_deadline(), Some(ms(860)));
        assert!(!script.is_done());
        assert_eq!(script.due(ms(5000)).len(), 2);
        assert!(script.is_done());
        assert_eq!(script.next_deadline(), None);
    }
}
