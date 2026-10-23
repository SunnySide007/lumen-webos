//! Draws the home screen from `lumen_core::home::View` into a `Batch`: tiles
//! in their app's colour with its icon (or the first letter of its title), the
//! focused app's title below its tile, the placeholder Top Shelf and the
//! status pill, with all text in Inter.

use lumen_core::apps::App;
use lumen_core::home::{TileView, View};
use lumen_core::layout::{self, SAFE, SCREEN_H, SCREEN_W};
use lumen_core::style::{self, Color};

use crate::atlas::Atlas;
use crate::icons::Icons;
use crate::render::{Batch, Rgba};
use crate::text::Text;

// The look's values live in `lumen_core::style`.
const BG_TOP: Rgba = Rgba::of(style::BACKGROUND.top);
const BG_BOTTOM: Rgba = Rgba::of(style::BACKGROUND.bottom);
const SHELF_TOP: Rgba = Rgba::of(style::SHELF.top);
const SHELF_BOTTOM: Rgba = Rgba::of(style::SHELF.bottom);
const TEXT: Rgba = Rgba::of(style::TEXT);
const TEXT_DIM: Rgba = Rgba::of(style::TEXT_DIM);
const SHADOW: Rgba = Rgba::of(style::SHADOW);
const PILL: Rgba = Rgba::of(style::PILL);
const ON_TILE: Rgba = Rgba::of(style::ON_TILE);
const WHITE: Rgba = Rgba::of(style::WHITE);
const REST_SHADE: f32 = style::TILE_REST_SHADE;
const TILE_SHEEN: f32 = style::TILE_SHEEN;
const PALETTE: [Color; 8] = style::TILE_PALETTE;

pub fn tile_color(index: usize) -> Rgba {
    Rgba::of(PALETTE[index % PALETTE.len()])
}

/// The app's own colour, or the palette's for its place in the list.
fn app_color(app: &App, index: usize) -> Rgba {
    match app.color {
        Some([red, green, blue]) => Rgba::rgb8(red, green, blue),
        None => tile_color(index),
    }
}

/// What drawing needs besides the view: the atlas, and the text and icons
/// that fill it.
pub struct Assets {
    pub atlas: Atlas,
    pub text: Text,
    pub icons: Icons,
}

impl Assets {
    /// Icons load as they are first drawn: deterministic, for tests and
    /// `--fixed-dt`.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            atlas: Atlas::default(),
            text: Text::new()?,
            icons: Icons::default(),
        })
    }

    /// Icons load on a thread of their own; a tile is drawn plain until its
    /// icon arrives.
    pub fn threaded() -> Result<Self, String> {
        Ok(Self {
            icons: Icons::threaded(),
            ..Self::new()?
        })
    }
}

/// Replaces `batch` with the home screen as `view` describes it.
pub fn home(batch: &mut Batch, view: &View<'_>, assets: &mut Assets) {
    let generation = assets.atlas.generation();
    paint(batch, view, assets);
    // The atlas filled up and was emptied part way: quads drawn before that
    // point at pixels that are gone, so draw the frame again into the fresh
    // one. Once is enough: with icons kept at most 160 px, one frame's icons
    // and glyphs fill well under half of an empty atlas.
    if assets.atlas.generation() != generation {
        paint(batch, view, assets);
    }
}

fn paint(batch: &mut Batch, view: &View<'_>, assets: &mut Assets) {
    batch.clear();
    batch.gradient_rect(0.0, 0.0, SCREEN_W, SCREEN_H, BG_TOP, BG_BOTTOM);
    shelf(batch, view, assets);
    if view.empty {
        let px = layout::EMPTY_PX;
        let text = "No apps";
        let w = assets.text.width(text, px);
        let baseline = layout::ROW0_Y + (layout::TILE_H + assets.text.cap_height(px)) / 2.0;
        let x = (SCREEN_W - w) / 2.0;
        assets.text.draw(
            batch,
            &mut assets.atlas,
            x,
            baseline,
            px,
            1.0,
            text,
            TEXT_DIM,
        );
    }
    // Every shadow goes under every tile, so a lifted tile's shadow never
    // darkens the edge of its neighbour; titles go over everything.
    for tile in &view.tiles {
        shadow(batch, tile);
    }
    for tile in &view.tiles {
        draw_tile(batch, tile, assets);
    }
    for tile in &view.tiles {
        title(batch, tile, assets);
    }
    if let Some(text) = view.status {
        status(batch, text, assets);
    }
}

