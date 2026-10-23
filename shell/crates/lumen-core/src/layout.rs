//! Where everything sits, in the fixed 1920x1080 space the UI is designed for
//! (docs/specs/shell-mvp.md, "Layout"). "Content" coordinates scroll with the
//! grid; screen coordinates do not. Look values (gaps, radii, type sizes, the
//! focus scale and shadow) live in `style`; the names here are aliases.

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

pub const COLUMNS: usize = 5;
/// 5:3, the shape of a tvOS app icon.
pub const TILE_W: f32 = 320.0;
pub const TILE_H: f32 = 192.0;
pub const TILE_RADIUS: f32 = style::TILE_RADIUS;
pub const COLUMN_GAP: f32 = style::COLUMN_GAP;
pub const COLUMN_PITCH: f32 = TILE_W + COLUMN_GAP;

/// The placeholder Top Shelf band, in content coordinates.
pub const TOP_SHELF: Rect = Rect::new(0.0, 0.0, SCREEN_W, 640.0);
pub const ROW0_Y: f32 = 700.0;
pub const ROW_PITCH: f32 = TILE_H + style::ROW_GAP;
/// Screen y where a focused row below row 0 sits.
pub const FOCUS_ROW_Y: f32 = 340.0;
pub const FOCUS_SCALE: f32 = style::FOCUS_SCALE;

pub const SHADOW_BLUR: f32 = style::SHADOW_BLUR;
pub const SHADOW_OFFSET: f32 = style::SHADOW_OFFSET;
pub const SHADOW_OPACITY: f32 = style::SHADOW_OPACITY;

/// A tile's icon: its longer side, at rest. It grows with the tile.
pub const ICON_SIZE: f32 = 120.0;
pub const LETTER_PX: f32 = style::LETTER_PX;

pub const TITLE_PX: f32 = style::TITLE_PX;
pub const TITLE_GAP: f32 = style::TITLE_GAP;
pub const SHELF_TITLE_PX: f32 = style::SHELF_TITLE_PX;
pub const SHELF_CAPTION_PX: f32 = style::SHELF_CAPTION_PX;
pub const SHELF_TITLE_GAP: f32 = style::SHELF_TITLE_GAP;
pub const STATUS_PX: f32 = style::STATUS_PX;
pub const EMPTY_PX: f32 = style::EMPTY_PX;

pub const fn row_of(index: usize) -> usize {
    index / COLUMNS
}

pub const fn column_of(index: usize) -> usize {
    index % COLUMNS
}

pub fn row_y(row: usize) -> f32 {
    ROW0_Y + row as f32 * ROW_PITCH
}

/// The resting rect of tile `index`, in content coordinates.
pub fn tile_rect(index: usize) -> Rect {
    Rect::new(
        SAFE.x + column_of(index) as f32 * COLUMN_PITCH,
        row_y(row_of(index)),
        TILE_W,
        TILE_H,
    )
}

/// `rect` with `lift` (0 resting, 1 focused) applied.
pub fn lifted(rect: Rect, lift: f32) -> Rect {
    rect.scaled(1.0 + (FOCUS_SCALE - 1.0) * lift)
}

