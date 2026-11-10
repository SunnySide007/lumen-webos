//! The Control Center (docs/specs/look-and-feel.md, "The Control Center"):
//! a panel the gear on the home opens, over the right of the screen. It
//! shows the Privacy panel, which only reads (decision 12), and Reduce
//! Motion: one switch per moving effect, editable on the TV (decision 7),
//! driving the same switches as the launch parameters' `"look"`.
//!
//! Reduce Motion can only take an effect away: what the launch look turns
//! off stays off, and its row says so.

use std::fmt;

use crate::anim::Spring;
use crate::art::Look;
use crate::privacy::{self, Reading};
use crate::style;

/// The moving effects Reduce Motion lists, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Tilt,
    Light,
    Parallax,
    Slide,
    Fade,
}

impl Motion {
    pub const ALL: [Self; 5] = [
        Self::Tilt,
        Self::Light,
        Self::Parallax,
        Self::Slide,
        Self::Fade,
    ];

    /// Its word, as `"look"` and the saved file say it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Tilt => "tilt",
            Self::Light => "light",
            Self::Parallax => "parallax",
            Self::Slide => "slide",
            Self::Fade => "fade",
        }
    }

    /// Its row's name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tilt => "Tilt",
            Self::Light => "Light sweep",
            Self::Parallax => "Parallax",
            Self::Slide => "Slides",
            Self::Fade => "Crossfades",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn in_look(self, look: &mut Look) -> &mut bool {
        match self {
            Self::Tilt => &mut look.tilt,
            Self::Light => &mut look.light,
            Self::Parallax => &mut look.parallax,
            Self::Slide => &mut look.slide,
            Self::Fade => &mut look.fade,
        }
    }

    fn of(self, look: Look) -> bool {
        let mut look = look;
        *self.in_look(&mut look)
    }
}

/// Reduce Motion: the effects the owner turned off on the TV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reduced([bool; 5]);

impl Reduced {
    pub fn is_off(&self, motion: Motion) -> bool {
        self.0[motion.index()]
    }

    pub fn set(&mut self, motion: Motion, off: bool) {
        self.0[motion.index()] = off;
    }

    /// `look` with these effects taken away.
    pub fn apply(&self, look: Look) -> Look {
        let mut look = look;
        for motion in Motion::ALL {
            if self.is_off(motion) {
                *motion.in_look(&mut look) = false;
            }
        }
        look
    }

    /// The saved form: the words of the effects turned off, one space
    /// apart (`tilt slide`), or nothing.
    pub fn to_words(&self) -> String {
        Motion::ALL
            .iter()
            .filter(|m| self.is_off(**m))
            .map(|m| m.word())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Reads the saved form. A word that names no effect is passed over: a
    /// damaged file must never keep the home from starting.
    pub fn from_words(text: &str) -> Self {
        let mut reduced = Self::default();
        for word in text.split_whitespace() {
            if let Some(motion) = Motion::ALL.iter().find(|m| m.word() == word) {
                reduced.set(*motion, true);
            }
        }
        reduced
    }
}

/// For the log: `reduce motion: tilt, slide off` or `reduce motion: none`.
impl fmt::Display for Reduced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let off: Vec<&str> = Motion::ALL
            .iter()
            .filter(|m| self.is_off(**m))
            .map(|m| m.word())
            .collect();
        if off.is_empty() {
            write!(f, "reduce motion: none")
        } else {
            write!(f, "reduce motion: {} off", off.join(", "))
        }
    }
}

/// One Reduce Motion row as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionRow {
    pub label: &'static str,
    /// Whether the effect runs: on in the launch look and not turned off.
    pub on: bool,
    /// Whether OK changes it: not if the launch look turned it off.
    pub editable: bool,
    pub focused: bool,
    /// The row's focus highlight, 0 to 1, on its spring.
    pub lift: f32,
}

/// The Control Center as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlView {
    /// How far it has come in, 0 to 1: the panel's opacity and the dim's.
    pub shown: f32,
    /// How far right of its place the panel still is, in pixels.
    pub offset: f32,
    pub privacy: Vec<privacy::Line>,
    pub motion: Vec<MotionRow>,
}

/// The Control Center's state: whether it is open, the focused row, the
/// Privacy panel's reading.
#[derive(Debug)]
pub struct Control {
    shown: Spring,
    focus: usize,
    lifts: [Spring; 5],
    reading: Reading,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            shown: Spring::new(0.0, style::PAGE),
            focus: 0,
            lifts: [Spring::new(0.0, style::LIFT); 5],
            reading: Reading::Unread,
        }
    }
}

impl Control {
    /// Open, or opening.
    pub fn is_open(&self) -> bool {
        self.shown.target() > 0.5
    }