fn shelf(batch: &mut Batch, view: &View<'_>, assets: &mut Assets) {
    let r = view.shelf;
    if r.bottom() <= 0.0 {
        return;
    }
    batch.gradient_rect(r.x, r.y, r.w, r.h, SHELF_TOP, SHELF_BOTTOM);
    let Some(title) = view.shelf_title else {
        return;
    };
    let px = layout::SHELF_TITLE_PX;
    let baseline = r.bottom() - layout::SHELF_TITLE_GAP;
    let caption = layout::SHELF_CAPTION_PX;
    let caption_baseline = baseline - assets.text.cap_height(px) - caption;
    let text = &mut assets.text;
    let atlas = &mut assets.atlas;
    text.draw(
        batch,
        atlas,
        SAFE.x,
        caption_baseline,
        caption,
        1.0,
        "TOP SHELF",
        TEXT_DIM,
    );
    let title = text.fit(title, px, SAFE.w);
    text.draw(batch, atlas, SAFE.x, baseline, px, 1.0, &title, TEXT);
}

/// A lifted tile's soft shadow, dropped below it, as dark as its lift.
fn shadow(batch: &mut Batch, tile: &TileView<'_>) {
    if tile.lift <= 0.0 {
        return;
    }
    let r = tile.rect;
    let radius = layout::TILE_RADIUS * r.w / layout::TILE_W;
    let lift = tile.lift.min(1.0);
    let s = r.offset(0.0, layout::SHADOW_OFFSET * lift);
    let color = SHADOW.alpha(layout::SHADOW_OPACITY * lift);
    batch.shadow(s.x, s.y, s.w, s.h, radius, layout::SHADOW_BLUR, color);
}

fn draw_tile(batch: &mut Batch, tile: &TileView<'_>, assets: &mut Assets) {
    let r = tile.rect;
    let zoom = r.w / layout::TILE_W;
    let radius = layout::TILE_RADIUS * zoom;
    let shade = REST_SHADE + (1.0 - REST_SHADE) * tile.lift.clamp(0.0, 1.0);
    let base = app_color(tile.app, tile.index).shade(shade);
    let top = base.lerp(WHITE, TILE_SHEEN);
    batch.rounded_rect(r.x, r.y, r.w, r.h, radius, top, base);

    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let icon = tile
        .app
        .icon
        .as_deref()
        .and_then(|path| assets.icons.get(path, &mut assets.atlas));
    if let Some((uv, w, h)) = icon {
        let side = layout::ICON_SIZE * zoom / w.max(h) as f32;
        let (iw, ih) = (w as f32 * side, h as f32 * side);
        batch.image(cx - iw / 2.0, cy - ih / 2.0, iw, ih, uv, ON_TILE);
        return;
    }
    // Still loading: the plain tile, so its letter does not flash up first.
    if tile
        .app
        .icon
        .as_deref()
        .is_some_and(|path| assets.icons.is_loading(path))
    {
        return;
    }
    // No icon: the title's first letter, rasterised once at its lifted size
    // and scaled, so the animation does not fill the atlas with sizes.
    let Some(first) = tile.app.title.chars().find(|c| !c.is_whitespace()) else {
        return;
    };
    let letter: String = first.to_uppercase().collect();
    let px = (layout::LETTER_PX * layout::FOCUS_SCALE).round();
    let scale = layout::LETTER_PX * zoom / px;
    let text = &mut assets.text;
    let w = text.width(&letter, px) * scale;
    let baseline = cy + text.cap_height(px) * scale / 2.0;
    text.draw(
        batch,
        &mut assets.atlas,
        cx - w / 2.0,
        baseline,
        px,
        scale,
        &letter,
        ON_TILE,
    );
}

