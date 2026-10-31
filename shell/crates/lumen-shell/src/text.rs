//! Text in Inter SemiBold, rasterised by fontdue into the atlas one glyph at a
//! time and drawn as image quads. The font is built into the binary, so the
//! simulator and the TV draw exactly the same pixels.
//!
//! fontdue kerns from a font's `kern` table only, and Inter 4.1 keeps its
//! kerning in GPOS, so `tools/font/kern_table.py` pulls the pairs out ahead
//! of time into a committed table (`make kern-table`), built in beside the
//! font. Text is kerned from that table: widths, cuts with an ellipsis and
//! drawing alike.

use std::borrow::Cow;
use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

use crate::atlas::{Atlas, Uv};
use crate::render::{Batch, Rgba};

/// Inter 4.1, SemiBold (SIL Open Font License 1.1; shell/assets/fonts).
static INTER_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
/// Its kerning, from its GPOS table (`make kern-table`).
static INTER_SEMIBOLD_KERN: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.kern");

const ELLIPSIS: char = '\u{2026}';

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

/// Kerning by pairs of characters, from the table `tools/font/kern_table.py`
/// writes (its docstring has the layout). Characters that kern alike share a
/// class, so a pair's value is one cell of a small class matrix.
struct Kerning {
    units_per_em: f32,
    /// Code points, ascending.
    chars: Vec<u16>,
    /// Each character's left and right class.
    left: Vec<u8>,
    right: Vec<u8>,
    /// Font units, row by left class.
    values: Vec<i16>,
    right_classes: usize,
}

impl Kerning {
    const MAGIC: &'static [u8] = b"LKRN";
    const VERSION: u16 = 1;
    const HEADER: usize = 14;

