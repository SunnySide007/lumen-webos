//! Text in Inter SemiBold, rasterised by fontdue into the atlas one glyph at a
//! time and drawn as image quads. The font is built into the binary, so the
//! simulator and the TV draw exactly the same pixels.
//!
//! fontdue kerns from a font's `kern` table only, and Inter 4.1 keeps its
//! kerning in GPOS, so text is drawn unkerned for now.

use std::borrow::Cow;
use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

use crate::atlas::{Atlas, Uv};
use crate::render::{Batch, Rgba};

/// Inter 4.1, SemiBold (SIL Open Font License 1.1; shell/assets/fonts).
static INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

const ELLIPSIS: &str = "\u{2026}";

/// fontdue prepares outlines for sizes up to this; the largest text drawn is
/// a letter tile's 110 px.
const OUTLINE_SCALE: f32 = 120.0;

/// Text is measured and rasterised at whole-pixel sizes, the same everywhere.
fn whole(px: f32) -> f32 {
    px.round().max(1.0)
}

/// A rasterised glyph and where the atlas keeps it (`None` for a glyph with
/// no pixels, such as a space).
struct Glyph {
    metrics: Metrics,
    uv: Option<Uv>,
    generation: u32,
}

pub struct Text {
    font: Font,
    /// By character and size in whole pixels.
    glyphs: HashMap<(char, u32), Glyph>,
    /// Glyphs rasterised since `take_rasterised` was last called.
    rasterised: u32,
}

impl Text {
    pub fn new() -> Result<Self, String> {
        let settings = FontSettings {
            scale: OUTLINE_SCALE,
            ..FontSettings::default()
        };
        let font = Font::from_bytes(INTER_SEMIBOLD, settings).map_err(|e| format!("Inter: {e}"))?;
        Ok(Self {
            font,
            glyphs: HashMap::new(),
            rasterised: 0,
        })
    }

    /// How many glyphs were rasterised since the last call, for the
    /// slow-frame lines.
    pub fn take_rasterised(&mut self) -> u32 {
        std::mem::take(&mut self.rasterised)
    }

    /// How far `text` advances at `px` pixels to the em.
    pub fn width(&self, text: &str, px: f32) -> f32 {
        let px = whole(px);
        text.chars()
            .map(|c| self.font.metrics(c, px).advance_width)
            .sum()
    }

    /// The height of a capital letter at `px`, for centring a line on a box:
    /// its baseline goes `cap_height / 2` below the box's middle.
    pub fn cap_height(&self, px: f32) -> f32 {
        self.font.metrics('H', whole(px)).bounds.height
    }