/// The focused app's name below its tile, fading in as the tile lifts.
fn title(batch: &mut Batch, tile: &TileView<'_>, assets: &mut Assets) {
    let lift = tile.lift.min(1.0);
    if lift <= 0.01 {
        return;
    }
    let px = layout::TITLE_PX;
    let r = tile.rect;
    let text = &mut assets.text;
    let name = text.fit(&tile.app.title, px, layout::TILE_W * layout::FOCUS_SCALE);
    let w = text.width(&name, px);
    let baseline = r.bottom() + layout::TITLE_GAP + text.cap_height(px);
    let x = r.x + (r.w - w) / 2.0;
    text.draw(
        batch,
        &mut assets.atlas,
        x,
        baseline,
        px,
        1.0,
        &name,
        TEXT.alpha(lift),
    );
}

/// The status line: a dark pill at the top right of the safe area, over the
/// Top Shelf when focus is on row 0, where launches usually happen.
fn status(batch: &mut Batch, text: &str, assets: &mut Assets) {
    let px = layout::STATUS_PX;
    let (pad_x, pad_y) = (style::PILL_PAD_X, style::PILL_PAD_Y);
    let t = &mut assets.text;
    // A long error (from luna, say) must not run off the screen.
    let text = t.fit(text, px, SAFE.w - 2.0 * pad_x);
    let cap = t.cap_height(px);
    let w = t.width(&text, px) + 2.0 * pad_x;
    let h = cap + 2.0 * pad_y;
    let (x, y) = (SAFE.right() - w, SAFE.y);
    batch.rounded_rect(x, y, w, h, h / 2.0, PILL, PILL);
    t.draw(
        batch,
        &mut assets.atlas,
        x + pad_x,
        y + pad_y + cap,
        px,
        1.0,
        &text,
        TEXT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use lumen_core::apps::{App, AppKind, Launcher, MockLauncher};
    use lumen_core::focus::Dir;
    use lumen_core::home::{BackAtTop, Home};
    use lumen_core::input::{Action, Press};

    /// A guard against regressions, not a measured limit: B1 drew 856 to 1,631
    /// quads at 60 fps on the B9, and B2 draws far fewer. What limits a B2
    /// frame, quads or shaded pixels, is for the TV run to measure.
    const QUAD_BUDGET: usize = 600;

    fn mock_home() -> Home {
        Home::new(MockLauncher::default().list().unwrap(), BackAtTop::Exit)
    }

    fn assets() -> Assets {
        Assets::new().expect("the built-in font parses")
    }

    fn quads(home: &Home, assets: &mut Assets) -> usize {
        let mut batch = Batch::default();
        super::home(&mut batch, &home.view(), assets);
        batch.quad_count()
    }

    fn down() -> Press {
        Press {
            action: Action::Move(Dir::Down),
            repeat: false,
        }
    }

    #[test]
    fn the_start_screen_fits_the_quad_budget() {
        let n = quads(&mock_home(), &mut assets());
        assert!(n > 30, "{n} quads: nothing drawn?");
        assert!(n <= QUAD_BUDGET, "{n} quads");
    }

    #[test]
    fn a_scrolled_screen_with_a_status_line_fits_the_quad_budget() {
        let mut home = mock_home();
        assert_eq!(home.handle(down(), Duration::ZERO), None);
        home.settle_now();
        home.show_status("Could not open Web Browser".to_owned(), Duration::ZERO);
        let n = quads(&home, &mut assets());
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
        let mut assets = assets();
        for row in 1..=5 {
            assert_eq!(home.handle(down(), Duration::ZERO), None);
            home.settle_now();
            let n = quads(&home, &mut assets);
            assert!(n <= QUAD_BUDGET, "row {row}: {n} quads");
        }
    }

    #[test]
    fn the_empty_state_draws() {
        let home = Home::new(Vec::new(), BackAtTop::Exit);
        assert!(quads(&home, &mut assets()) > 2);
    }

    #[test]
    fn only_the_focused_tile_shows_its_title() {
        let mut assets = assets();
        // The first tile is focused; `rename` lengthens one app's title,
        // keeping its first letter (and so its letter tile) the same.
        let settled = |rename: Option<usize>| {
            let mut apps = MockLauncher::default().list().unwrap();
            if let Some(i) = rename {
                apps[i].title.push_str(" Long");
            }
            let mut home = Home::new(apps, BackAtTop::Exit);
            home.settle_now();
            home
        };
        let plain = quads(&settled(None), &mut assets);
        assert_eq!(
            quads(&settled(Some(1)), &mut assets),
            plain,
            "an unfocused tile draws no title"
        );
        // The focused title is drawn twice, under its tile and on the Top
        // Shelf; "Netflix Long" fits both, one quad a letter.
        let longer = quads(&settled(Some(0)), &mut assets);
        assert_eq!(longer - plain, 2 * "Long".len());
    }

    #[test]
    fn a_tile_takes_its_apps_colour_or_the_palettes() {
        let app = App::new("a", "A", AppKind::App);
        assert_eq!(app_color(&app, 3), tile_color(3));
        let app = app.with_color([1, 2, 3]);
        assert_eq!(app_color(&app, 3), Rgba::rgb8(1, 2, 3));
    }

    #[test]
    fn a_missing_icon_falls_back_to_the_letter() {
        let mut assets = assets();
        let app = App::new("a", "plex", AppKind::App).with_icon("/no/such/icon.png");
        let mut home = Home::new(vec![app], BackAtTop::Exit);
        home.settle_now();
        let with_missing = quads(&home, &mut assets);
        let mut plain = Home::new(vec![App::new("a", "plex", AppKind::App)], BackAtTop::Exit);
        plain.settle_now();
        assert_eq!(with_missing, quads(&plain, &mut assets));
    }

    #[test]
    fn a_frame_that_fills_the_atlas_never_points_at_stale_pixels() {
        // Canonical, as the TV's launcher gives them.
        let icons = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/mock-icons")
            .canonicalize()
            .unwrap();
        let apps = MockLauncher::default()
            .with_icons_in(&icons)
            .list()
            .unwrap();
        let mut home = Home::new(apps, BackAtTop::Exit);
        home.settle_now();
        home.show_status("Opening Netflix".to_owned(), Duration::ZERO);
        let clean = quads(&home, &mut assets());

        // Leave R rows of the atlas free, so the frame fills it at a
        // different point each time: early, in the middle, or not at all.
        let mut emptied = 0;
        for free in (8..=320).step_by(8) {
            let mut assets = assets();
            let (w, h) = (2046, 1022 - free);
            assets
                .atlas
                .insert(w, h, &vec![0; (w * h * 4) as usize])
                .unwrap();
            let before = assets.atlas.generation();
            let mut batch = Batch::default();
            super::home(&mut batch, &home.view(), &mut assets);
            if assets.atlas.generation() != before {
                emptied += 1;
            }
            assert_eq!(batch.quad_count(), clean, "{free} rows free");
            // Every image must sample pixels placed in the current atlas.
            let live: Vec<_> = assets.atlas.take_uploads().iter().map(content_uv).collect();
            for uv in batch.image_uvs() {
                let inside = live.iter().any(|r| {
                    uv.0 >= r.0 - 1e-6
                        && uv.1 >= r.1 - 1e-6
                        && uv.2 <= r.2 + 1e-6
                        && uv.3 <= r.3 + 1e-6
                });
                assert!(inside, "{free} rows free: {uv:?} is stale");
            }
        }
        assert!(
            emptied > 10,
            "the sweep must empty the atlas mid-frame: {emptied}"
        );
    }

    /// The texture coordinates of an upload's pixels, inside its padding.
    fn content_uv(up: &crate::atlas::Upload) -> (f32, f32, f32, f32) {
        use crate::atlas::{ATLAS_H, ATLAS_W};
        let (aw, ah) = (ATLAS_W as f32, ATLAS_H as f32);
        (
            (up.x + 1) as f32 / aw,
            (up.y + 1) as f32 / ah,
            (up.x + up.w - 1) as f32 / aw,
            (up.y + up.h - 1) as f32 / ah,
        )
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