    fn parse(table: &[u8]) -> Result<Self, String> {
        let u16_at = |at: usize| {
            table
                .get(at..at + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or("kerning table: cut short")
        };
        if table.get(..4) != Some(Self::MAGIC) {
            return Err("kerning table: not a kerning table".to_owned());
        }
        if u16_at(4)? != Self::VERSION {
            return Err("kerning table: not version 1 of the format".to_owned());
        }
        let units_per_em = u16_at(6)?;
        let n = usize::from(u16_at(8)?);
        let (l, r) = (usize::from(u16_at(10)?), usize::from(u16_at(12)?));
        let chars_at = Self::HEADER;
        let classes_at = chars_at + 2 * n;
        let values_at = classes_at + 2 * n;
        // Checked: on the TV's 32-bit target, 2 x 65,535 x 65,535 overflows.
        let size = l
            .checked_mul(r)
            .and_then(|cells| cells.checked_mul(2))
            .and_then(|bytes| bytes.checked_add(values_at));
        if units_per_em == 0 || size != Some(table.len()) {
            return Err(format!(
                "kerning table: {} bytes, not as its header says",
                table.len()
            ));
        }
        let chars: Vec<u16> = (0..n)
            .map(|i| u16_at(chars_at + 2 * i))
            .collect::<Result<_, _>>()?;
        let left = table[classes_at..classes_at + n].to_vec();
        let right = table[classes_at + n..values_at].to_vec();
        let (pairs, _) = table[values_at..].as_chunks::<2>();
        let values = pairs.iter().map(|&b| i16::from_le_bytes(b)).collect();
        if !chars.is_sorted_by(|a, b| a < b)
            || left.iter().any(|&k| usize::from(k) >= l)
            || right.iter().any(|&k| usize::from(k) >= r)
        {
            return Err(
                "kerning table: characters out of order or a class past the matrix".to_owned(),
            );
        }
        Ok(Self {
            units_per_em: f32::from(units_per_em),
            chars,
            left,
            right,
            values,
            right_classes: r,
        })
    }

    /// How much closer (negative) or further apart `right` goes after `left`,
    /// in font units.
    fn units(&self, left: char, right: char) -> i16 {
        let index = |c: char| {
            let c = u16::try_from(u32::from(c)).ok()?;
            self.chars.binary_search(&c).ok()
        };
        let (Some(a), Some(b)) = (index(left), index(right)) else {
            return 0;
        };
        let cell = usize::from(self.left[a]) * self.right_classes + usize::from(self.right[b]);
        self.values[cell]
    }
}

pub struct Text {
    font: Font,
    kerning: Kerning,
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
            kerning: Kerning::parse(INTER_SEMIBOLD_KERN)?,
            glyphs: HashMap::new(),
            rasterised: 0,
        })
    }

    /// The kerning between `left` and `right` at `px`, in pixels: negative
    /// pulls `right` closer.
    pub fn kern(&self, left: char, right: char, px: f32) -> f32 {
        f32::from(self.kerning.units(left, right)) * whole(px) / self.kerning.units_per_em
    }

    /// How far the pen moves for `c` after `prev`, at `px`: the kerning
    /// between them and `c`'s advance.
    fn advance(&self, prev: Option<char>, c: char, px: f32) -> f32 {
        let kern = prev.map_or(0.0, |p| self.kern(p, c, px));
        kern + self.font.metrics(c, whole(px)).advance_width
    }

    /// How many glyphs were rasterised since the last call, for the
    /// slow-frame lines.
    pub fn take_rasterised(&mut self) -> u32 {
        std::mem::take(&mut self.rasterised)
    }

    /// How far `text` advances at `px` pixels to the em, kerned.
    pub fn width(&self, text: &str, px: f32) -> f32 {
        let mut prev = None;
        let mut w = 0.0;
        for c in text.chars() {
            w += self.advance(prev, c, px);
            prev = Some(c);
        }
        w
    }

    /// The height of a capital letter at `px`, for centring a line on a box:
    /// its baseline goes `cap_height / 2` below the box's middle.
    pub fn cap_height(&self, px: f32) -> f32 {
        self.font.metrics('H', whole(px)).bounds.height
    }

    /// `text` cut to fit `max_w` at `px`, ending in an ellipsis when cut. If
    /// not even the ellipsis fits, nothing does. The cut is measured as it is
    /// drawn, kerned against the ellipsis too.
    pub fn fit<'a>(&self, text: &'a str, px: f32, max_w: f32) -> Cow<'a, str> {
        if self.width(text, px) <= max_w {
            return Cow::Borrowed(text);
        }
        let ellipsis = self.advance(None, ELLIPSIS, px);
        if ellipsis > max_w {
            return Cow::Borrowed("");
        }
        let mut used = 0.0;
        let mut prev = None;
        let mut end = 0;
        // Each character adds far more advance than any kerning takes away,
        // so the first cut that doesn't fit ends the search.
        for (i, c) in text.char_indices() {
            used += self.advance(prev, c, px);
            prev = Some(c);
            // A cut never ends in a space, so only a cut after a visible
            // character is measured.
            if c.is_whitespace() {
                continue;
            }
            // Added as `width` adds them, so the cut measures the same.
            if used + (self.kern(c, ELLIPSIS, px) + ellipsis) > max_w {
                break;
            }
            end = i + c.len_utf8();
        }
        Cow::Owned(format!("{}{ELLIPSIS}", &text[..end]))
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
        let mut prev = None;
        for c in text.chars() {
            if let Some(p) = prev {
                pen += self.kern(p, c, px);
            }
            prev = Some(c);
            let Some(glyph) = self.glyph(atlas, c, size) else {
                // Not drawn, but still advanced past, as `width` measures it.
                pen += self.font.metrics(c, whole(px)).advance_width;
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
    fn the_kerning_table_loads() {
        let k = Kerning::parse(INTER_SEMIBOLD_KERN).expect("the built-in table parses");
        assert_eq!(k.units_per_em, 2048.0);
        // Inter 4.1 SemiBold's GPOS, read by tools/font/kern_table.py.
        assert_eq!(k.units('A', 'V'), -154);
        assert_eq!(k.units('Y', 'o'), -202);
        assert_eq!(k.units('H', 'H'), 0, "straight sides don't kern");
        assert_eq!(k.units('A', '\u{4E2D}'), 0, "a character not in the table");
    }

    #[test]
    fn a_damaged_kerning_table_is_refused() {
        let good = INTER_SEMIBOLD_KERN;
        assert!(Kerning::parse(&good[..good.len() - 1]).is_err(), "short");
        assert!(Kerning::parse(&[good, &[0]].concat()).is_err(), "long");
        let mut magic = good.to_vec();
        magic[0] = b'X';
        assert!(Kerning::parse(&magic).is_err(), "magic");
        let mut class = good.to_vec();
        let n = usize::from(u16::from_le_bytes([good[8], good[9]]));
        class[14 + 2 * n] = 255;
        assert!(Kerning::parse(&class).is_err(), "a class past the matrix");
        // 65,535 x 65,535 classes: a size that overflows 32 bits is refused,
        // not wrapped.
        let mut huge = good[..14].to_vec();
        huge[8..14].copy_from_slice(&[0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(Kerning::parse(&huge).is_err(), "too many classes");
    }

    #[test]
    fn known_pairs_come_out_tighter_than_unkerned() {
        let t = text();
        for pair in ["AV", "To", "Ty", "Yo", "LT", "r."] {
            let mut chars = pair.chars();
            let (a, b) = (chars.next().unwrap(), chars.next().unwrap());
            let unkerned = t.width(&a.to_string(), 72.0) + t.width(&b.to_string(), 72.0);
            let kerned = t.width(pair, 72.0);
            assert!(kerned < unkerned - 2.0, "{pair}: {kerned} vs {unkerned}");
            assert!(
                (kerned - (unkerned + t.kern(a, b, 72.0))).abs() < 1e-3,
                "{pair}: the width is the advances plus the kerning"
            );
        }
        assert_eq!(t.kern('A', 'V', 72.0), -154.0 * 72.0 / 2048.0);
        assert_eq!(t.kern('H', 'H', 72.0), 0.0);
    }

    #[test]
    fn drawn_glyphs_are_kerned() {
        let mut t = text();
        let (mut atlas, white) = (Atlas::default(), Rgba(1.0, 1.0, 1.0, 1.0));
        let mut pair = Batch::default();
        t.draw(&mut pair, &mut atlas, 0.0, 500.0, 72.0, 1.0, "AV", white);
        // The V alone, where it would go unkerned.
        let mut v = Batch::default();
        let after_a = t.width("A", 72.0);
        t.draw(&mut v, &mut atlas, after_a, 500.0, 72.0, 1.0, "V", white);
        let (_, _, right, _) = pair.bounds();
        let (_, _, v_right, _) = v.bounds();
        let kern = t.kern('A', 'V', 72.0);
        assert!(
            (right - (v_right + kern)).abs() <= 1.0,
            "the V moves left by the kerning: {right} vs {v_right} {kern}"
        );
    }

    #[test]
    fn fitting_a_title_counts_the_kerning() {
        let t = text();
        let text = "AVAVAVAVAV Yoga To Type";
        // Exactly the kerned width of a cut: unkerned, one letter fewer fits.
        let cut = format!("AVAVAV{ELLIPSIS}");
        let kerned = t.width(&cut, 30.0);
        let unkerned: f32 = cut.chars().map(|c| t.width(&c.to_string(), 30.0)).sum();
        assert!(unkerned > kerned + 1.0, "{unkerned} vs {kerned}");
        assert_eq!(t.fit(text, 30.0, kerned), cut);
        // Every cut fits, and keeps as much as fits.
        for max_w in (20..=400).step_by(3).map(|w| w as f32) {
            let fitted = t.fit(text, 30.0, max_w);
            assert!(t.width(&fitted, 30.0) <= max_w, "{max_w}: {fitted}");
            let Some(kept) = fitted.strip_suffix(ELLIPSIS) else {
                continue;
            };
            let next = text[kept.len()..]
                .char_indices()
                .find(|&(_, c)| !c.is_whitespace())
                .map(|(i, c)| kept.len() + i + c.len_utf8());
            if let Some(end) = next {
                let longer = format!("{}{ELLIPSIS}", &text[..end]);
                assert!(t.width(&longer, 30.0) > max_w, "{max_w}: {longer} fits too");
            }
        }
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
        let just = t.width(&ELLIPSIS.to_string(), 30.0);
        assert_eq!(t.fit("Anything", 30.0, just), ELLIPSIS.to_string());
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
