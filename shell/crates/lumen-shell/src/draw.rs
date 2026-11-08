//! Draws the home screen from `lumen_core::home::View` into a `Batch`
//! (docs/specs/look-and-feel.md, "The home"): the art full screen behind
//! everything (its hero, or the gradient until one is in), the clock and
//! what is playing at the top right, the focused app's name over the row,
//! the row's wide tiles in their app's colour with its icon (or its name),
//! the input chips under them, the "Other apps" page when it turns in, and
//! the status pill, with all text in Inter.
//!
//! The order keeps the main shader off most of the screen: the art is drawn
//! by a program of its own (`Batch::art`), and what the main program draws
//! over it (tiles, chips, text, shadows) covers about a fifth of the pixels.

use std::collections::HashMap;

use lumen_core::apps::App;
use lumen_core::art::Hero;
use lumen_core::focus::Dir;
use lumen_core::home::{ChipView, SweepView, TileView, View, page_fades};
use lumen_core::layout::{self, Rect, SAFE, SCREEN_H, SCREEN_W};
use lumen_core::style::{self, Color};

use crate::atlas::{Atlas, Uv};
use crate::icons::Icons;
use crate::render::{Batch, Rgba, Texture, Tilt};
use crate::text::Text;

pub mod warm;

// The look's values live in `lumen_core::style`.
const BG_TOP: Rgba = Rgba::of(style::BACKGROUND.top);
const BG_BOTTOM: Rgba = Rgba::of(style::BACKGROUND.bottom);
const TEXT: Rgba = Rgba::of(style::TEXT);
const TEXT_DIM: Rgba = Rgba::of(style::TEXT_DIM);
const SHADOW: Rgba = Rgba::of(style::SHADOW);
const PILL: Rgba = Rgba::of(style::PILL);
const ON_TILE: Rgba = Rgba::of(style::ON_TILE);
const ON_LIGHT_TILE: Rgba = Rgba::of(style::CHIP_TEXT_FOCUS);
const WHITE: Rgba = Rgba::of(style::WHITE);
const CHIP: Rgba = Rgba::of(style::CHIP);
const CHIP_FOCUS: Rgba = Rgba::of(style::CHIP_FOCUS);
const CHIP_TEXT_FOCUS: Rgba = Rgba::of(style::CHIP_TEXT_FOCUS);
const LIVE: Rgba = Rgba::of(style::LIVE);
const OTHERS_DIM: Rgba = Rgba::of(style::OTHERS_DIM);
const OWNER_SCRIM: Rgba = Rgba::of(style::OWNER_SCRIM);
const REST_SHADE: f32 = style::TILE_REST_SHADE;
const TILE_SHEEN: f32 = style::TILE_SHEEN;
const PALETTE: [Color; 8] = style::TILE_PALETTE;
/// A whole texture, edge to edge.
const WHOLE: Uv = Uv {
    u0: 0.0,
    v0: 0.0,
    u1: 1.0,
    v1: 1.0,
};
/// The whole screen.
const SCREEN: Rect = Rect::new(0.0, 0.0, SCREEN_W, SCREEN_H);

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

/// What reads on a tile of colour `c`: white, or dark on a light tile (LG
/// gives Netflix and YouTube white).
fn on_color(c: Rgba) -> Rgba {
    let luminance = 0.2126 * c.0 + 0.7152 * c.1 + 0.0722 * c.2;
    if luminance > 0.6 {
        ON_LIGHT_TILE
    } else {
        ON_TILE
    }
}

/// A hero's layer in its texture: which texture, and the rows of the screen
/// it covers, top and bottom, as fractions of its height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeroLayer {
    pub texture: Texture,
    pub rows: (f32, f32),
}

/// What drawing needs besides the view: the atlas, the text and icons that
/// fill it, and the heroes uploaded into textures of their own.
pub struct Assets {
    pub atlas: Atlas,
    pub text: Text,
    pub icons: Icons,
    /// Each uploaded hero's layers, back to front.
    pub heroes: HashMap<Hero, Vec<HeroLayer>>,
}