    /// Opens on the first row, with the Privacy panel reading.
    pub fn open(&mut self) {
        self.shown.set_target(1.0);
        self.lifts = [Spring::new(0.0, style::LIFT); 5];
        self.lifts[0] = Spring::new(1.0, style::LIFT);
        self.focus = 0;
        if !matches!(self.reading, Reading::Read(_)) {
            self.reading = Reading::Reading;
        }
    }

    pub fn close(&mut self) {
        self.shown.set_target(0.0);
    }

    /// Moves focus one row up (`-1`) or down (`1`); false at an end.
    pub fn step(&mut self, by: isize) -> bool {
        let Some(to) = self
            .focus
            .checked_add_signed(by)
            .filter(|&i| i < Motion::ALL.len())
        else {
            return false;
        };
        self.lifts[self.focus].set_target(0.0);
        self.lifts[to].set_target(1.0);
        self.focus = to;
        true
    }

    pub fn focused(&self) -> Motion {
        Motion::ALL[self.focus]
    }

    pub fn set_reading(&mut self, reading: Reading) {
        self.reading = reading;
    }

    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    pub fn springs_mut(&mut self) -> impl Iterator<Item = &mut Spring> {
        std::iter::once(&mut self.shown).chain(self.lifts.iter_mut())
    }

    pub fn springs(&self) -> impl Iterator<Item = &Spring> {
        std::iter::once(&self.shown).chain(self.lifts.iter())
    }

    /// What to draw, with the launch look and Reduce Motion for the rows
    /// and `look` (the two together) for the slide; `None` once closed.
    /// Open but not yet stepped, it is there with nothing shown.
    pub fn view(&self, launch: Look, reduced: Reduced, look: Look) -> Option<ControlView> {
        let shown = self.shown.value().clamp(0.0, 1.0);
        if shown <= 0.001 && !self.is_open() {
            return None;
        }
        let slide = if look.slide { style::PANEL_SLIDE } else { 0.0 };
        Some(ControlView {
            shown,
            offset: slide * (1.0 - shown),
            privacy: privacy::lines(&self.reading),
            motion: Motion::ALL
                .iter()
                .enumerate()
                .map(|(i, &m)| MotionRow {
                    label: m.label(),
                    on: m.of(launch) && !reduced.is_off(m),
                    editable: m.of(launch),
                    focused: self.is_open() && i == self.focus,
                    lift: self.lifts[i].value().clamp(0.0, 1.0),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_motion_takes_effects_away_and_never_adds_one() {
        let mut reduced = Reduced::default();
        assert_eq!(reduced.apply(Look::FULL), Look::FULL);
        reduced.set(Motion::Tilt, true);
        reduced.set(Motion::Fade, true);
        let look = reduced.apply(Look::FULL);
        assert!(!look.tilt && !look.fade && look.light && look.hero && look.dim);
        assert_eq!(reduced.apply(Look::PLAIN), Look::PLAIN);
        assert_eq!(reduced.to_string(), "reduce motion: tilt, fade off");
        assert_eq!(Reduced::default().to_string(), "reduce motion: none");
    }

    #[test]
    fn it_saves_as_words_and_reads_back_passing_over_the_rest() {
        let mut reduced = Reduced::default();
        reduced.set(Motion::Parallax, true);
        reduced.set(Motion::Slide, true);
        assert_eq!(reduced.to_words(), "parallax slide");
        assert_eq!(Reduced::from_words("parallax slide\n"), reduced);
        assert_eq!(Reduced::from_words(" slide  sparkle\tparallax "), reduced);
        assert_eq!(Reduced::from_words(""), Reduced::default());
        assert_eq!(Reduced::default().to_words(), "");
    }

    #[test]
    fn rows_say_what_runs_and_what_the_launch_look_keeps_off() {
        let mut control = Control::default();
        assert!(
            control
                .view(Look::FULL, Reduced::default(), Look::FULL)
                .is_none()
        );
        control.open();
        control.shown.snap();
        let launch = Look {
            light: false,
            ..Look::FULL
        };
        let mut reduced = Reduced::default();
        reduced.set(Motion::Tilt, true);
        let view = control
            .view(launch, reduced, reduced.apply(launch))
            .expect("open");
        let rows: Vec<(&str, bool, bool, bool)> = view
            .motion
            .iter()
            .map(|r| (r.label, r.on, r.editable, r.focused))
            .collect();
        assert_eq!(
            rows,
            [
                ("Tilt", false, true, true),
                ("Light sweep", false, false, false),
                ("Parallax", true, true, false),
                ("Slides", true, true, false),
                ("Crossfades", true, true, false),
            ]
        );
        assert_eq!(view.offset, 0.0, "in place once open");
        assert_eq!(view.privacy[0].value, "Reading…");
    }

    #[test]
    fn focus_stops_at_the_ends() {
        let mut control = Control::default();
        control.open();
        assert!(!control.step(-1));
        for _ in 0..4 {
            assert!(control.step(1));
        }
        assert_eq!(control.focused(), Motion::Fade);
        assert!(!control.step(1));
    }
}
