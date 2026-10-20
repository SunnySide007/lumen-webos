//! Draws the home screen from `lumen_core::home::View` into a `Batch`. B1's
//! look: solid tiles with their titles in the 5x7 font; icons and real text
//! arrive in milestone B2.

use std::borrow::Cow;

use lumen_core::home::{TileView, View};
use lumen_core::layout::{self, SAFE, SCREEN_H, SCREEN_W};

use crate::font;
use crate::render::{Batch, Rgba};

const BG_TOP: Rgba = Rgba::rgb8(14, 18, 34);
const BG_BOTTOM: Rgba = Rgba::rgb8(0, 0, 0);
const SHELF_TOP: Rgba = Rgba::rgb8(46, 56, 98);
const SHELF_BOTTOM: Rgba = Rgba::rgb8(14, 18, 34);
const TEXT: Rgba = Rgba::rgb8(235, 238, 245);
const TEXT_DIM: Rgba = Rgba::rgb8(140, 148, 165);
const SHADOW: Rgba = Rgba(0.0, 0.0, 0.0, 1.0);
const PILL: Rgba = Rgba(0.0, 0.0, 0.0, 0.75);
const WHITE: Rgba = Rgba(1.0, 1.0, 1.0, 1.0);

/// Resting tiles are this bright; lifting brings a tile to full brightness.
const REST_SHADE: f32 = 0.8;
/// How much lighter a tile's top edge is than its bottom.
const TILE_SHEEN: f32 = 0.14;

/// Placeholder tile colours until icons arrive, by position in the launcher's
/// list: with rows of 5 and 8 colours, no tile shares a colour with the tile
/// beside, above or below it.
const PALETTE: [Rgba; 8] = [
    Rgba::rgb8(196, 58, 58),
    Rgba::rgb8(214, 120, 46),
    Rgba::rgb8(190, 152, 44),
    Rgba::rgb8(64, 156, 92),
    Rgba::rgb8(44, 140, 160),
    Rgba::rgb8(58, 104, 196),
    Rgba::rgb8(112, 82, 190),
    Rgba::rgb8(170, 70, 140),
];

pub fn tile_color(index: usize) -> Rgba {
    PALETTE[index % PALETTE.len()]
}

/// `text` cut to fit `max_w` at `scale`, ending in "..." when cut.
pub fn fit(text: &str, max_w: f32, scale: f32) -> Cow<'_, str> {
    let fits = |s: &str| font::width(s) as f32 * scale <= max_w;
    if fits(text) {
        return Cow::Borrowed(text);
    }
    let mut kept = String::new();
    for c in text.chars() {
        kept.push(c);
        if !fits(&format!("{kept}...")) {
            kept.pop();
            break;
        }
    }
    Cow::Owned(format!("{}...", kept.trim_end()))
}

/// Replaces `batch` with the home screen as `view` describes it.
pub fn home(batch: &mut Batch, view: &View<'_>) {
    batch.clear();
    batch.gradient_rect(0.0, 0.0, SCREEN_W, SCREEN_H, BG_TOP, BG_BOTTOM);
    shelf(batch, view);
    if view.empty {
        let scale = 6.0;
        let text = "NO APPS";
        let w = font::width(text) as f32 * scale;
        let y = layout::ROW0_Y + (layout::TILE_H - font::GLYPH_H as f32 * scale) / 2.0;
        batch.text((SCREEN_W - w) / 2.0, y, scale, text, TEXT_DIM);
    }
    // Every shadow goes under every tile, so a lifted tile's shadow never
    // darkens the edge of its neighbour.
    for tile in &view.tiles {
        shadow(batch, tile);
    }
    for tile in &view.tiles {
        draw_tile(batch, tile);
    }
    if let Some(text) = view.status {
        status(batch, text);
    }
}

fn shelf(batch: &mut Batch, view: &View<'_>) {
    let r = view.shelf;
    if r.bottom() <= 0.0 {
        return;
    }
    batch.gradient_rect(r.x, r.y, r.w, r.h, SHELF_TOP, SHELF_BOTTOM);
    let Some(title) = view.shelf_title else {
        return;
    };
    let scale = layout::SHELF_TITLE_SCALE;
    let y = r.bottom() - layout::SHELF_TITLE_GAP - font::GLYPH_H as f32 * scale;
    let caption = layout::STATUS_SCALE;
    let caption_y = y - (font::GLYPH_H as f32 + 6.0) * caption;
    batch.text(SAFE.x, caption_y, caption, "TOP SHELF", TEXT_DIM);
    let title = fit(title, SAFE.w, scale);
    batch.text(SAFE.x, y, scale, &title, TEXT);
}

/// A lifted tile's soft shadow: layers stepping out, dropped below the tile,
/// that together reach `SHADOW_OPACITY` where they all overlap.
fn shadow(batch: &mut Batch, tile: &TileView<'_>) {
    if tile.lift <= 0.0 {
        return;
    }
    let r = tile.rect;
    let radius = layout::TILE_RADIUS * r.w / layout::TILE_W;
    let layers = layout::SHADOW_LAYERS as f32;
    let each = 1.0 - (1.0 - layout::SHADOW_OPACITY).powf(1.0 / layers);
    let color = SHADOW.alpha(each * tile.lift.min(1.0));
    let drop = layout::SHADOW_OFFSET * tile.lift;
    for i in 1..=layout::SHADOW_LAYERS {
        let d = i as f32 * layout::SHADOW_STEP;
        let s = r.outset(d).offset(0.0, drop);
        batch.rounded_rect(s.x, s.y, s.w, s.h, radius + d, color, color);
    }
}

