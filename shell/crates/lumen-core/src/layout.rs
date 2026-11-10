//! Where everything sits, in the fixed 1920x1080 space the UI is designed for
//! (docs/specs/look-and-feel.md, "The home"). The home is one page that never
//! scrolls: the clock at the top right, the focused app's name, one row of
//! wide tiles and a slim line of input chips under it, all over the art. The
//! "Other apps" page is a grid that scrolls; its "content" coordinates scroll
//! with it, screen coordinates do not. Look values (gaps, radii, type sizes,
//! the focus scale and shadows) live in `style`; the names here are aliases.

use crate::style;

pub const SCREEN_W: f32 = 1920.0;
pub const SCREEN_H: f32 = 1080.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn centre_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    /// Scaled by `s` about its centre.
    pub fn scaled(&self, s: f32) -> Self {
        let (w, h) = (self.w * s, self.h * s);
        Self::new(
            self.x - (w - self.w) / 2.0,
            self.y - (h - self.h) / 2.0,
            w,
            h,
        )
    }

    pub fn offset(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Grown by `d` on every side.
    pub fn outset(&self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
}

/// The safe area, `style::SAFE_X` and `style::SAFE_Y` in from the edges.
pub const SAFE: Rect = Rect::new(
    style::SAFE_X,
    style::SAFE_Y,
    SCREEN_W - 2.0 * style::SAFE_X,
    SCREEN_H - 2.0 * style::SAFE_Y,
);

// The home's row of tiles.

/// The most tiles the row holds: the contract's four apps and "Other apps"
/// (decision 10), five across the safe width with nothing off screen.
pub const ROW_MAX: usize = 5;
/// 16:9, so a tile is a small screen.
pub const TILE_W: f32 = 320.0;
pub const TILE_H: f32 = 180.0;
pub const TILE_RADIUS: f32 = style::TILE_RADIUS;
pub const COLUMN_GAP: f32 = style::COLUMN_GAP;
pub const COLUMN_PITCH: f32 = TILE_W + COLUMN_GAP;
/// The row's top edge, at rest.
pub const ROW_Y: f32 = 612.0;
pub const FOCUS_SCALE: f32 = style::FOCUS_SCALE;

// The input chips under the row.

/// The most chips the line holds: four HDMI inputs, any others, and
/// Settings; more go to "Other apps".
pub const CHIPS_MAX: usize = 7;
/// A chip's width before its name is measured (`chip_width`).
pub const CHIP_W: f32 = 220.0;
/// The narrowest and widest a chip is: "PC" is not a dot, and a long name
/// is cut before it crowds out its neighbours.
pub const CHIP_MIN_W: f32 = 160.0;
pub const CHIP_MAX_W: f32 = 360.0;
pub const CHIP_H: f32 = 60.0;
pub const CHIP_GAP: f32 = style::CHIP_GAP;
pub const CHIP_Y: f32 = 900.0;
pub const CHIP_FOCUS_SCALE: f32 = style::CHIP_FOCUS_SCALE;

// Text on the home.

/// The focused app's name: its baseline, at the safe area's left.
pub const TITLE_BASELINE: f32 = 500.0;
/// The line under it.
pub const SUBTITLE_BASELINE: f32 = 556.0;
/// The clock, right-aligned to the safe area: its baseline.
pub const CLOCK_BASELINE: f32 = SAFE.y + 44.0;
/// What is playing, under the clock.
pub const PLAYING_BASELINE: f32 = CLOCK_BASELINE + 46.0;

pub const SHADOW_BLUR: f32 = style::SHADOW_BLUR;
pub const SHADOW_OFFSET: f32 = style::SHADOW_OFFSET;
pub const SHADOW_OPACITY: f32 = style::SHADOW_OPACITY;

/// A tile's icon: its longer side, at rest. It grows with the tile.
pub const ICON_SIZE: f32 = 104.0;

pub const TITLE_PX: f32 = style::TITLE_PX;
pub const SUBTITLE_PX: f32 = style::SUBTITLE_PX;
pub const STATUS_PX: f32 = style::STATUS_PX;
pub const EMPTY_PX: f32 = style::EMPTY_PX;

// The gear and the Control Center.

/// The gear that opens the Control Center: a round button at the top
/// right, left of the clock with room for its widest time (`12:59`), its
/// middle level with the clock's figures. Up from the row reaches it.
pub const GEAR: Rect = Rect::new(
    SAFE.x + SAFE.w - 150.0 - GEAR_SIZE,
    SAFE.y - 6.0,
    GEAR_SIZE,
    GEAR_SIZE,
);
pub const GEAR_SIZE: f32 = 64.0;
/// The gear's pictogram, its side.
pub const GEAR_ICON: f32 = 40.0;
pub const GEAR_FOCUS_SCALE: f32 = 1.12;

/// The Control Center's panel, open: the right of the safe area, top to
/// bottom.
pub const PANEL: Rect = Rect::new(SAFE.x + SAFE.w - PANEL_W, SAFE.y, PANEL_W, SAFE.h);
pub const PANEL_W: f32 = 640.0;
pub const PANEL_RADIUS: f32 = 28.0;
/// Inside it, from its left and top edges.
pub const PANEL_PAD: f32 = 44.0;
pub const PANEL_HEADING_BASELINE: f32 = 84.0;
/// A section's name, and its first line's baseline below it.
pub const PRIVACY_SECTION_BASELINE: f32 = 156.0;
pub const PRIVACY_LINE_BASELINE: f32 = 208.0;
pub const PRIVACY_LINE_PITCH: f32 = 50.0;
/// The note under the Privacy lines: that they only read.
pub const PRIVACY_NOTE_GAP: f32 = 24.0;
pub const MOTION_SECTION_BASELINE: f32 = 500.0;
/// Reduce Motion's rows: the first's top, each one's height and pitch.
pub const MOTION_ROW_Y: f32 = 528.0;
pub const MOTION_ROW_H: f32 = 72.0;
pub const MOTION_ROW_PITCH: f32 = 78.0;
pub const MOTION_ROW_RADIUS: f32 = 16.0;
/// From the last row's bottom to the top of the note under it.
pub const MOTION_NOTE_GAP: f32 = 14.0;
/// How far a row reaches past the panel's padding on each side, so its
/// label and switch line up with the text above.
pub const MOTION_ROW_INSET: f32 = 16.0;
/// A row's switch: its track, right-aligned in the row.
pub const SWITCH_W: f32 = 60.0;
pub const SWITCH_H: f32 = 34.0;
pub const SWITCH_INSET: f32 = 4.0;

/// The panel's rect `offset` pixels right of its place.
pub fn panel(offset: f32) -> Rect {
    PANEL.offset(offset, 0.0)
}

/// Reduce Motion's row `i`, inside the panel at `panel`.
pub fn motion_row(panel: Rect, i: usize) -> Rect {
    Rect::new(
        panel.x + PANEL_PAD - MOTION_ROW_INSET,
        panel.y + MOTION_ROW_Y + i as f32 * MOTION_ROW_PITCH,
        panel.w - 2.0 * (PANEL_PAD - MOTION_ROW_INSET),
        MOTION_ROW_H,
    )
}

/// The resting rect of the row's tile `i`.
pub fn row_tile(i: usize) -> Rect {
    Rect::new(SAFE.x + i as f32 * COLUMN_PITCH, ROW_Y, TILE_W, TILE_H)
}

/// A chip's width for a name `text_w` wide at `style::CHIP_PX`: the name,
/// its padding each side and room for the dot of what is playing, so that
/// never cuts it, from `CHIP_MIN_W` to `CHIP_MAX_W`.
pub fn chip_width(text_w: f32) -> f32 {
    let w = text_w + 2.0 * style::CHIP_PAD_X + style::LIVE_DOT + style::LIVE_GAP;
    w.ceil().clamp(CHIP_MIN_W, CHIP_MAX_W)
}

/// The chips' resting rects, `widths` wide, left to right from the safe
/// area's left a gap apart; all narrowed alike if they would run past its
/// right.
pub fn chip_rects(widths: &[f32]) -> Vec<Rect> {
    let gaps = CHIP_GAP * widths.len().saturating_sub(1) as f32;
    let total: f32 = widths.iter().sum();
    let k = if total + gaps > SAFE.w {
        (SAFE.w - gaps) / total
    } else {
        1.0
    };
    let mut x = SAFE.x;
    widths
        .iter()
        .map(|w| {
            let w = (w * k).floor();
            let rect = Rect::new(x, CHIP_Y, w, CHIP_H);
            x += w + CHIP_GAP;
            rect
        })
        .collect()
}

/// A tile's `rect` with `lift` (0 resting, 1 focused) applied.
pub fn lifted(rect: Rect, lift: f32) -> Rect {
    rect.scaled(1.0 + (FOCUS_SCALE - 1.0) * lift)
}

/// A chip's `rect` with `lift` applied.
pub fn chip_lifted(rect: Rect, lift: f32) -> Rect {
    rect.scaled(1.0 + (CHIP_FOCUS_SCALE - 1.0) * lift)
}

/// The one of `count` items laid out by `rect_of` whose middle is nearest
/// `x`, the earlier on a tie: where Up and Down land between the row and
/// the chips. `None` if there are none.
pub fn nearest(count: usize, x: f32, rect_of: impl Fn(usize) -> Rect) -> Option<usize> {
    (0..count).min_by(|&a, &b| {
        let da = (rect_of(a).centre_x() - x).abs();
        let db = (rect_of(b).centre_x() - x).abs();
        da.total_cmp(&db)
    })
}

// The "Other apps" page: a grid that scrolls, in content coordinates.

pub const GRID_COLUMNS: usize = 5;
/// The page's heading: its baseline, at the safe area's left.
pub const HEADING_BASELINE: f32 = 150.0;
/// The grid's first row, at rest.
pub const GRID_Y: f32 = 230.0;
pub const GRID_PITCH: f32 = TILE_H + style::ROW_GAP;
pub const GRID_TITLE_PX: f32 = style::GRID_TITLE_PX;
pub const TITLE_GAP: f32 = style::TITLE_GAP;

/// The resting rect of the grid's tile `i`, in content coordinates.
pub fn grid_tile(i: usize) -> Rect {
    Rect::new(
        SAFE.x + (i % GRID_COLUMNS) as f32 * COLUMN_PITCH,
        GRID_Y + (i / GRID_COLUMNS) as f32 * GRID_PITCH,
        TILE_W,
        TILE_H,
    )
}

/// The scroll that shows grid row `row` focused: none for the first two;
/// from the third on, the focused row sits second, so the row above it
/// still shows.
pub fn grid_scroll_for_row(row: usize) -> f32 {
    row.saturating_sub(1) as f32 * GRID_PITCH
}

/// Whether a tile drawn at `rect` (on screen: lifted, bumped and scrolled)
/// shows at all, counting its shadow when it is lifted.
pub fn tile_on_screen(rect: Rect, lift: f32) -> bool {
    let (above, below) = if lift > 0.0 {
        (SHADOW_BLUR, SHADOW_BLUR + SHADOW_OFFSET)
    } else {
        (0.0, 0.0)
    };
    rect.bottom() + below > 0.0 && rect.y - above < SCREEN_H
}

/// How far a fully lifted tile grows past its resting edge, top and bottom.
fn lift_growth() -> f32 {
    TILE_H * (FOCUS_SCALE - 1.0) / 2.0
}

/// How far below a tile's resting edge a fully lifted tile's shadow reaches.
pub fn shadow_reach() -> f32 {
    lift_growth() + SHADOW_OFFSET + SHADOW_BLUR
}

/// How far below a grid tile's resting edge the focused name's line can
/// reach: a whole em below the top of its capitals bounds the descenders.
pub fn grid_title_reach() -> f32 {
    lift_growth() + TITLE_GAP + GRID_TITLE_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn five_tiles_fill_the_safe_width_exactly() {
        assert_eq!(row_tile(0).x, SAFE.x);
        assert_eq!(row_tile(ROW_MAX - 1).right(), SAFE.right());
        assert!(close(TILE_W / TILE_H, 16.0 / 9.0));
    }

    #[test]
    fn the_most_chips_fit_the_safe_width_at_any_widths() {
        let unmeasured = chip_rects(&[CHIP_W; CHIPS_MAX]);
        assert_eq!(unmeasured[0].x, SAFE.x);
        assert!(unmeasured[CHIPS_MAX - 1].right() <= SAFE.right());
        let widest = chip_rects(&[CHIP_MAX_W; CHIPS_MAX]);
        assert!(widest[CHIPS_MAX - 1].right() <= SAFE.right());
        for pair in widest.windows(2) {
            assert!((pair[1].x - pair[0].right() - CHIP_GAP).abs() < 1e-3);
            assert_eq!(pair[0].w, pair[1].w);
        }
        // Lifted, a widest chip keeps clear of its neighbour.
        let lifted = chip_lifted(widest[1], 1.0);
        assert!(lifted.x > widest[0].right() && lifted.right() < widest[2].x);
    }

    #[test]
    fn a_chip_is_its_name_and_padding_within_its_bounds() {
        assert_eq!(chip_width(0.0), CHIP_MIN_W);
        assert_eq!(chip_width(1000.0), CHIP_MAX_W);
        let room = 2.0 * style::CHIP_PAD_X + style::LIVE_DOT + style::LIVE_GAP;
        assert_eq!(chip_width(170.4), (170.4 + room).ceil());
        let rects = chip_rects(&[160.0, 240.0, 300.0]);
        assert_eq!(rects[1].x, SAFE.x + 160.0 + CHIP_GAP);
        assert_eq!(rects[2].w, 300.0);
    }

    #[test]
    fn a_lifted_tile_keeps_clear_of_its_neighbours_and_the_text() {
        let focused = lifted(row_tile(1), 1.0);
        assert!(row_tile(2).x - focused.right() > 15.0);
        assert!(focused.x - row_tile(0).right() > 15.0);
        // The name's line ends a capital's height of 30 px below its
        // baseline at most; the lifted tile starts well below that.
        assert!(focused.y - SUBTITLE_BASELINE > 30.0, "{}", focused.y);
    }

    #[test]
    fn the_focused_tiles_shadow_ends_above_the_lifted_chips() {
        let reach = ROW_Y + TILE_H + shadow_reach();
        let chip = chip_lifted(chip_rects(&[CHIP_W])[0], 1.0);
        assert!(reach < chip.y, "{reach} against {}", chip.y);
        assert!(chip.bottom() < SAFE.bottom());
    }

    #[test]
    fn the_clock_sits_in_the_safe_area_above_the_title() {
        const { assert!(CLOCK_BASELINE - style::CLOCK_PX * 0.73 >= SAFE.y - 1.0) };
        const { assert!(PLAYING_BASELINE + style::PLAYING_PX < TITLE_BASELINE - TITLE_PX) };
    }

    #[test]
    fn nearest_picks_by_middles_and_the_earlier_on_a_tie() {
        // Tile 1's middle (600) is nearer chip 2's (678) than chip 1's
        // (434).
        let chips = chip_rects(&[CHIP_W; 5]);
        let chip_rect = |i: usize| chips[i];
        assert_eq!(nearest(5, row_tile(1).centre_x(), chip_rect), Some(2));
        assert_eq!(nearest(5, chip_rect(4).centre_x(), row_tile), Some(3));
        assert_eq!(nearest(0, 100.0, row_tile), None);
        let tie = |i: usize| Rect::new(i as f32 * 20.0, 0.0, 10.0, 10.0);
        assert_eq!(nearest(2, 15.0, tie), Some(0));
    }

    #[test]
    fn the_grid_scrolls_from_its_third_row_and_titles_fit_the_gap() {
        assert_eq!(grid_scroll_for_row(0), 0.0);
        assert_eq!(grid_scroll_for_row(1), 0.0);
        assert_eq!(grid_scroll_for_row(3), 2.0 * GRID_PITCH);
        for row in 1..8 {
            let y = grid_tile(row * GRID_COLUMNS).y - grid_scroll_for_row(row);
            assert!(y <= GRID_Y + GRID_PITCH);
        }
        assert!(grid_title_reach() < GRID_PITCH - TILE_H);
        assert!(shadow_reach() < GRID_PITCH - TILE_H);
        // Three rows show at the top; the third's name stays on screen.
        let third = grid_tile(2 * GRID_COLUMNS);
        assert!(third.bottom() + grid_title_reach() < SCREEN_H);
    }

    #[test]
    fn tiles_off_screen_are_culled() {
        let above = Rect::new(80.0, -TILE_H - 10.0, TILE_W, TILE_H);
        assert!(!tile_on_screen(above, 0.0));
        assert!(tile_on_screen(above, 1.0), "its shadow reaches down");
        let below = Rect::new(80.0, SCREEN_H + 10.0, TILE_W, TILE_H);
        assert!(tile_on_screen(below, 1.0), "its shadow spreads up");
        assert!(!tile_on_screen(below.offset(0.0, 40.0), 1.0));
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.scaled(2.0), Rect::new(-40.0, -5.0, 200.0, 100.0));
        assert_eq!(r.offset(5.0, -5.0), Rect::new(15.0, 15.0, 100.0, 50.0));
        assert_eq!(r.outset(2.0), Rect::new(8.0, 18.0, 104.0, 54.0));
        assert_eq!((r.right(), r.bottom(), r.centre_x()), (110.0, 70.0, 60.0));
    }
}
