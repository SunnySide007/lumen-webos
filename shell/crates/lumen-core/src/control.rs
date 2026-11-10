//! The Control Center (docs/specs/look-and-feel.md, "The Control Center"):
//! a panel the gear on the home opens, over the right of the screen. It
//! shows the Privacy panel, which only reads (decision 12), and Reduce
//! Motion: one switch, editable on the TV (decision 7), that turns off
//! every moving effect at once, driving the same switches as the launch
//! parameters' `"look"`. The effects stay switches of their own there, for
//! the bench; the saved file names each one.
//!
//! Reduce Motion can only take effects away: what the launch look turns
//! off stays off, and with nothing moving in it the switch does nothing.

use std::fmt;

use crate::anim::Spring;
use crate::art::Look;
use crate::layout;
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

/// The one switch's row.
pub const REDUCE_MOTION: &str = "Reduce Motion";
/// What it says it stops, under it.
pub const REDUCE_MOTION_NOTE: &str = "Stops tilt, light sweep, parallax, slides and fades";

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

    /// Every effect off.
    pub const ALL: Self = Self([true; 5]);

    /// Whether the switch is on over `launch`: every effect `launch` runs
    /// is off, and it runs one.
    pub fn is_on(&self, launch: Look) -> bool {
        let moving: Vec<Motion> = Motion::ALL.into_iter().filter(|m| m.of(launch)).collect();
        !moving.is_empty() && moving.iter().all(|m| self.is_off(*m))
    }

    /// The switch pressed over `launch`: on, every effect off; off, every
    /// effect back. `None` if `launch` runs none, so there is nothing to
    /// take away.
    pub fn toggled(&self, launch: Look) -> Option<Self> {
        if !Motion::ALL.iter().any(|m| m.of(launch)) {
            return None;
        }
        Some(if self.is_on(launch) {
            Self::default()
        } else {
            Self::ALL
        })
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

/// A switch's row as drawn: Reduce Motion's.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionRow {
    pub label: &'static str,
    /// Whether the switch is on (`Reduced::is_on`).
    pub on: bool,
    /// Whether OK changes it: not if the launch look runs no effect.
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
    /// The switches' rows: Reduce Motion's alone.
    pub motion: Vec<MotionRow>,
}

/// How many switch rows the panel has.
const ROWS: usize = 1;

/// The Control Center's state: whether it is open, the focused row, the
/// Privacy panel's reading.
#[derive(Debug)]
pub struct Control {
    shown: Spring,
    focus: usize,
    lifts: [Spring; ROWS],
    reading: Reading,
}

impl Default for Control {
    fn default() -> Self {
        Self {
            shown: Spring::new(0.0, style::PAGE),
            focus: 0,
            lifts: [Spring::new(0.0, style::LIFT); ROWS],
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
        self.lifts = [Spring::new(0.0, style::LIFT); ROWS];
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
        let Some(to) = self.focus.checked_add_signed(by).filter(|&i| i < ROWS) else {
            return false;
        };
        self.lifts[self.focus].set_target(0.0);
        self.lifts[to].set_target(1.0);
        self.focus = to;
        true
    }

    /// Focuses row `i`, as the pointer over it does; false if it had focus
    /// already (or there is no such row).
    pub fn focus_row(&mut self, i: usize) -> bool {
        if i >= ROWS || i == self.focus {
            return false;
        }
        self.lifts[self.focus].set_target(0.0);
        self.lifts[i].set_target(1.0);
        self.focus = i;
        true
    }

    /// The switch row under the point `x`,`y`, with the panel where it is
    /// drawn now under `look`.
    pub fn row_under(&self, x: f32, y: f32, look: Look) -> Option<usize> {
        let panel = layout::panel(self.offset(look));
        (0..ROWS).find(|&i| layout::motion_row(panel, i).contains(x, y))
    }

    /// How far right of its place the panel is now, under `look`.
    fn offset(&self, look: Look) -> f32 {
        let shown = self.shown.value().clamp(0.0, 1.0);
        let slide = if look.slide { style::PANEL_SLIDE } else { 0.0 };
        slide * (1.0 - shown)
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
        Some(ControlView {
            shown,
            offset: self.offset(look),
            privacy: privacy::lines(&self.reading),
            motion: vec![MotionRow {
                label: REDUCE_MOTION,
                on: reduced.is_on(launch),
                editable: reduced.toggled(launch).is_some(),
                focused: self.is_open() && self.focus == 0,
                lift: self.lifts[0].value().clamp(0.0, 1.0),
            }],
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
    fn the_switch_turns_every_effect_off_and_all_back() {
        let none = Reduced::default();
        assert!(!none.is_on(Look::FULL));
        let on = none.toggled(Look::FULL).expect("something moves");
        assert_eq!(on, Reduced::ALL);
        assert!(on.is_on(Look::FULL));
        let look = on.apply(Look::FULL);
        assert!(!look.tilt && !look.light && !look.parallax && !look.slide && !look.fade);
        assert!(
            look.hero && look.shadow && look.sheen && look.dim,
            "only motion"
        );
        assert_eq!(on.toggled(Look::FULL), Some(none));
        // Some effects off (an older file's), it reads off, and turns the
        // rest off.
        let some = Reduced::from_words("tilt slide");
        assert!(!some.is_on(Look::FULL));
        assert_eq!(some.toggled(Look::FULL), Some(Reduced::ALL));
        // What the launch look keeps off counts as off already.
        let launch = Look {
            light: false,
            parallax: false,
            ..Look::FULL
        };
        assert!(Reduced::from_words("tilt slide fade").is_on(launch));
        // Nothing moving: nothing to take away.
        assert_eq!(none.toggled(Look::PLAIN), None);
        assert!(!Reduced::ALL.is_on(Look::PLAIN));
    }

    #[test]
    fn the_panel_has_the_one_switch_saying_whether_it_is_on() {
        let mut control = Control::default();
        assert!(
            control
                .view(Look::FULL, Reduced::default(), Look::FULL)
                .is_none()
        );
        control.open();
        control.shown.snap();
        let row = |control: &Control, launch: Look, reduced: Reduced| {
            let view = control
                .view(launch, reduced, reduced.apply(launch))
                .expect("open");
            assert_eq!(view.motion.len(), 1);
            let r = &view.motion[0];
            (r.label, r.on, r.editable, r.focused)
        };
        let off = row(&control, Look::FULL, Reduced::default());
        assert_eq!(off, (REDUCE_MOTION, false, true, true));
        let on = row(&control, Look::FULL, Reduced::ALL);
        assert_eq!(on, (REDUCE_MOTION, true, true, true));
        let still = row(&control, Look::PLAIN, Reduced::default());
        assert_eq!(still, (REDUCE_MOTION, false, false, true));
        let view = control
            .view(Look::FULL, Reduced::default(), Look::FULL)
            .unwrap();
        assert_eq!(view.offset, 0.0, "in place once open");
        assert_eq!(view.privacy[0].value, "Reading…");
        // One row: Up and Down go nowhere.
        assert!(!control.step(-1) && !control.step(1));
    }
}