fn draw_tile(batch: &mut Batch, tile: &TileView<'_>) {
    let r = tile.rect;
    let zoom = r.w / layout::TILE_W;
    let radius = layout::TILE_RADIUS * zoom;
    let shade = REST_SHADE + (1.0 - REST_SHADE) * tile.lift.clamp(0.0, 1.0);
    let base = tile_color(tile.index).shade(shade);
    let top = base.lerp(WHITE, TILE_SHEEN);
    batch.rounded_rect(r.x, r.y, r.w, r.h, radius, top, base);

    let scale = layout::TITLE_SCALE * zoom;
    let max_w = (layout::TILE_W - 2.0 * layout::TITLE_PAD) * zoom;
    let title = fit(&tile.app.title, max_w, scale);
    let w = font::width(&title) as f32 * scale;
    let h = font::GLYPH_H as f32 * scale;
    batch.text(
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - h) / 2.0,
        scale,
        &title,
        TEXT,
    );
}

/// The status line: a dark pill at the top right of the safe area, over the
/// Top Shelf when focus is on row 0, where launches usually happen.
fn status(batch: &mut Batch, text: &str) {
    let scale = layout::STATUS_SCALE;
    let (pad_x, pad_y) = (32.0, 14.0);
    // A long error (from luna, say) must not run off the screen.
    let text = fit(text, SAFE.w - 2.0 * pad_x, scale);
    let text = text.as_ref();
    let w = font::width(text) as f32 * scale + 2.0 * pad_x;
    let h = font::GLYPH_H as f32 * scale + 2.0 * pad_y;
    let (x, y) = (SAFE.right() - w, SAFE.y);
    batch.rounded_rect(x, y, w, h, h / 2.0, PILL, PILL);
    batch.text_right(SAFE.right() - pad_x, y + pad_y, scale, text, TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use lumen_core::apps::{Launcher, MockLauncher};
    use lumen_core::focus::Dir;
    use lumen_core::home::{BackAtTop, Home};
    use lumen_core::input::{Action, Press};

    /// The B9 held 60 fps at about 3,000 quads in one call (hello-gles); the
    /// home screen stays at or under that.
    const QUAD_BUDGET: usize = 3000;

    fn mock_home() -> Home {
        Home::new(MockLauncher::default().list().unwrap(), BackAtTop::Exit)
    }

    fn quads(home: &Home) -> usize {
        let mut batch = Batch::default();
        super::home(&mut batch, &home.view());
        batch.quad_count()
    }

    #[test]
    fn the_start_screen_fits_the_quad_budget() {
        let n = quads(&mock_home());
        assert!(n > 100, "{n} quads: nothing drawn?");
        assert!(n <= QUAD_BUDGET, "{n} quads");
    }

    #[test]
    fn a_scrolled_screen_with_a_status_line_fits_the_quad_budget() {
        let mut home = mock_home();
        let press = Press {
            action: Action::Move(Dir::Down),
            repeat: false,
        };
        assert_eq!(home.handle(press, Duration::ZERO), None);
        home.settle_now();
        home.show_status("Could not open Web Browser".to_owned(), Duration::ZERO);
        let n = quads(&home);
        assert!(n <= QUAD_BUDGET, "{n} quads");
    }

    #[test]
    fn a_long_list_scrolled_deep_fits_the_quad_budget() {
        let apps = MockLauncher::default()
            .list()
            .unwrap()
            .into_iter()
            .cycle()
            .take(40)
            .collect();
        let mut home = Home::new(apps, BackAtTop::Exit);
        let down = Press {
            action: Action::Move(Dir::Down),
            repeat: false,
        };
        for row in 1..=5 {
            assert_eq!(home.handle(down, Duration::ZERO), None);
            home.settle_now();
            let n = quads(&home);
            assert!(n <= QUAD_BUDGET, "row {row}: {n} quads");
        }
    }

    #[test]
    fn the_empty_state_draws() {
        let home = Home::new(Vec::new(), BackAtTop::Exit);
        assert!(quads(&home) > 2);
    }

    #[test]
    fn long_titles_are_cut_to_fit() {
        let max = layout::TILE_W - 2.0 * layout::TITLE_PAD;
        let scale = layout::TITLE_SCALE;
        assert_eq!(fit("Netflix", max, scale), "Netflix");
        let cut = fit("An Extremely Long Application Name", max, scale);
        assert!(cut.ends_with("..."), "{cut}");
        assert!(font::width(&cut) as f32 * scale <= max);
        assert!(cut.len() > 10, "kept as much as fits: {cut}");
        assert_eq!(fit("Anything", 1.0, scale), "...");
    }

    #[test]
    fn no_tile_shares_a_colour_with_its_neighbours() {
        let cols = layout::COLUMNS;
        for i in 0..40 {
            if (i + 1) % cols != 0 {
                assert_ne!(tile_color(i), tile_color(i + 1), "tiles {i} and the next");
            }
            assert_ne!(tile_color(i), tile_color(i + cols), "tile {i} and below");
        }
    }
}
