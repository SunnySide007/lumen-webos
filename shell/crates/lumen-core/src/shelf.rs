//! The Top Shelf's art over time: the hero on screen, the one fading in over
//! it, and how the two are drawn (docs/specs/look-and-feel.md, "Phase 4
//! features"). The shell says when a hero's textures are in (`show`); this
//! fades to it on a spring and says when the old one can go (`take_faded`).

use crate::anim::Spring;
use crate::art::{Hero, Look};
use crate::style;

/// A crossfade that finished: what the shelf showed before and what it shows
/// now. `None` is the placeholder gradient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faded {
    pub from: Option<Hero>,
    pub to: Option<Hero>,
}

#[derive(Debug)]
pub struct Shelf {
    /// On screen once nothing fades; `None` is the placeholder gradient.
    shown: Option<Hero>,
    /// Fading in over it, if anything is.
    incoming: Option<Option<Hero>>,
    /// How far the incoming art has faded in, 0 to 1.
    fade: Spring,
    /// Crossfades finished and not yet taken, oldest first.
    faded: Vec<Faded>,
}

/// How to draw the shelf's art: `base` opaque, then `over` at its alpha. A
/// hero of `None` is the placeholder gradient.
///
/// A layered hero is never drawn over anything: its layers laid at an alpha
/// below 1 would let its own sky show through its ridge. So when one fades
/// in, it is the base, and what it replaces fades out over it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShelfView<'a> {
    pub base: Option<&'a Hero>,
    pub over: Option<(Option<&'a Hero>, f32)>,
    /// How much of the placeholder shows, 0 to 1: its caption fades with it.
    pub placeholder: f32,
}

impl ShelfView<'_> {
    /// The placeholder gradient alone.
    pub const PLACEHOLDER: Self = Self {
        base: None,
        over: None,
        placeholder: 1.0,
    };
}

impl Default for Shelf {
    fn default() -> Self {
        Self {
            shown: None,
            incoming: None,
            fade: Spring::new(0.0, style::HERO_FADE),
            faded: Vec::new(),
        }
    }
}

impl Shelf {
    /// What the shelf shows, or is fading to.
    pub fn target(&self) -> Option<&Hero> {
        match &self.incoming {
            Some(incoming) => incoming.as_ref(),
            None => self.shown.as_ref(),
        }
    }

    /// Fades to `hero` (`None`, the gradient). A crossfade already under
    /// way ends first, at once. Returns whether anything changed: asking
    /// for what the shelf already shows, or fades to, changes nothing.
    pub fn show(&mut self, hero: Option<Hero>) -> bool {
        if self.target() == hero.as_ref() {
            return false;
        }
        self.settle_now();
        if self.shown == hero {
            return false;
        }
        self.incoming = Some(hero);
        self.fade = Spring::new(0.0, style::HERO_FADE);
        self.fade.set_target(1.0);
        true
    }

    pub fn is_animating(&self) -> bool {
        self.incoming.is_some()
    }

    /// Steps the crossfade by `dt` seconds; once it settles, the incoming
    /// art is what the shelf shows.
    pub fn step(&mut self, dt: f32) {
        if self.incoming.is_none() {
            return;
        }
        self.fade.step(dt);
        if self.fade.is_settled() {
            self.finish();
        }
    }

    /// Ends a crossfade under way on the incoming art.
    pub fn settle_now(&mut self) {
        if self.incoming.is_some() {
            self.finish();
        }
    }

    fn finish(&mut self) {
        let Some(to) = self.incoming.take() else {
            return;
        };
        let from = std::mem::replace(&mut self.shown, to.clone());
        self.faded.push(Faded { from, to });
        self.fade = Spring::new(0.0, style::HERO_FADE);
    }

    /// The oldest crossfade finished since the last call.
    pub fn take_faded(&mut self) -> Option<Faded> {
        (!self.faded.is_empty()).then(|| self.faded.remove(0))
    }