/// The content scroll that shows `row` focused: none for row 0, otherwise the
/// row sits at `FOCUS_ROW_Y`.
pub fn scroll_for_row(row: usize) -> f32 {
    if row == 0 {
        0.0
    } else {
        row_y(row) - FOCUS_ROW_Y
    }
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

/// How far below a tile's resting edge the focused title's line can reach:
/// a whole em below the top of its capitals bounds the descenders too.
pub fn title_reach() -> f32 {
    lift_growth() + TITLE_GAP + TITLE_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn five_tiles_fill_the_safe_width_exactly() {
        assert_eq!(tile_rect(0).x, SAFE.x);
        assert_eq!(tile_rect(COLUMNS - 1).right(), SAFE.right());
        assert_eq!(tile_rect(COLUMNS).x, SAFE.x, "the sixth tile starts row 1");
        assert_eq!(tile_rect(COLUMNS).y, ROW0_Y + ROW_PITCH);
    }

    #[test]
    fn a_lifted_tile_keeps_clear_of_its_neighbours() {
        let focused = lifted(tile_rect(1), 1.0);
        assert!(close(focused.w, 368.0) && close(focused.h, 220.8));
        assert!(close(tile_rect(2).x - focused.right(), 16.0));
        assert!(close(focused.x - tile_rect(0).right(), 16.0));
        let below = tile_rect(1 + COLUMNS);
        assert!(close(below.y - focused.bottom(), 73.6));
    }

    #[test]
    fn the_shadow_stays_inside_the_row_gap() {
        let gap = ROW_PITCH - TILE_H;
        assert!(close(shadow_reach(), 56.4));
        assert!(shadow_reach() < gap);
    }

    #[test]
    fn the_focused_title_stays_inside_the_row_gap() {
        let gap = ROW_PITCH - TILE_H;
        assert!(close(title_reach(), 58.4));
        assert!(title_reach() < gap);
    }

    #[test]
    fn an_icon_and_a_letter_fit_inside_a_resting_tile() {
        const { assert!(ICON_SIZE < TILE_H && LETTER_PX < TILE_H) };
    }

    #[test]
    fn lifted_edge_tiles_reach_only_into_the_side_margins() {
        let left = lifted(tile_rect(0), 1.0);
        let right = lifted(tile_rect(COLUMNS - 1), 1.0);
        assert!(close(SAFE.x - left.x, 24.0));
        assert!(close(right.right() - SAFE.right(), 24.0));
        assert!(left.x > 0.0 && right.right() < SCREEN_W);
    }

    #[test]
    fn row_one_scrolls_the_top_shelf_exactly_off_screen() {
        assert_eq!(scroll_for_row(0), 0.0);
        assert_eq!(scroll_for_row(1), TOP_SHELF.bottom());
        assert_eq!(row_y(0) - scroll_for_row(1), SAFE.y);
        for row in 1..6 {
            assert_eq!(scroll_for_row(row), 360.0 + 280.0 * row as f32);
            assert_eq!(row_y(row) - scroll_for_row(row), FOCUS_ROW_Y);
        }
    }

    #[test]
    fn tiles_off_screen_are_culled() {
        let resting = |row: usize, scroll: f32| {
            let r = tile_rect(row * COLUMNS).offset(0.0, -scroll);
            tile_on_screen(r, 0.0)
        };
        // Scrolled to the top: rows 0 and 1 show, row 2 starts below the edge.
        assert!(resting(0, 0.0) && resting(1, 0.0));
        assert!(!resting(2, 0.0));
        // Focus on row 3: row 1 ends just above the screen, rows 2 to 5 show.
        let scroll = scroll_for_row(3);
        assert!(!resting(1, scroll));
        assert!((2..=5).all(|r| resting(r, scroll)));
        assert!(!resting(6, scroll));
    }

    #[test]
    fn a_lifted_tiles_shadow_keeps_it_on_screen() {
        let above = Rect::new(80.0, -TILE_H - 10.0, TILE_W, TILE_H);
        assert!(!tile_on_screen(above, 0.0));
        assert!(tile_on_screen(above, 1.0), "its shadow reaches down");
        let below = Rect::new(80.0, SCREEN_H + 10.0, TILE_W, TILE_H);
        assert!(tile_on_screen(below, 1.0), "its shadow spreads up");
        assert!(!tile_on_screen(below.offset(0.0, 30.0), 1.0));
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.scaled(2.0), Rect::new(-40.0, -5.0, 200.0, 100.0));
        assert_eq!(r.offset(5.0, -5.0), Rect::new(15.0, 15.0, 100.0, 50.0));
        assert_eq!(r.outset(2.0), Rect::new(8.0, 18.0, 104.0, 54.0));
        assert_eq!((r.right(), r.bottom()), (110.0, 70.0));
    }
}
