//! Every look value in one place: colours, gradients, type sizes, radii, the
//! shadow, the focus scale, spacing and the springs' speeds
//! (docs/specs/look-and-feel.md, "Tokens"). Phase 4 tunes the look here, and
//! the reference screenshots change with it.
//!
//! Geometry that follows from these (where a tile sits, how far a scroll goes)
//! stays in `layout`; `layout` and `home` keep their old names for these
//! values as aliases.

use std::time::Duration;

use crate::anim::Tuning;

/// A colour, each channel 0 to 1, alpha straight (not premultiplied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Opaque, from 8-bit channels.
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }
}

/// A vertical gradient, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub top: Color,
    pub bottom: Color,
}

pub const WHITE: Color = Color::rgb8(255, 255, 255);
pub const BLACK: Color = Color::rgb8(0, 0, 0);

// Colours.

/// The screen behind everything; fixed, it doesn't scroll.
pub const BACKGROUND: Gradient = Gradient {
    top: Color::rgb8(14, 18, 34),
    bottom: BLACK,
};
/// The placeholder Top Shelf, until it has art.
pub const SHELF: Gradient = Gradient {
    top: Color::rgb8(46, 56, 98),
    bottom: Color::rgb8(14, 18, 34),
};
pub const TEXT: Color = Color::rgb8(235, 238, 245);
pub const TEXT_DIM: Color = Color::rgb8(140, 148, 165);
/// The status pill; also the flat fallback for glass.
pub const PILL: Color = BLACK.with_alpha(0.75);
/// An icon's tint (none) and a letter tile's letter.
pub const ON_TILE: Color = WHITE;

/// Resting tiles are this bright; lifting brings a tile to full brightness.
pub const TILE_REST_SHADE: f32 = 0.8;
/// How much lighter a tile's top edge is than its bottom: this far toward
/// white.
pub const TILE_SHEEN: f32 = 0.14;
/// Tile colours for apps that bring none, by position in the launcher's list:
/// with rows of 5 and 8 colours, no tile shares a colour with the tile beside,
/// above or below it.
pub const TILE_PALETTE: [Color; 8] = [
    Color::rgb8(196, 58, 58),
    Color::rgb8(214, 120, 46),
    Color::rgb8(190, 152, 44),
    Color::rgb8(64, 156, 92),
    Color::rgb8(44, 140, 160),
    Color::rgb8(58, 104, 196),
    Color::rgb8(112, 82, 190),
    Color::rgb8(170, 70, 140),
];

// Type: Inter SemiBold, in pixels to the em.

/// The focused tile's title.
pub const TITLE_PX: f32 = 30.0;
pub const SHELF_TITLE_PX: f32 = 72.0;
/// "TOP SHELF", above the shelf's title.
pub const SHELF_CAPTION_PX: f32 = 24.0;
pub const STATUS_PX: f32 = 28.0;
/// "No apps".
pub const EMPTY_PX: f32 = 48.0;
/// The first letter of the title, on a tile with no icon, at rest.
pub const LETTER_PX: f32 = 96.0;

// Radii. The status pill is fully round: its radius is half its height.

/// A tile's corners at rest; they grow with the tile.
pub const TILE_RADIUS: f32 = 12.0;

// Focus: the lifted tile and its shadow.

/// How much a fully lifted tile grows, about its centre.
pub const FOCUS_SCALE: f32 = 1.15;
/// The soft shadow under a lifted tile, offset down and fading out over
/// `SHADOW_BLUR` pixels either side of the tile's edge.
pub const SHADOW_BLUR: f32 = 24.0;
pub const SHADOW_OFFSET: f32 = 18.0;
pub const SHADOW_OPACITY: f32 = 0.4;
pub const SHADOW: Color = BLACK;

// Spacing, in pixels.

/// tvOS's guide: this far in from the left and right edges.
pub const SAFE_X: f32 = 80.0;
/// And from the top and bottom.
pub const SAFE_Y: f32 = 60.0;
pub const COLUMN_GAP: f32 = 40.0;
/// Between a row's tiles and the next row's, at rest.
pub const ROW_GAP: f32 = 88.0;
/// From a lifted tile's lower edge to the top of the capitals of its title.
pub const TITLE_GAP: f32 = 14.0;
/// From the Top Shelf title's baseline to the shelf's lower edge.
pub const SHELF_TITLE_GAP: f32 = 60.0;
/// Inside the status pill, around its text: left and right, top and bottom.
pub const PILL_PAD_X: f32 = 28.0;
pub const PILL_PAD_Y: f32 = 18.0;