impl Assets {
    /// Icons load as they are first drawn: deterministic, for tests and
    /// `--fixed-dt`.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            atlas: Atlas::default(),
            text: Text::new()?,
            icons: Icons::default(),
            heroes: HashMap::new(),
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
    art(batch, view, assets);
    if view.page > 0.0 {
        let dim = OTHERS_DIM.alpha(OTHERS_DIM.3 * view.page);
        batch.gradient_rect(0.0, 0.0, SCREEN_W, SCREEN_H, dim, dim);
    }
    if view.empty {
        let px = layout::EMPTY_PX;
        let text = "No apps";
        let w = assets.text.width(text, px);
        let baseline = layout::ROW_Y + (layout::TILE_H + assets.text.cap_height(px)) / 2.0;
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
    corner(batch, view, assets);
    headings(batch, view, assets);
    // Every shadow goes under every tile, so a lifted tile's shadow never
    // darkens the edge of its neighbour; names go over everything.
    for tile in &view.tiles {
        shadow(batch, tile, view.look.shadow);
    }
    // A tile leans as one piece, its icon or name and its light with it;
    // its shadow stays level.
    for tile in &view.tiles {
        if tile.tilt != [0.0; 2] {
            let r = tile.rect;
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            batch.set_tilt(Some(Tilt::new(cx, cy, tile.tilt, style::TILT_DISTANCE)));
        }
        draw_tile(batch, tile, view.look.sheen, assets);
        if let Some(sweep) = tile.sweep {
            light(batch, tile, sweep);
        }
        batch.set_tilt(None);
    }
    for tile in view.tiles.iter().filter(|t| t.named) {
        grid_title(batch, tile, assets);
    }
    for chip in &view.chips {
        draw_chip(batch, chip, assets);
    }
    if let Some(text) = view.status {
        status(batch, text, assets);
    }
}

/// The art behind everything: a hero, the gradient, or a crossfade between
/// them, full screen. The owner's art, which has no fade of its own, gets
/// one over it.
fn art(batch: &mut Batch, view: &View<'_>, assets: &Assets) {
    let art = view.art;
    picture(batch, art.base, 1.0, view.drift, assets);
    if let Some((over, alpha)) = art.over {
        picture(batch, over, alpha, view.drift, assets);
    }
    let scrim =
        |hero: Option<&Hero>| hero.is_some_and(|h| !h.has_scrim() && assets.heroes.contains_key(h));
    let alpha = match art.over {
        Some((over, a)) => {
            let base = if scrim(art.base) { 1.0 - a } else { 0.0 };
            base + if scrim(over) { a } else { 0.0 }
        }
        None => f32::from(u8::from(scrim(art.base))),
    };
    if alpha > 0.0 {
        let top = SCREEN_H * style::OWNER_SCRIM_FROM;
        let dark = OWNER_SCRIM.alpha(OWNER_SCRIM.3 * alpha);
        batch.gradient_rect(0.0, top, SCREEN_W, SCREEN_H - top, dark.alpha(0.0), dark);
    }
}

/// One picture at `alpha`: `hero`'s layers, back to front, or the gradient
/// for `None` (and for a hero with no textures, which the home loop never
/// shows). A layered hero's layers drift by `drift` (`View::drift`).
fn picture(batch: &mut Batch, hero: Option<&Hero>, alpha: f32, drift: [f32; 2], assets: &Assets) {
    let Some(layers) = hero.and_then(|hero| assets.heroes.get(hero)) else {
        batch.gradient_rect(
            0.0,
            0.0,
            SCREEN_W,
            SCREEN_H,
            BG_TOP.alpha(alpha),
            BG_BOTTOM.alpha(alpha),
        );
        return;
    };
    if layers.len() > 1 {
        drifting_layers(batch, SCREEN, layers, alpha, drift);
        return;
    }
    for layer in layers {
        let (top, bottom) = (layer.rows.0 * SCREEN_H, layer.rows.1 * SCREEN_H);
        batch.art(
            layer.texture,
            0.0,
            top,
            SCREEN_W,
            bottom - top,
            WHOLE,
            WHITE.alpha(alpha),
        );
    }
}

/// A layered hero's layers, each moved `drift` times its own
/// `style::PARALLAX_PX` against the last move. All are drawn alike a little
/// larger than the screen, about its middle, so the layers still line up at
/// rest and none drifts far enough to show its edge: art at a fraction `a`
/// of the screen shows at `0.5 + (a - 0.5) k`, plus the layer's drift. Each
/// quad is cut to the screen, its texture window cut with it.
fn drifting_layers(batch: &mut Batch, r: Rect, layers: &[HeroLayer], alpha: f32, drift: [f32; 2]) {
    let most = style::PARALLAX_PX.iter().copied().fold(0.0, f32::max);
    let k = r.h / (r.h - 2.0 * most);
    // Along one side of the screen (from `at`, `len` long): where art at a
    // fraction shows, and which fraction shows at a place.
    let place = |a: f32, at: f32, len: f32, off: f32| at + len * (0.5 + (a - 0.5) * k) + off;
    let fraction = |p: f32, at: f32, len: f32, off: f32| 0.5 + ((p - at - off) / len - 0.5) / k;
    for (layer, px) in layers.iter().zip(style::PARALLAX_PX) {
        let (ox, oy) = (drift[0] * px, drift[1] * px);
        let (rows0, rows1) = layer.rows;
        let x0 = place(0.0, r.x, r.w, ox).max(r.x);
        let x1 = place(1.0, r.x, r.w, ox).min(r.right());
        let y0 = place(rows0, r.y, r.h, oy).max(r.y);
        let y1 = place(rows1, r.y, r.h, oy).min(r.bottom());
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        // Clamped: a layer's own edge, mapped there and back, may come out a
        // rounding error outside its texture.
        let v = |y: f32| ((fraction(y, r.y, r.h, oy) - rows0) / (rows1 - rows0)).clamp(0.0, 1.0);
        let uv = Uv {
            u0: fraction(x0, r.x, r.w, ox),
            v0: v(y0),
            u1: fraction(x1, r.x, r.w, ox),
            v1: v(y1),
        };
        batch.art(
            layer.texture,
            x0,
            y0,
            x1 - x0,
            y1 - y0,
            uv,
            WHITE.alpha(alpha),
        );
    }
}

/// The top right corner: the clock, and under it what is playing, with its
/// dot. It stays on both pages.
fn corner(batch: &mut Batch, view: &View<'_>, assets: &mut Assets) {
    let right = SAFE.right();
    let text = &mut assets.text;
    if let Some(clock) = &view.clock {
        let px = style::CLOCK_PX;
        let w = text.width(clock, px);
        let baseline = layout::CLOCK_BASELINE;
        text.draw(
            batch,
            &mut assets.atlas,
            right - w,
            baseline,
            px,
            1.0,
            clock,
            TEXT,
        );
    }
    if let Some(playing) = view.playing {
        let px = style::PLAYING_PX;
        let name = text.fit(playing, px, SAFE.w / 3.0);
        let w = text.width(&name, px);
        let baseline = layout::PLAYING_BASELINE;
        let x = right - w;
        text.draw(
            batch,
            &mut assets.atlas,
            x,
            baseline,
            px,
            1.0,
            &name,
            TEXT_DIM,
        );
        let d = style::LIVE_DOT;
        let cy = baseline - text.cap_height(px) / 2.0;
        let dx = x - style::LIVE_GAP - d;
        batch.rounded_rect(dx, cy - d / 2.0, d, d, d / 2.0, LIVE, LIVE);
    }
}

/// The focused item's name and the line under it, on the home; the
/// "Other apps" heading on its page. Each fades with its page.
fn headings(batch: &mut Batch, view: &View<'_>, assets: &mut Assets) {
    let (home, others) = page_fades(view.page);
    let text = &mut assets.text;
    let atlas = &mut assets.atlas;
    let slide = -style::PAGE_SLIDE * view.page;
    if home > 0.0 {
        if let Some(title) = view.title {
            let px = layout::TITLE_PX;
            let title = text.fit(title, px, SAFE.w * 0.62);
            let y = layout::TITLE_BASELINE + slide;
            text.draw(batch, atlas, SAFE.x, y, px, 1.0, &title, TEXT.alpha(home));
        }
        if let Some(line) = &view.subtitle {
            let px = layout::SUBTITLE_PX;
            let line = text.fit(line, px, SAFE.w * 0.62);
            let y = layout::SUBTITLE_BASELINE + slide;
            text.draw(
                batch,
                atlas,
                SAFE.x,
                y,
                px,
                1.0,
                &line,
                TEXT_DIM.alpha(home),
            );
        }
    }
    if others > 0.0 && view.heading_y > 0.0 {
        let px = style::HEADING_PX;
        let color = TEXT.alpha(others);
        let heading = lumen_core::arrange::OTHERS_TITLE;
        text.draw(
            batch,
            atlas,
            SAFE.x,
            view.heading_y,
            px,
            1.0,
            heading,
            color,
        );
    }
}

/// A tile's soft shadow, dropped below it: the lifted one's deep, and if
/// `resting` the others' close and light. Each fades with its page.
fn shadow(batch: &mut Batch, tile: &TileView<'_>, resting: bool) {
    let lift = tile.lift.clamp(0.0, 1.0);
    if lift <= 0.0 && !resting {
        return;
    }
    let mix = |rest: f32, lifted: f32| {
        let rest = if resting { rest } else { 0.0 };
        rest + (lifted - rest) * lift
    };
    let blur = mix(style::REST_SHADOW_BLUR, layout::SHADOW_BLUR);
    let offset = mix(style::REST_SHADOW_OFFSET, layout::SHADOW_OFFSET);
    let opacity = mix(style::REST_SHADOW_OPACITY, layout::SHADOW_OPACITY);
    let r = tile.rect;
    let radius = layout::TILE_RADIUS * r.w / layout::TILE_W;
    let s = r.offset(0.0, offset);
    let color = SHADOW.alpha(opacity * tile.alpha);
    batch.shadow(s.x, s.y, s.w, s.h, radius, blur.max(1.0), color);
}

fn draw_tile(batch: &mut Batch, tile: &TileView<'_>, sheen: bool, assets: &mut Assets) {
    let r = tile.rect;
    let zoom = r.w / layout::TILE_W;
    let radius = layout::TILE_RADIUS * zoom;
    let lift = tile.lift.clamp(0.0, 1.0);
    let shade = REST_SHADE + (1.0 - REST_SHADE) * lift;
    let own = app_color(tile.app, tile.index);
    let base = own.shade(shade);
    let top = base.lerp(WHITE, TILE_SHEEN);
    let a = tile.alpha;
    if sheen && lift > 0.0 {
        // The sheen: the top left corner lit, fading across to the bottom
        // right, as the lift comes.
        let s = style::FOCUS_SHEEN * lift;
        let colors = [
            top.lerp(WHITE, s),
            top.lerp(WHITE, s * 0.35),
            base.lerp(WHITE, s * 0.35),
            base,
        ];
        batch.rounded_rect4(r.x, r.y, r.w, r.h, radius, colors.map(|c| c.alpha(a)));
    } else {
        batch.rounded_rect(r.x, r.y, r.w, r.h, radius, top.alpha(a), base.alpha(a));
    }

    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let icon = tile
        .app
        .icon
        .as_deref()
        .and_then(|path| assets.icons.get(path, &mut assets.atlas));
    if let Some((uv, w, h)) = icon {
        let side = layout::ICON_SIZE * zoom / w.max(h) as f32;
        let (iw, ih) = (w as f32 * side, h as f32 * side);
        batch.image(cx - iw / 2.0, cy - ih / 2.0, iw, ih, uv, ON_TILE.alpha(a));
        return;
    }
    // Still loading: the plain tile, so its name does not flash up first.
    if tile
        .app
        .icon
        .as_deref()
        .is_some_and(|path| assets.icons.is_loading(path))
    {
        return;
    }
    // No icon: the app's name across the tile, rasterised once at its
    // lifted size and scaled, so the lift does not fill the atlas with sizes.
    let px = (style::TILE_LABEL_PX * layout::FOCUS_SCALE).round();
    let scale = style::TILE_LABEL_PX * zoom / px;
    let text = &mut assets.text;
    let room = (layout::TILE_W - 48.0) * layout::FOCUS_SCALE;
    let name = text.fit(&tile.app.title, px, room);
    let w = text.width(&name, px) * scale;
    let baseline = cy + text.cap_height(px) * scale / 2.0;
    let color = on_color(own).alpha(a);
    text.draw(
        batch,
        &mut assets.atlas,
        cx - w / 2.0,
        baseline,
        px,
        scale,
        &name,
        color,
    );
}

/// The light crossing a newly focused tile: a soft band of white, brightest
/// in its middle, that travels the way focus moved, cut by the tile's
/// corners. Two quads, one each side of the band's middle, each clipped to
/// the tile.
fn light(batch: &mut Batch, tile: &TileView<'_>, sweep: SweepView) {
    let r = tile.rect;
    let radius = layout::TILE_RADIUS * r.w / layout::TILE_W;
    let half = style::SWEEP_WIDTH / 2.0;
    // Along the tile the way the band travels: 0 where it enters, 1 where
    // it leaves.
    let forward = matches!(sweep.dir, Dir::Right | Dir::Down);
    let across = matches!(sweep.dir, Dir::Left | Dir::Right);
    let glow = |at: f32| {
        let a = style::SWEEP_STRENGTH * (1.0 - (at - sweep.at).abs() / half).max(0.0);
        WHITE.alpha(a * tile.alpha)
    };
    for (from, to) in [(sweep.at - half, sweep.at), (sweep.at, sweep.at + half)] {
        let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
        if to <= from {
            continue;
        }
        // In the tile's own terms, left to right or top to bottom.
        let (lo, hi, c_lo, c_hi) = if forward {
            (from, to, glow(from), glow(to))
        } else {
            (1.0 - to, 1.0 - from, glow(to), glow(from))
        };
        let (part, colors) = if across {
            let part = Rect {
                x: r.x + lo * r.w,
                w: (hi - lo) * r.w,
                ..r
            };
            (part, [c_lo, c_hi, c_lo, c_hi])
        } else {
            let part = Rect {
                y: r.y + lo * r.h,
                h: (hi - lo) * r.h,
                ..r
            };
            (part, [c_lo, c_lo, c_hi, c_hi])
        };
        batch.rounded_part(r, radius, part, colors);
    }
}

/// On the "Other apps" page, the focused tile's name below it, fading in as
/// it lifts.
fn grid_title(batch: &mut Batch, tile: &TileView<'_>, assets: &mut Assets) {
    let lift = tile.lift.min(1.0);
    if lift <= 0.01 {
        return;
    }
    let px = layout::GRID_TITLE_PX;
    let r = tile.rect;
    let text = &mut assets.text;
    let name = text.fit(&tile.app.title, px, layout::TILE_W * layout::FOCUS_SCALE);
    let w = text.width(&name, px);
    let baseline = r.bottom() + layout::TITLE_GAP + text.cap_height(px);
    let x = r.x + (r.w - w) / 2.0;
    let color = TEXT.alpha(lift * tile.alpha);
    text.draw(batch, &mut assets.atlas, x, baseline, px, 1.0, &name, color);
}

/// An input chip: frosted at rest, nearly white and lifted when focused,
/// its name centred, with a dot before it if it is what was playing.
fn draw_chip(batch: &mut Batch, chip: &ChipView<'_>, assets: &mut Assets) {
    let r = chip.rect;
    let lift = chip.lift.clamp(0.0, 1.0);
    let a = chip.alpha;
    let fill = CHIP.lerp(CHIP_FOCUS, lift);
    let fill = fill.alpha(fill.3 * a);
    batch.rounded_rect(r.x, r.y, r.w, r.h, r.h / 2.0, fill, fill);
    let zoom = r.w / layout::CHIP_W;
    let px = (style::CHIP_PX * layout::CHIP_FOCUS_SCALE).round();
    let scale = style::CHIP_PX * zoom / px;
    let dot = if chip.playing {
        style::LIVE_DOT + style::LIVE_GAP
    } else {
        0.0
    };
    let text = &mut assets.text;
    let room = (layout::CHIP_W - 2.0 * style::CHIP_PAD_X - dot) * layout::CHIP_FOCUS_SCALE;
    let name = text.fit(&chip.app.title, px, room);
    let w = text.width(&name, px) * scale;
    let whole = w + dot * zoom;
    let x = r.x + (r.w - whole) / 2.0;
    let cap = text.cap_height(px) * scale;
    let baseline = r.y + (r.h + cap) / 2.0;
    if chip.playing {
        let d = style::LIVE_DOT * zoom;
        let live = LIVE.alpha(a);
        batch.rounded_rect(x, baseline - cap / 2.0 - d / 2.0, d, d, d / 2.0, live, live);
    }
    let color = TEXT.lerp(CHIP_TEXT_FOCUS, lift).alpha(a);
    text.draw(
        batch,
        &mut assets.atlas,
        x + dot * zoom,
        baseline,
        px,
        scale,
        &name,
        color,
    );
}

/// The status line: a dark pill at the top left of the safe area, clear of
/// the clock.
fn status(batch: &mut Batch, text: &str, assets: &mut Assets) {
    let px = layout::STATUS_PX;
    let (pad_x, pad_y) = (style::PILL_PAD_X, style::PILL_PAD_Y);
    let t = &mut assets.text;
    // A long error (from luna, say) must not run into the clock.
    let text = t.fit(text, px, SAFE.w * 0.6 - 2.0 * pad_x);
    let cap = t.cap_height(px);
    let w = t.width(&text, px) + 2.0 * pad_x;
    let h = cap + 2.0 * pad_y;
    let (x, y) = (SAFE.x, SAFE.y);
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
mod tests;
