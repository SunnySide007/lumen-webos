//! The home loop's part in the Top Shelf hero (docs/specs/look-and-feel.md,
//! "Phase 4 features"): when to load the hero for where focus rests, when to
//! upload it and start its crossfade, and when to free what the shelf no
//! longer shows. The loop decides; the screen loads, uploads and frees
//! (decision 8).
//!
//! * A hero loads only once focus has rested: at the start, and
//!   `style::HERO_REST` after a burst that input began has settled. A key
//!   held along row 0 loads nothing for the tiles it passes.
//! * Its pixels go into textures only on a pass that began with an idle wait
//!   and stayed settled, with the shell in front: never inside a burst. Its
//!   crossfade starts there.
//! * The textures, the atlas and every hero, stay within `TEXTURE_BUDGET`: a
//!   hero that would take them over it is not uploaded. Only a crossfade
//!   holds two heroes, and the one it leaves goes at the next idle wait.

use std::time::Duration;

use lumen_core::art::Hero;
use lumen_core::home::Home;
use lumen_core::shelf::Faded;
use lumen_core::style;

use crate::atlas::{ATLAS_H, ATLAS_W};

use super::{HeroNews, Screen};

/// The GPU texture memory the shell may hold (look-and-feel.md, target 9):
/// 24 MB, in millions of bytes, the stricter reading.
pub(super) const TEXTURE_BUDGET: usize = 24_000_000;
/// The atlas, there from the start.
pub(super) const ATLAS_BYTES: usize = ATLAS_W as usize * ATLAS_H as usize * 4;
/// How often a settled screen looks for what the art loader is doing.
pub(super) const HERO_POLL: Duration = Duration::from_millis(16);

/// A hero decoded and not uploaded.
#[derive(Debug)]
struct Decoded {
    hero: Hero,
    bytes: usize,
    took: Duration,
}

/// A crossfade under way: its frames so far, and the most its textures held.
#[derive(Debug)]
struct Fading {
    frames: u32,
    peak: usize,
    /// The frame that showed it settled has been counted.
    done: bool,
}

#[derive(Debug)]
pub(super) struct HeroPlan {
    /// The look has heroes.
    on: bool,
    /// The owner's art is listed, so the heroes' choice is final.
    listed: bool,
    /// Asked of the loader and not back.
    loading: Option<Hero>,
    decoded: Vec<Decoded>,
    /// In textures, with what they count against the budget.
    uploaded: Vec<(Hero, usize)>,
    /// Would not load or upload: never asked for again.
    failed: Vec<Hero>,
    /// When, on the loop's clock, focus has rested long enough for the
    /// wanted hero to load.
    due: Duration,
    /// The last input the plan knows of, to tell a burst input began.
    input_seen: Duration,
    fading: Option<Fading>,
    /// The most the textures have held.
    peak: usize,
    /// The texture total has been logged.
    total_said: bool,
}

