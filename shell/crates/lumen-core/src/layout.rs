//! Where everything sits, in the fixed 1920x1080 space the UI is designed for
//! (docs/specs/shell-mvp.md, "Layout"). "Content" coordinates scroll with the
//! grid; screen coordinates do not.

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

/// tvOS's guide: 80 px in from the sides, 60 px from the top and bottom.
pub const SAFE: Rect = Rect::new(80.0, 60.0, 1760.0, 960.0);

pub const COLUMNS: usize = 5;
/// 5:3, the shape of a tvOS app icon.
pub const TILE_W: f32 = 320.0;
pub const TILE_H: f32 = 192.0;
pub const TILE_RADIUS: f32 = 12.0;
pub const COLUMN_GAP: f32 = 40.0;
pub const COLUMN_PITCH: f32 = TILE_W + COLUMN_GAP;

/// The placeholder Top Shelf band, in content coordinates.
pub const TOP_SHELF: Rect = Rect::new(0.0, 0.0, SCREEN_W, 640.0);
pub const ROW0_Y: f32 = 700.0;
pub const ROW_PITCH: f32 = 280.0;
/// Screen y where a focused row below row 0 sits.
pub const FOCUS_ROW_Y: f32 = 340.0;
/// How much a fully lifted tile grows, about its centre.
pub const FOCUS_SCALE: f32 = 1.15;

/// The soft shadow under a lifted tile: layers stepping out, offset down.
pub const SHADOW_LAYERS: usize = 4;
pub const SHADOW_STEP: f32 = 6.0;
pub const SHADOW_OFFSET: f32 = 18.0;
pub const SHADOW_OPACITY: f32 = 0.4;

/// Text sizes, in screen pixels per 5x7-font pixel, and spacing.
pub const TITLE_SCALE: f32 = 3.0;
/// Space kept clear between a title and the tile's sides.
pub const TITLE_PAD: f32 = 16.0;
pub const SHELF_TITLE_SCALE: f32 = 8.0;
/// Gap from the bottom of the Top Shelf title to the shelf's lower edge.
pub const SHELF_TITLE_GAP: f32 = 60.0;
pub const STATUS_SCALE: f32 = 3.0;

/// Room a row needs beyond its resting tiles: the lift, its shadow, a bump.
const ROW_MARGIN: f32 = 80.0;

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

/// Whether any part of `row`, lifted, shadowed or bumped, is on screen.
pub fn row_on_screen(row: usize, scroll: f32) -> bool {
    let top = row_y(row) - scroll - ROW_MARGIN;
    let bottom = row_y(row) + TILE_H - scroll + ROW_MARGIN;
    bottom > 0.0 && top < SCREEN_H
}

/// How far below a tile's resting edge a fully lifted tile's shadow reaches.
pub fn shadow_reach() -> f32 {
    let growth = TILE_H * (FOCUS_SCALE - 1.0) / 2.0;
    growth + SHADOW_OFFSET + SHADOW_LAYERS as f32 * SHADOW_STEP
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
    fn rows_off_screen_are_culled() {
        // Scrolled to the top: rows 0 and 1 show, row 2 starts below the edge.
        assert!(row_on_screen(0, 0.0));
        assert!(row_on_screen(1, 0.0));
        assert!(!row_on_screen(2, 0.0));
        // Focus on row 3: row 0 is far above, rows 2 to 5 show.
        let scroll = scroll_for_row(3);
        assert!(!row_on_screen(0, scroll));
        assert!((2..=5).all(|r| row_on_screen(r, scroll)));
        assert!(!row_on_screen(6, scroll));
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