// Motion: critically damped springs (docs/specs/shell-mvp.md, "Motion").
// `omega` sets the speed; `epsilon` and `rest_speed` say when one has
// settled.

/// Each tile's lift, 0 resting to 1 focused.
pub const LIFT: Tuning = Tuning {
    omega: 30.0,
    epsilon: 0.001,
    rest_speed: 0.01,
};
/// The grid's scroll, in px.
pub const SCROLL: Tuning = Tuning {
    omega: 20.0,
    epsilon: 0.25,
    rest_speed: 2.0,
};
/// The nudge of a blocked move, in px.
pub const BUMP: Tuning = Tuning {
    omega: 20.0,
    epsilon: 0.25,
    rest_speed: 2.0,
};
/// How far a blocked move nudges the focused tile, in px.
pub const BUMP_PEAK: f32 = 14.0;
/// How far Select dips the focused tile's lift.
pub const PRESS_DIP: f32 = 0.5;

// The Top Shelf hero (docs/specs/look-and-feel.md, "Phase 4 features").

/// A colour whose channels spread less than this (0 to 1) has no hue to
/// pick a palette hero by, so its app takes the ambient hero, as black,
/// white and greys do.
pub const HERO_MIN_CHROMA: f32 = 0.12;
/// How long focus rests, the screen settled, before the hero for the tile
/// the shelf shows loads: a run along row 0 loads nothing for the tiles
/// passed over.
pub const HERO_REST: Duration = Duration::from_millis(200);
/// The crossfade to a new hero, 0 to 1.
pub const HERO_FADE: Tuning = Tuning {
    omega: 12.0,
    epsilon: 0.002,
    rest_speed: 0.02,
};

// Focus light and tilt (docs/specs/look-and-feel.md, "Phase 4 features").

/// How far a newly focused tile leans toward the move, at most, in degrees:
/// the edge it moved toward goes back, and it springs upright.
pub const TILT_DEGREES: f32 = 5.0;
/// The lean's spring, in radians.
pub const TILT: Tuning = Tuning {
    omega: 14.0,
    epsilon: 0.0005,
    rest_speed: 0.005,
};
/// How far the eye is from the screen for the lean's perspective, in px: a
/// 5 degree lean of the lifted tile makes its far edge about 3% shorter.
pub const TILT_DISTANCE: f32 = 1200.0;
/// How long the light takes to sweep once across a newly focused tile.
pub const SWEEP_TIME: Duration = Duration::from_millis(450);
/// The light's band, as a fraction of the tile it crosses.
pub const SWEEP_WIDTH: f32 = 0.45;
/// The light at its brightest: white at this alpha.
pub const SWEEP_STRENGTH: f32 = 0.2;
/// How far the ambient hero's layers drift against a move at most, in px:
/// back (the sky) least, front (the ridge) most, so the shelf reads as
/// depth. Each is drawn 1.9% larger than the shelf, so a drift never shows
/// an edge.
pub const PARALLAX_PX: [f32; 3] = [1.0, 3.0, 6.0];
/// The drift's spring, 1 at its farthest: it moves and settles with the
/// tile's lean.
pub const PARALLAX: Tuning = Tuning {
    omega: TILT.omega,
    epsilon: 0.005,
    rest_speed: 0.05,
};

// Dither.

/// The ordered dither on gradients and shapes, peak to peak, as a fraction
/// of full scale: one 8-bit step, so half a step either way, enough to break
/// a band's edge and too little to see as grain.
pub const DITHER: f32 = 1.0 / 255.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_come_from_bytes() {
        assert_eq!(
            Color::rgb8(255, 0, 51),
            Color {
                r: 1.0,
                g: 0.0,
                b: 0.2,
                a: 1.0
            }
        );
        assert_eq!(PILL.a, 0.75);
        assert_eq!(PILL.with_alpha(1.0), BLACK);
    }

    #[test]
    fn the_shelf_fades_into_the_background() {
        assert_eq!(SHELF.bottom, BACKGROUND.top);
    }
}