/// A hero's name, or the placeholder's.
fn name(hero: Option<&Hero>) -> String {
    hero.map_or_else(|| "gradient".to_owned(), Hero::name)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl HeroPlan {
    /// The plan at the loop's start, `on` if the look has heroes. Focus
    /// rests on the first tile already, so its hero may load at once;
    /// `last_input` is the input time the loop starts from.
    pub(super) fn new(on: bool, last_input: Duration) -> Self {
        Self {
            on,
            listed: false,
            loading: None,
            decoded: Vec::new(),
            uploaded: Vec::new(),
            failed: Vec::new(),
            due: Duration::ZERO,
            input_seen: last_input,
            fading: None,
            peak: ATLAS_BYTES,
            total_said: false,
        }
    }

    /// The atlas and every hero in textures.
    fn total(&self) -> usize {
        ATLAS_BYTES + self.uploaded.iter().map(|(_, b)| b).sum::<usize>()
    }

    fn is_uploaded(&self, hero: &Hero) -> bool {
        self.uploaded.iter().any(|(h, _)| h == hero)
    }

    fn is_decoded(&self, hero: &Hero) -> bool {
        self.decoded.iter().any(|d| &d.hero == hero)
    }

    /// Takes in what the loader finished, on any pass: nothing it says
    /// touches GL. Returns the lines to log.
    pub(super) fn take_news(&mut self, screen: &mut impl Screen, home: &mut Home) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.on {
            return lines;
        }
        for news in screen.hero_news() {
            match news {
                HeroNews::OwnerArt { apps, refused } => {
                    self.listed = true;
                    lines.extend(refused.iter().map(|r| format!("art-owner/{r}; not used")));
                    if !apps.is_empty() {
                        let mut ids: Vec<&str> = apps.iter().map(|id| id.as_str()).collect();
                        ids.sort_unstable();
                        lines.push(format!(
                            "art-owner: art for {} apps: {}",
                            ids.len(),
                            ids.join(", ")
                        ));
                    }
                    home.set_owner_art(apps);
                }
                HeroNews::Decoded {
                    hero, bytes, took, ..
                } => {
                    self.loading.take_if(|h| *h == hero);
                    self.decoded.push(Decoded { hero, bytes, took });
                }
                HeroNews::Failed { hero, why } => {
                    self.loading.take_if(|h| *h == hero);
                    lines.push(format!(
                        "hero {}: {why}; the shelf shows its gradient",
                        hero.name()
                    ));
                    self.failed.push(hero);
                }
            }
        }
        lines
    }

    /// A burst is over. If input came in it, the wanted hero waits for focus
    /// to rest `style::HERO_REST` from now.
    pub(super) fn burst_over(&mut self, now: Duration, last_input: Duration) {
        if last_input != self.input_seen {
            self.input_seen = last_input;
            self.due = now + style::HERO_REST;
        }
    }

    /// The one place a hero is uploaded, freed or asked for: a pass that
    /// began with an idle wait, still settled after its events, with the
    /// shell in front. Returns the lines to log.
    pub(super) fn idle(
        &mut self,
        home: &mut Home,
        screen: &mut impl Screen,
        now: Duration,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.on {
            return lines;
        }
        while let Some(Faded { from, to }) = home.take_faded() {
            if let Some(fading) = self.fading.take() {
                lines.push(format!(
                    "crossfade {} to {}: {} frames; textures {} KB at most",
                    name(from.as_ref()),
                    name(to.as_ref()),
                    fading.frames,
                    fading.peak / 1024
                ));
            }
        }
        let wanted = home.wanted_hero().cloned();
        let target = home.shelf_target().cloned();
        // What the shelf no longer shows and focus no longer wants goes:
        // textures, then pixels never uploaded.
        let unused = |hero: &Hero| target.as_ref() != Some(hero) && wanted.as_ref() != Some(hero);
        for (hero, _) in self.uploaded.extract_if(.., |(hero, _)| unused(hero)) {
            screen.free_hero(&hero);
        }
        for d in self
            .decoded
            .extract_if(.., |d| unused(&d.hero) || target.as_ref() == Some(&d.hero))
        {
            screen.free_hero(&d.hero);
            lines.push(format!(
                "hero {}: decoded in {:.1} ms; dropped, focus moved on",
                d.hero.name(),
                ms(d.took)
            ));
        }
        let Some(wanted) = wanted.filter(|w| target.as_ref() != Some(w)) else {
            self.say_total(home, &mut lines);
            return lines;
        };
        if self.failed.contains(&wanted) {
            home.show_hero(None);
        } else if self.is_uploaded(&wanted) {
            self.fade_to(home, wanted);
        } else if let Some(at) = self.decoded.iter().position(|d| d.hero == wanted) {
            let d = self.decoded.remove(at);
            let total = self.total() + d.bytes;
            if total > TEXTURE_BUDGET {
                screen.free_hero(&d.hero);
                lines.push(format!(
                    "hero {}: {} KB more would take the textures to {} KB, over {} KB; not uploaded",
                    d.hero.name(),
                    d.bytes / 1024,
                    total / 1024,
                    TEXTURE_BUDGET / 1024
                ));
                self.failed.push(d.hero);
                home.show_hero(None);
            } else {
                match screen.upload_hero(&d.hero) {
                    Ok(up) => {
                        lines.push(format!(
                            "hero {}: {} {}, {} KB; decoded in {:.1} ms, uploaded in {:.1} ms",
                            d.hero.name(),
                            up.layers,
                            if up.layers == 1 { "layer" } else { "layers" },
                            up.bytes / 1024,
                            ms(d.took),
                            ms(up.took)
                        ));
                        self.uploaded.push((d.hero.clone(), up.bytes));
                        self.fade_to(home, d.hero);
                    }
                    Err(e) => {
                        lines.push(format!(
                            "hero {}: upload failed: {e}; the shelf shows its gradient",
                            d.hero.name()
                        ));
                        self.failed.push(d.hero);
                        home.show_hero(None);
                    }
                }
            }
        } else if self.listed && self.loading.is_none() && now >= self.due {
            screen.load_hero(&wanted);
            self.loading = Some(wanted);
        }
        self.say_total(home, &mut lines);
        lines
    }

    /// Starts the crossfade to `hero`, whose textures are in.
    fn fade_to(&mut self, home: &mut Home, hero: Hero) {
        let total = self.total();
        self.peak = self.peak.max(total);
        self.fading = Some(Fading {
            frames: 0,
            peak: total,
            done: false,
        });
        home.show_hero(Some(hero));
    }

    /// The texture total, once, at the first idle wait with a hero on the
    /// shelf and nothing fading.
    fn say_total(&mut self, home: &Home, lines: &mut Vec<String>) {
        if self.total_said || self.fading.is_some() || home.shelf_target().is_none() {
            return;
        }
        self.total_said = true;
        let total = self.total();
        lines.push(format!(
            "textures: {} KB in all (atlas {} KB, heroes {} KB); {} KB at most so far",
            total / 1024,
            ATLAS_BYTES / 1024,
            (total - ATLAS_BYTES) / 1024,
            self.peak / 1024
        ));
    }

    /// A frame of `home` was shown: a crossfade counts its frames up to the
    /// one that shows it settled.
    pub(super) fn frame_drawn(&mut self, home: &Home) {
        if let Some(fading) = self.fading.as_mut().filter(|f| !f.done) {
            fading.frames += 1;
            fading.done = !home.is_fading();
        }
    }

    /// When a settled screen must look again. At once, if `can_act` (no key
    /// held, so `idle` will run), when a crossfade has settled (its line, and
    /// the textures it left to free) or the wanted hero waits decoded, in
    /// textures or due to load; soon while the loader works; or once focus
    /// has rested long enough for the wanted hero to load. Nothing else need
    /// wake the loop: under a fixed clock nothing else would.
    pub(super) fn deadline(&self, home: &Home, now: Duration, can_act: bool) -> Option<Duration> {
        if !self.on {
            return None;
        }
        let target = home.shelf_target();
        let unused = |hero: &Hero| target != Some(hero) && home.wanted_hero() != Some(hero);
        let settled = self.fading.is_some() && !home.is_fading();
        if can_act && (settled || self.uploaded.iter().any(|(hero, _)| unused(hero))) {
            return Some(now);
        }
        if !self.listed || self.loading.is_some() {
            return Some(now + HERO_POLL);
        }
        let wanted = home.wanted_hero()?;
        if target == Some(wanted) {
            return None;
        }
        if self.failed.contains(wanted) {
            return (can_act && target.is_some()).then_some(now);
        }
        if self.is_decoded(wanted) || self.is_uploaded(wanted) || self.due <= now {
            return can_act.then_some(now);
        }
        Some(self.due)
    }

    /// Whether the shelf will change by itself: the settled screenshot
    /// waits for it.
    pub(super) fn pending(&self, home: &Home) -> bool {
        if !self.on {
            return false;
        }
        if !self.listed || self.loading.is_some() || home.is_fading() {
            return true;
        }
        let target = home.shelf_target();
        home.wanted_hero().is_some_and(|wanted| {
            if self.failed.contains(wanted) {
                target.is_some()
            } else {
                target != Some(wanted)
            }
        })
    }
}