    pub fn view(&self, look: &Look) -> ShelfView<'_> {
        let shown = self.shown.as_ref();
        let Some(incoming) = &self.incoming else {
            return ShelfView {
                base: shown,
                over: None,
                placeholder: if shown.is_none() { 1.0 } else { 0.0 },
            };
        };
        let incoming = incoming.as_ref();
        let f = self.fade.value().clamp(0.0, 1.0);
        let placeholder = match (shown, incoming) {
            (None, None) => 1.0,
            (None, Some(_)) => 1.0 - f,
            (Some(_), None) => f,
            (Some(_), Some(_)) => 0.0,
        };
        if incoming.is_some_and(|hero| hero.is_layered(look)) {
            ShelfView {
                base: incoming,
                over: Some((shown, 1.0 - f)),
                placeholder,
            }
        } else {
            ShelfView {
                base: shown,
                over: Some((incoming, f)),
                placeholder,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;
    const PLEX: Hero = Hero::Named("hero-plex");

    /// Steps at 60 fps until the crossfade ends; returns the frames it took.
    fn settle(shelf: &mut Shelf) -> usize {
        let mut frames = 0;
        while shelf.is_animating() {
            shelf.step(FRAME);
            frames += 1;
            assert!(frames < 300, "never settled");
        }
        frames
    }

    #[test]
    fn a_new_shelf_shows_the_placeholder() {
        let shelf = Shelf::default();
        assert_eq!(shelf.target(), None);
        assert!(!shelf.is_animating());
        let view = shelf.view(&Look::FULL);
        assert_eq!((view.base, view.over, view.placeholder), (None, None, 1.0));
    }

    #[test]
    fn a_flat_hero_fades_in_over_the_placeholder_and_settles() {
        let mut shelf = Shelf::default();
        assert!(shelf.show(Some(PLEX)));
        assert_eq!(shelf.target(), Some(&PLEX));
        let start = shelf.view(&Look::FULL);
        assert_eq!(start.base, None);
        assert_eq!(start.over, Some((Some(&PLEX), 0.0)));
        shelf.step(0.1);
        let mid = shelf.view(&Look::FULL);
        let (_, alpha) = mid.over.unwrap();
        assert!(alpha > 0.1 && alpha < 0.9, "{alpha}");
        assert!((mid.placeholder - (1.0 - alpha)).abs() < 1e-6);
        let frames = settle(&mut shelf);
        // About 0.7 s at 60 fps.
        assert!((30..50).contains(&frames), "{frames} frames");
        let done = shelf.view(&Look::FULL);
        assert_eq!(
            (done.base, done.over, done.placeholder),
            (Some(&PLEX), None, 0.0)
        );
        assert_eq!(
            shelf.take_faded(),
            Some(Faded {
                from: None,
                to: Some(PLEX)
            })
        );
        assert_eq!(shelf.take_faded(), None);
    }

    #[test]
    fn a_layered_hero_is_the_base_and_what_it_replaces_fades_out_over_it() {
        let mut shelf = Shelf::default();
        shelf.show(Some(PLEX));
        settle(&mut shelf);
        shelf.show(Some(Hero::Ambient));
        shelf.step(0.1);
        let view = shelf.view(&Look::FULL);
        assert_eq!(view.base, Some(&Hero::Ambient));
        let (over, alpha) = view.over.unwrap();
        assert_eq!(over, Some(&PLEX));
        assert!(alpha > 0.1 && alpha < 0.9, "{alpha}");
        // Flat without parallax: the plain crossfade.
        let flat = Look {
            parallax: false,
            ..Look::FULL
        };
        let view = shelf.view(&flat);
        assert_eq!(view.base, Some(&PLEX));
        assert_eq!(view.over.unwrap().0, Some(&Hero::Ambient));
        settle(&mut shelf);
        assert_eq!(
            shelf.take_faded(),
            Some(Faded {
                from: None,
                to: Some(PLEX)
            })
        );
        assert_eq!(
            shelf.take_faded(),
            Some(Faded {
                from: Some(PLEX),
                to: Some(Hero::Ambient)
            })
        );
    }

    #[test]
    fn the_same_hero_never_fades() {
        let mut shelf = Shelf::default();
        shelf.show(Some(PLEX));
        assert!(!shelf.show(Some(PLEX)), "already fading to it");
        settle(&mut shelf);
        shelf.take_faded();
        assert!(!shelf.show(Some(PLEX)), "already shown");
        assert!(!shelf.is_animating());
        assert_eq!(shelf.take_faded(), None);
    }

    #[test]
    fn a_hero_that_fails_fades_back_to_the_placeholder() {
        let mut shelf = Shelf::default();
        shelf.show(Some(PLEX));
        settle(&mut shelf);
        assert!(shelf.show(None));
        shelf.step(0.1);
        let view = shelf.view(&Look::FULL);
        let (over, alpha) = view.over.unwrap();
        assert_eq!((view.base, over), (Some(&PLEX), None));
        assert!((view.placeholder - alpha).abs() < 1e-6);
    }

    #[test]
    fn a_new_hero_mid_crossfade_ends_the_old_one_first() {
        let mut shelf = Shelf::default();
        shelf.show(Some(PLEX));
        shelf.step(0.05);
        assert!(shelf.show(Some(Hero::Palette(2))));
        assert_eq!(
            shelf.take_faded(),
            Some(Faded {
                from: None,
                to: Some(PLEX)
            })
        );
        let view = shelf.view(&Look::FULL);
        assert_eq!(view.base, Some(&PLEX));
        assert_eq!(view.over, Some((Some(&Hero::Palette(2)), 0.0)));
    }

    #[test]
    fn settling_now_ends_the_crossfade_on_the_incoming_art() {
        let mut shelf = Shelf::default();
        shelf.show(Some(PLEX));
        shelf.step(0.05);
        shelf.settle_now();
        assert!(!shelf.is_animating());
        assert_eq!(shelf.target(), Some(&PLEX));
        assert!(shelf.take_faded().is_some());
    }
}