    /// `text` cut to fit `max_w` at `px`, ending in an ellipsis when cut. If
    /// not even the ellipsis fits, nothing does.
    pub fn fit<'a>(&self, text: &'a str, px: f32, max_w: f32) -> Cow<'a, str> {
        if self.width(text, px) <= max_w {
            return Cow::Borrowed(text);
        }
        let room = max_w - self.width(ELLIPSIS, px);
        if room < 0.0 {
            return Cow::Borrowed("");
        }
        let mut used = 0.0;
        let mut end = 0;
        for (i, c) in text.char_indices() {
            used += self.font.metrics(c, whole(px)).advance_width;
            if used > room {
                break;
            }
            end = i + c.len_utf8();
        }
        Cow::Owned(format!("{}{ELLIPSIS}", text[..end].trim_end()))
    }

    /// Draws `text` rasterised at `px` and scaled by `scale`, starting at `x`
    /// on the baseline `baseline`. A size that animates should keep `px` fixed
    /// (its largest) and change `scale`, so the atlas holds one copy.
    // Where, how big, what and in which colour.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        batch: &mut Batch,
        atlas: &mut Atlas,
        x: f32,
        baseline: f32,
        px: f32,
        scale: f32,
        text: &str,
        color: Rgba,
    ) {
        let size = whole(px) as u32;
        // A scale within 1% of 1 (a lifted letter tile) is drawn at 1, on whole
        // pixels, one texel a pixel, which keeps glyphs sharp.
        let scale = if (scale - 1.0).abs() < 0.01 {
            1.0
        } else {
            scale
        };
        let snap = |v: f32| if scale == 1.0 { v.round() } else { v };
        let mut pen = 0.0;
        for c in text.chars() {
            let Some(glyph) = self.glyph(atlas, c, size) else {
                continue;
            };
            let m = glyph.metrics;
            if let Some(uv) = glyph.uv {
                let gx = snap(x + (pen + m.xmin as f32) * scale);
                let top = snap(baseline - (m.ymin as f32 + m.height as f32) * scale);
                batch.image(
                    gx,
                    top,
                    m.width as f32 * scale,
                    m.height as f32 * scale,
                    uv,
                    color,
                );
            }
            pen += m.advance_width;
        }
    }

    /// The glyph for `c` at `size`, rasterised and placed in the atlas the
    /// first time, and again after the atlas is emptied. `None` only when it
    /// cannot fit even in an empty atlas.
    fn glyph(&mut self, atlas: &mut Atlas, c: char, size: u32) -> Option<&Glyph> {
        let generation = atlas.generation();
        let fresh = self
            .glyphs
            .get(&(c, size))
            .is_some_and(|g| g.uv.is_none() || g.generation == generation);
        if !fresh {
            let (metrics, coverage) = self.font.rasterize(c, size as f32);
            self.rasterised = self.rasterised.saturating_add(1);
            let uv = if metrics.width == 0 || metrics.height == 0 {
                None
            } else {
                // White, premultiplied: every channel is the coverage.
                let rgba: Vec<u8> = coverage.iter().flat_map(|&a| [a, a, a, a]).collect();
                let (w, h) = (metrics.width as u32, metrics.height as u32);
                match atlas.insert(w, h, &rgba) {
                    Some(uv) => Some(uv),
                    None => {
                        atlas.clear();
                        Some(atlas.insert(w, h, &rgba)?)
                    }
                }
            };
            let glyph = Glyph {
                metrics,
                uv,
                generation: atlas.generation(),
            };
            self.glyphs.insert((c, size), glyph);
        }
        self.glyphs.get(&(c, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> Text {
        Text::new().expect("the built-in font parses")
    }

    #[test]
    fn width_grows_with_the_text_and_the_size() {
        let t = text();
        assert_eq!(t.width("", 30.0), 0.0);
        let one = t.width("Plex", 30.0);
        assert!(one > 40.0 && one < 90.0, "{one}");
        assert!(t.width("Plex TV", 30.0) > one);
        assert!((t.width("Plex", 60.0) - 2.0 * one).abs() < 0.5);
    }

    #[test]
    fn the_cap_height_is_about_three_quarters_of_the_size() {
        let cap = text().cap_height(100.0);
        assert!((65.0..80.0).contains(&cap), "{cap}");
    }

    #[test]
    fn long_text_is_cut_to_fit_with_an_ellipsis() {
        let t = text();
        assert_eq!(t.fit("Netflix", 30.0, 368.0), "Netflix");
        let cut = t.fit("An Extremely Long Application Name", 30.0, 368.0);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(t.width(&cut, 30.0) <= 368.0);
        assert!(cut.chars().count() > 12, "kept as much as fits: {cut}");
        assert!(
            !cut.contains(&format!(" {ELLIPSIS}")),
            "no space before the ellipsis"
        );
        assert_eq!(t.fit("Anything", 30.0, 1.0), "");
        let just = t.width(ELLIPSIS, 30.0);
        assert_eq!(t.fit("Anything", 30.0, just), ELLIPSIS);
    }

    #[test]
    fn each_glyph_is_rasterised_and_placed_once() {
        let mut t = text();
        let (mut batch, mut atlas) = (Batch::default(), Atlas::default());
        let white = Rgba(1.0, 1.0, 1.0, 1.0);
        t.draw(&mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "aa b", white);
        // "a" and "b" have pixels; the space has none and draws nothing.
        assert_eq!(batch.quad_count(), 3);
        assert_eq!(atlas.take_uploads().len(), 2);
        t.draw(&mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "ba", white);
        assert!(atlas.take_uploads().is_empty(), "both glyphs were cached");
        t.draw(&mut batch, &mut atlas, 0.0, 100.0, 31.0, 1.0, "a", white);
        assert_eq!(atlas.take_uploads().len(), 1, "a new size is a new glyph");
    }

    #[test]
    fn rasterised_glyphs_are_counted_until_taken() {
        let mut t = text();
        let (mut batch, mut atlas) = (Batch::default(), Atlas::default());
        let white = Rgba(1.0, 1.0, 1.0, 1.0);
        assert_eq!(t.take_rasterised(), 0);
        t.draw(&mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "aa b", white);
        // "a", the space and "b": a space is rasterised too, to learn it is empty.
        assert_eq!(t.take_rasterised(), 3);
        assert_eq!(t.take_rasterised(), 0, "taking resets the count");
        t.draw(&mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "ba", white);
        assert_eq!(
            t.take_rasterised(),
            0,
            "cached glyphs are not rasterised again"
        );
    }

    #[test]
    fn glyphs_are_placed_again_after_the_atlas_is_emptied() {
        let mut t = text();
        let (mut batch, mut atlas) = (Batch::default(), Atlas::default());
        let white = Rgba(1.0, 1.0, 1.0, 1.0);
        t.draw(
            &mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "Lumen", white,
        );
        atlas.take_uploads();
        atlas.clear();
        t.draw(
            &mut batch, &mut atlas, 0.0, 100.0, 30.0, 1.0, "Lumen", white,
        );
        assert_eq!(atlas.take_uploads().len(), 5, "L, u, m, e and n");
    }

    #[test]
    fn glyphs_sit_on_the_baseline_and_scale_about_the_start() {
        let mut t = text();
        let (mut batch, mut atlas) = (Batch::default(), Atlas::default());
        let white = Rgba(1.0, 1.0, 1.0, 1.0);
        let cap = t.cap_height(100.0);
        t.draw(&mut batch, &mut atlas, 100.0, 500.0, 100.0, 1.0, "H", white);
        let (left, top, _, bottom) = batch.bounds();
        assert!(
            (bottom - 500.0).abs() <= 1.0,
            "an H stands on the baseline: {bottom}"
        );
        assert!((500.0 - top - cap).abs() <= 2.0, "and is a cap tall: {top}");
        assert!(
            (100.0..112.0).contains(&left),
            "just right of the pen: {left}"
        );

        let mut half = Batch::default();
        t.draw(&mut half, &mut atlas, 100.0, 500.0, 100.0, 0.5, "H", white);
        let (h_left, h_top, _, h_bottom) = half.bounds();
        assert!((h_bottom - 500.0).abs() <= 1.0);
        assert!((500.0 - h_top - cap / 2.0).abs() <= 1.5);
        assert!((h_left - 100.0 - (left - 100.0) / 2.0).abs() <= 1.0);
    }
}
