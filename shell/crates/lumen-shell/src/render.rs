//! A batched quad renderer: every frame is a list of quads in a fixed
//! 1920x1080 space, uploaded once and drawn with one call. On the B9 one such
//! call of about 3,000 flat quads held 60 fps (hello-gles; OLED55B9PUA, webOS
//! 4.10.2-31, firmware 05.50.00).
//!
//! A quad is one of three kinds, all drawn by one shader:
//! * flat: a colour, or a gradient from top to bottom;
//! * shape: a rounded rectangle the fragment shader cuts out, with a one-pixel
//!   soft edge, or a wide one for a shadow;
//! * image: a region of a texture, tinted: the atlas (icons and glyphs), or
//!   a texture of its own (a Top Shelf hero's layer).
//!
//! Colours blend premultiplied, which keeps the edges of scaled icons and
//! glyphs free of dark fringes.
//!
//! The batch is drawn in runs, one for each change of texture: the art's
//! quads come first, under everything, so a frame has one run per hero layer
//! on screen and one for the atlas. The shader keeps its one sampler, and the
//! vertices go to GL in one upload.
//!
//! The art behind everything covers every pixel of the screen, so it is
//! drawn by a program of its own (`Batch::art`): a texture times a colour,
//! with two varyings and no branch, where the main program would carry its
//! shape, kind and dither for every pixel. The main program draws the rest.

use std::ffi::c_void;

use lumen_core::layout::Rect;
use lumen_core::style::{self, Color};

use crate::atlas::{ATLAS_H, ATLAS_W, Upload, Uv};
use crate::gl::{self, Gl};

/// The surface the UI is designed for. The TV's graphics plane is this size;
/// on a PC it is letterboxed into the window.
pub use lumen_core::layout::{SCREEN_H, SCREEN_W};

/// x, y; u, v; r, g, b, a; the pixel's offset from the shape's centre; the
/// shape's half width, half height, corner radius and edge softness; kind;
/// w, the perspective divide, 1 unless the quad leans (`Tilt`).
const FLOATS_PER_VERTEX: usize = 16;
const VERTICES_PER_QUAD: usize = 6;

const FLAT: f32 = 0.0;
const SHAPE: f32 = 1.0;
const IMAGE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    }

    /// A colour from `lumen_core::style`.
    pub const fn of(c: Color) -> Self {
        Self(c.r, c.g, c.b, c.a)
    }

    pub const fn alpha(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, a)
    }

    /// From `self` (t = 0) to `to` (t = 1), alpha included.
    pub fn lerp(self, to: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self(
            mix(self.0, to.0),
            mix(self.1, to.1),
            mix(self.2, to.2),
            mix(self.3, to.3),
        )
    }

    /// The colour times `k` (below 1 darker), alpha kept.
    pub fn shade(self, k: f32) -> Self {
        Self(self.0 * k, self.1 * k, self.2 * k, self.3)
    }
}

/// What the fragment shader cuts out of a shape quad: a rounded rectangle
/// centred at (cx, cy), and how soft its edge is.
#[derive(Debug, Clone, Copy)]
struct Shape {
    cx: f32,
    cy: f32,
    half_w: f32,
    half_h: f32,
    radius: f32,
    soft: f32,
}

const NO_SHAPE: Shape = Shape {
    cx: 0.0,
    cy: 0.0,
    half_w: 0.0,
    half_h: 0.0,
    radius: 0.0,
    soft: 0.0,
};

const NO_UV: Uv = Uv {
    u0: 0.0,
    v0: 0.0,
    u1: 0.0,
    v1: 0.0,
};

/// The texture an image quad samples: the atlas, or a texture of its own by
/// the GL name `Renderer::create_texture` gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Texture {
    Atlas,
    Own(u32),
}

/// A lean in perspective, which `Batch::set_tilt` projects the quads pushed
/// after it by: their plane turns about its upright axis through (cx, cy) by
/// `angles[0]` radians (positive sends its right side back) and about its
/// level one by `angles[1]` (positive sends its lower side back), seen from
/// `distance` px in front of the screen. Each vertex is drawn where it
/// projects, with the w that has the GPU interpolate its texture place and
/// shape offset in perspective, so a leaning tile's corners, icon and
/// letter stay true; two triangles of one flat quad agree exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    cx: f32,
    cy: f32,
    sin: [f32; 2],
    cos: [f32; 2],
    distance: f32,
}

impl Tilt {
    pub fn new(cx: f32, cy: f32, angles: [f32; 2], distance: f32) -> Self {
        Self {
            cx,
            cy,
            sin: angles.map(f32::sin),
            cos: angles.map(f32::cos),
            distance,
        }
    }

    /// Where (x, y) on the leaning plane shows on the screen, and its w.
    fn project(&self, x: f32, y: f32) -> (f32, f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let z = dx * self.sin[0] + dy * self.sin[1];
        let w = (self.distance + z) / self.distance;
        (
            self.cx + dx * self.cos[0] / w,
            self.cy + dy * self.cos[1] / w,
            w,
        )
    }
}

/// Vertices drawn with one texture bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub texture: Texture,
    pub first: usize,
    pub count: usize,
    /// Whether any of its quads samples the texture; flat and shape quads
    /// draw the same with any texture bound, so they join any run but the
    /// art's.
    sampled: bool,
    /// Drawn by the art's program (`Batch::art`).
    pub art: bool,
}

/// A quad's corners, and what they carry besides position.
struct Quad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: Uv,
    /// Top left, top right, bottom left, bottom right.
    colors: [Rgba; 4],
    shape: Shape,
    kind: f32,
    /// What an image quad samples; `None` for flat and shape quads.
    texture: Option<Texture>,
    /// Drawn by the art's program.
    art: bool,
}

#[derive(Default)]
pub struct Batch {
    verts: Vec<f32>,
    runs: Vec<Run>,
    /// The lean the quads pushed now take, if any.
    tilt: Option<Tilt>,
}

impl Batch {
    pub fn clear(&mut self) {
        self.verts.clear();
        self.runs.clear();
        self.tilt = None;
    }

    /// Leans the quads pushed from now on, until set to `None`.
    pub fn set_tilt(&mut self, tilt: Option<Tilt>) {
        self.tilt = tilt;
    }

    /// The runs to draw, in order: each a texture and the vertices drawn
    /// with it bound.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// Counts a quad's six vertices into a run: the last one if it can take
    /// them, else a new one. `texture` is what the quad samples, `None` for
    /// a flat or shape quad; `art`, whether the art's program draws it.
    fn add_to_run(&mut self, texture: Option<Texture>, art: bool) {
        let first = self.vertex_count() - VERTICES_PER_QUAD;
        match (self.runs.last_mut(), texture) {
            (Some(run), _) if run.art != art => self.runs.push(Run {
                texture: texture.unwrap_or(Texture::Atlas),
                first,
                count: VERTICES_PER_QUAD,
                sampled: texture.is_some(),
                art,
            }),
            (Some(run), None) => run.count += VERTICES_PER_QUAD,
            (Some(run), Some(t)) if run.texture == t || !run.sampled => {
                run.texture = t;
                run.sampled = true;
                run.count += VERTICES_PER_QUAD;
            }
            _ => self.runs.push(Run {
                texture: texture.unwrap_or(Texture::Atlas),
                first,
                count: VERTICES_PER_QUAD,
                sampled: texture.is_some(),
                art,
            }),
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.verts.len() / FLOATS_PER_VERTEX
    }

    pub fn quad_count(&self) -> usize {
        self.vertex_count() / VERTICES_PER_QUAD
    }

    /// The smallest box holding every vertex: left, top, right, bottom.
    #[cfg(test)]
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let xs = self.verts.iter().step_by(FLOATS_PER_VERTEX);
        let ys = self.verts.iter().skip(1).step_by(FLOATS_PER_VERTEX);
        (
            xs.clone().copied().fold(f32::MAX, f32::min),
            ys.clone().copied().fold(f32::MAX, f32::min),
            xs.copied().fold(f32::MIN, f32::max),
            ys.copied().fold(f32::MIN, f32::max),
        )
    }

    /// How many quads lean: their w is not 1.
    #[cfg(test)]
    pub fn leaning_quads(&self) -> usize {
        self.verts
            .chunks(FLOATS_PER_VERTEX * VERTICES_PER_QUAD)
            .filter(|quad| quad.chunks(FLOATS_PER_VERTEX).any(|v| v[15] != 1.0))
            .count()
    }

    /// Quad `n`'s box: left, top, right, bottom.
    #[cfg(test)]
    pub fn quad_bounds(&self, n: usize) -> (f32, f32, f32, f32) {
        let quad = Self {
            verts: self
                .verts
                .chunks(FLOATS_PER_VERTEX * VERTICES_PER_QUAD)
                .nth(n)
                .expect("quad n")
                .to_vec(),
            runs: Vec::new(),
            tilt: None,
        };
        quad.bounds()
    }

    /// Each image quad's atlas region: u0, v0, u1, v1.
    #[cfg(test)]
    pub fn image_uvs(&self) -> Vec<(f32, f32, f32, f32)> {
        self.verts
            .chunks(FLOATS_PER_VERTEX * VERTICES_PER_QUAD)
            .filter(|quad| quad[14] == IMAGE)
            .map(|quad| {
                let us = quad.iter().skip(2).step_by(FLOATS_PER_VERTEX);
                let vs = quad.iter().skip(3).step_by(FLOATS_PER_VERTEX);
                (
                    us.clone().copied().fold(f32::MAX, f32::min),
                    vs.clone().copied().fold(f32::MAX, f32::min),
                    us.copied().fold(f32::MIN, f32::max),
                    vs.copied().fold(f32::MIN, f32::max),
                )
            })
            .collect()
    }

    fn push(&mut self, q: &Quad) {
        let (x1, y1) = (q.x + q.w, q.y + q.h);
        let s = q.shape;
        let corners = [
            (q.x, q.y, q.uv.u0, q.uv.v0, q.colors[0]),
            (x1, q.y, q.uv.u1, q.uv.v0, q.colors[1]),
            (q.x, y1, q.uv.u0, q.uv.v1, q.colors[2]),
            (x1, y1, q.uv.u1, q.uv.v1, q.colors[3]),
        ];
        for i in [0, 1, 2, 1, 3, 2] {
            let (x, y, u, v, c) = corners[i];
            let (sx, sy, w) = self.tilt.map_or((x, y, 1.0), |t| t.project(x, y));
            self.verts.extend_from_slice(&[
                sx,
                sy,
                u,
                v,
                c.0,
                c.1,
                c.2,
                c.3,
                x - s.cx,
                y - s.cy,
                s.half_w,
                s.half_h,
                s.radius,
                s.soft,
                q.kind,
                w,
            ]);
        }
        self.add_to_run(q.texture, q.art);
    }

    /// A rectangle shaded from `top` to `bottom`.
    pub fn gradient_rect(&mut self, x: f32, y: f32, w: f32, h: f32, top: Rgba, bottom: Rgba) {
        self.push(&Quad {
            x,
            y,
            w,
            h,
            uv: NO_UV,
            colors: [top, top, bottom, bottom],
            shape: NO_SHAPE,
            kind: FLAT,
            texture: None,
            art: false,
        });
    }

    /// A rounded rectangle like `rounded_rect`, each corner its own colour:
    /// top left, top right, bottom left, bottom right, blended across it.
    /// The focused tile's sheen is its top left corner, at no cost.
    // The rectangle, its radius and the four colours.
    #[allow(clippy::too_many_arguments)]
    pub fn rounded_rect4(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, colors: [Rgba; 4]) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let margin = 1.0;
        // The colours carried out to the grown edges, so the blend stays
        // where it was.
        let at = |u: f32, v: f32| {
            let top = colors[0].lerp(colors[1], u);
            let bottom = colors[2].lerp(colors[3], u);
            top.lerp(bottom, v)
        };
        let (u0, u1) = (-margin / w, 1.0 + margin / w);
        let (v0, v1) = (-margin / h, 1.0 + margin / h);
        self.push(&Quad {
            x: x - margin,
            y: y - margin,
            w: w + 2.0 * margin,
            h: h + 2.0 * margin,
            uv: NO_UV,
            colors: [at(u0, v0), at(u1, v0), at(u0, v1), at(u1, v1)],
            shape: Shape {
                cx: x + w / 2.0,
                cy: y + h / 2.0,
                half_w: w / 2.0,
                half_h: h / 2.0,
                radius: r,
                soft: 0.5,
            },
            kind: SHAPE,
            texture: None,
            art: false,
        });
    }

    /// A rectangle with corners of radius `r`, shaded from `top` to `bottom`,
    /// cut out by the shader: one quad, a pixel bigger on every side for the
    /// soft edge.
    // The same x, y, w, h as `gradient_rect`, plus a radius and two colours.
    #[allow(clippy::too_many_arguments)]
    pub fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top: Rgba,
        bottom: Rgba,
    ) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        self.shape(x, y, w, h, r, 0.5, (top, bottom), 1.0);
    }

    /// A soft shadow the shape of a rounded rectangle: solid inside, fading
    /// to nothing over `blur` pixels either side of its edge.
    // The rectangle, its radius, the blur and one colour.
    #[allow(clippy::too_many_arguments)]
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, c: Rgba) {
        if w <= 0.0 || h <= 0.0 || c.3 <= 0.0 {
            return;
        }
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let blur = blur.max(0.5);
        self.shape(x, y, w, h, r, blur, (c, c), blur + 0.5);
    }

    /// A shape quad grown by `margin` on every side, its colours carried out
    /// to the grown edges so the gradient stays where it was.
    // Private; the public callers name every argument.
    #[allow(clippy::too_many_arguments)]
    fn shape(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        soft: f32,
        (top, bottom): (Rgba, Rgba),
        margin: f32,
    ) {
        let (top, bottom) = (
            top.lerp(bottom, -margin / h),
            top.lerp(bottom, 1.0 + margin / h),
        );
        self.push(&Quad {
            x: x - margin,
            y: y - margin,
            w: w + 2.0 * margin,
            h: h + 2.0 * margin,
            uv: NO_UV,
            colors: [top, top, bottom, bottom],
            shape: Shape {
                cx: x + w / 2.0,
                cy: y + h / 2.0,
                half_w: w / 2.0,
                half_h: h / 2.0,
                radius: r,
                soft,
            },
            kind: SHAPE,
            texture: None,
            art: false,
        });
    }

    /// The `part` of the rounded rectangle `rect` (radius `r`) that lies
    /// inside it, cut by its corners, each corner of the part its own colour:
    /// top left, top right, bottom left, bottom right. The light crossing a
    /// tile is two of these.
    pub fn rounded_part(&mut self, rect: Rect, r: f32, part: Rect, colors: [Rgba; 4]) {
        if part.w <= 0.0 || part.h <= 0.0 {
            return;
        }
        let r = r.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
        self.push(&Quad {
            x: part.x,
            y: part.y,
            w: part.w,
            h: part.h,
            uv: NO_UV,
            colors,
            shape: Shape {
                cx: rect.x + rect.w / 2.0,
                cy: rect.y + rect.h / 2.0,
                half_w: rect.w / 2.0,
                half_h: rect.h / 2.0,
                radius: r,
                soft: 0.5,
            },
            kind: SHAPE,
            texture: None,
            art: false,
        });
    }

    /// The atlas region `uv` drawn into the rectangle, its colour times `tint`.
    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, uv: Uv, tint: Rgba) {
        self.image_from(Texture::Atlas, x, y, w, h, uv, tint);
    }

    /// The region `uv` of `texture` drawn into the rectangle, its colour
    /// times `tint`.
    // `image`'s arguments, and the texture.
    #[allow(clippy::too_many_arguments)]
    pub fn image_from(
        &mut self,
        texture: Texture,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: Uv,
        tint: Rgba,
    ) {
        self.image_quad(texture, x, y, w, h, uv, tint, false);
    }

    /// Art behind everything: like `image_from`, drawn by the art's
    /// program, in a run of its own.
    // `image_from`'s arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn art(&mut self, texture: Texture, x: f32, y: f32, w: f32, h: f32, uv: Uv, tint: Rgba) {
        self.image_quad(texture, x, y, w, h, uv, tint, true);
    }

    // `image_from`'s arguments, and whether it is art.
    #[allow(clippy::too_many_arguments)]
    fn image_quad(
        &mut self,
        texture: Texture,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: Uv,
        tint: Rgba,
        art: bool,
    ) {
        if w <= 0.0 || h <= 0.0 || tint.3 <= 0.0 {
            return;
        }
        self.push(&Quad {
            x,
            y,
            w,
            h,
            uv,
            colors: [tint; 4],
            shape: NO_SHAPE,
            kind: IMAGE,
            texture: Some(texture),
            art,
        });
    }
}

/// The largest 16:9 rectangle centred in a `w` x `h` drawable, as GL viewport
/// coordinates (origin bottom-left).
pub fn fit_viewport(w: i32, h: i32) -> (i32, i32, i32, i32) {
    let scale = (w as f32 / SCREEN_W).min(h as f32 / SCREEN_H);
    let vw = (SCREEN_W * scale).round() as i32;
    let vh = (SCREEN_H * scale).round() as i32;
    ((w - vw) / 2, (h - vh) / 2, vw, vh)
}

// No #version line: GLSL ES 1.00 and desktop GLSL 1.10 both accept these, and
// the precision statements only exist where GL_ES is defined.
const VERTEX_SHADER: &str = "
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec4 a_color;
attribute vec2 a_local;
attribute vec4 a_shape;
attribute float a_kind;
attribute float a_w;
uniform vec2 u_screen;
#ifdef DITHER
varying vec3 v_bayer;
#endif
varying vec2 v_uv;
varying vec4 v_color;
varying vec2 v_local;
varying vec4 v_shape;
varying float v_kind;
void main() {
    vec2 ndc = vec2(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0);
    gl_Position = vec4(ndc * a_w, 0.0, a_w);
#ifdef DITHER
    v_bayer = vec3(a_pos * (0.25 * a_w), a_w);
#endif
    v_uv = a_uv;
    v_color = a_color;
    v_local = a_local;
    v_shape = a_shape;
    v_kind = a_kind;
}
";

// Highp where the GPU has it: mediump is 16-bit on Mali, too coarse for
// distances across a 300 px pill or texels in a 2048 px atlas.
//
// Flat and shape quads are dithered when `u_dither` is above 0: a 4x4 Bayer
// threshold from the pixel's place, centred, times `u_dither` and the
// pixel's alpha, so the 8-bit framebuffer's rounding breaks a gradient's
// bands into a pattern finer than the eye resolves. Images, the heroes and
// the atlas, are left as they are.
//
// The threshold is one fetch from `BAYER`, a 4x4 texture on unit 1 that
// repeats, nearest texel: no arithmetic but the fetch and one multiply-add.
// Worked out in the shader (two Bayer steps, two floors and a divide for
// every pixel), the dither put the B9's full look past its vsync
// (2026-11-08: 29.6 ms a frame against 17.1 without it). Its place is
// `v_bayer`, the design-space position in texels (a quarter of a pixel),
// which on the TV is the framebuffer's pixel: a highp varying, where GLSL
// ES declares `gl_FragCoord` mediump, whose 16-bit floats past 1024 hold
// only even pixel centres. It goes as (x w, y w, w) and `texture2DProj`
// divides it back, undoing the perspective interpolation a leaning quad
// gets, so the pattern stays on the screen's pixels there too. A pixel's
// centre lands half a texel from the texel's edges, so nearest filtering
// picks the same texel on any GPU.
//
// The dither is built in only where `DITHER` is defined (`Program::build`):
// a separate program, made when the dither is turned on. Without it the
// shaders are the ones from before the dither, with no `v_pos` and no
// branch on `u_dither`. Built in always, that varying and that branch ran
// for every pixel even at `u_dither` 0, and they are the suspect for the
// plain look's 21.6 ms a frame on the B9's Mali-T820 MP2 against 17 ms
// before lane J (2026-11-07; look-and-feel.md, "Measured on the B9").
const FRAGMENT_SHADER: &str = "
#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif
uniform sampler2D u_atlas;
#ifdef DITHER
uniform sampler2D u_bayer;
uniform float u_dither;
varying vec3 v_bayer;
#endif
varying vec2 v_uv;
varying vec4 v_color;
varying vec2 v_local;
varying vec4 v_shape;
varying float v_kind;
void main() {
    vec4 c = vec4(v_color.rgb * v_color.a, v_color.a);
    if (v_kind > 1.5) {
        gl_FragColor = texture2D(u_atlas, v_uv) * c;
#ifdef DITHER
        return;
#endif
    } else if (v_kind > 0.5) {
        vec2 q = abs(v_local) - v_shape.xy + v_shape.z;
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_shape.z;
        gl_FragColor = c * (1.0 - smoothstep(-v_shape.w, v_shape.w, d));
    } else {
        gl_FragColor = c;
    }
#ifdef DITHER
    gl_FragColor.rgb += (texture2DProj(u_bayer, v_bayer).r - 0.5) * u_dither * gl_FragColor.a;
#endif
}
";

// The art's program: a texture times a colour, nothing else. Highp for the
// texture place where the GPU has it, as above: 16-bit floats step 0.6 of a
// texel across a 1280 px texture.
const ART_VERTEX_SHADER: &str = "
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec4 a_color;
attribute float a_w;
uniform vec2 u_screen;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    vec2 ndc = vec2(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0);
    gl_Position = vec4(ndc * a_w, 0.0, a_w);
    v_uv = a_uv;
    v_color = vec4(a_color.rgb * a_color.a, a_color.a);
}
";

const ART_FRAGMENT_SHADER: &str = "
#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif
uniform sampler2D u_atlas;
varying vec2 v_uv;
varying vec4 v_color;
void main() {
    gl_FragColor = texture2D(u_atlas, v_uv) * v_color;
}
";

/// The attributes the art's program reads.
const ART_ATTRIBUTES: [&std::ffi::CStr; 4] = [c"a_pos", c"a_uv", c"a_color", c"a_w"];

/// Each attribute: its name and how many floats it takes, in vertex order.
const ATTRIBUTES: [(&std::ffi::CStr, i32); 7] = [
    (c"a_pos", 2),
    (c"a_uv", 2),
    (c"a_color", 4),
    (c"a_local", 2),
    (c"a_shape", 4),
    (c"a_kind", 1),
    (c"a_w", 1),
];

/// One attribute of a program: where it is, and its floats' offset and
/// count in a vertex.
#[derive(Debug, Clone, Copy)]
struct Attribute {
    at: u32,
    offset: usize,
    floats: i32,
}

/// The dither's 4x4 Bayer thresholds, row by row: each of the 16 levels
/// once, as `level * 17`, so a fetch gives `level / 15`. The same pattern
/// the shader worked out before (`tests`).
const BAYER: [u8; 16] = {
    let levels = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    let mut bytes = [0; 16];
    let mut i = 0;
    while i < 16 {
        bytes[i] = levels[i] * 17;
        i += 1;
    }
    bytes
};
/// The texture unit `BAYER` is on; the atlas and the art use unit 0.
const BAYER_UNIT: i32 = 1;

/// `u_dither` for a dither of `peak` to peak: a fetch minus a half spans
/// 15/16 of the 16 levels' range, so the levels step as they did when
/// the shader worked out `level / 16 - 15 / 32`.
fn dither_uniform(peak: f32) -> f32 {
    peak * 15.0 / 16.0
}

/// A linked program and where its inputs are.
struct Program {
    id: u32,
    attributes: Vec<Attribute>,
    u_screen: i32,
    /// The dither's strength; only the dithered program has it.
    u_dither: Option<i32>,
}

/// Where each of `names` is in a linked program, with its place in a vertex.
fn attributes_of(gl: &Gl, id: u32, names: &[&std::ffi::CStr]) -> Result<Vec<Attribute>, String> {
    let mut offset = 0;
    let mut found = Vec::with_capacity(names.len());
    for (name, floats) in ATTRIBUTES {
        if names.contains(&name) {
            // SAFETY: the program linked and the name is a NUL-terminated literal.
            let at = unsafe { (gl.get_attrib_location)(id, name.as_ptr()) };
            let at = u32::try_from(at)
                .map_err(|_| format!("shader is missing {}", name.to_string_lossy()))?;
            found.push(Attribute { at, offset, floats });
        }
        offset += floats as usize;
    }
    Ok(found)
}

impl Program {
    /// The art's program (`ART_VERTEX_SHADER`).
    fn build_art(gl: &Gl) -> Result<Self, String> {
        let vs = gl.compile_shader(gl::VERTEX_SHADER, ART_VERTEX_SHADER)?;
        let fs = gl.compile_shader(gl::FRAGMENT_SHADER, ART_FRAGMENT_SHADER)?;
        let id = gl.link_program(vs, fs)?;
        let attributes = attributes_of(gl, id, &ART_ATTRIBUTES)?;
        // SAFETY: the program linked and the names are NUL-terminated literals.
        let (u_screen, u_atlas) = unsafe {
            (
                (gl.get_uniform_location)(id, c"u_screen".as_ptr()),
                (gl.get_uniform_location)(id, c"u_atlas".as_ptr()),
            )
        };
        if u_screen < 0 || u_atlas < 0 {
            return Err("the art's shader is missing u_screen or u_atlas".to_owned());
        }
        // SAFETY: the program linked above; its sampler reads texture unit 0.
        unsafe {
            (gl.use_program)(id);
            (gl.uniform1i)(u_atlas, 0);
        }
        Ok(Self {
            id,
            attributes,
            u_screen,
            u_dither: None,
        })
    }

    /// The shaders, with the dither built in if `dither`.
    fn build(gl: &Gl, dither: bool) -> Result<Self, String> {
        let define = if dither { "#define DITHER\n" } else { "" };
        let vs = gl.compile_shader(gl::VERTEX_SHADER, &format!("{define}{VERTEX_SHADER}"))?;
        let fs = gl.compile_shader(gl::FRAGMENT_SHADER, &format!("{define}{FRAGMENT_SHADER}"))?;
        let id = gl.link_program(vs, fs)?;
        let names = ATTRIBUTES.map(|(name, _)| name);
        let attributes = attributes_of(gl, id, &names)?;
        // SAFETY: as above.
        let (u_screen, u_atlas, u_dither, u_bayer) = unsafe {
            (
                (gl.get_uniform_location)(id, c"u_screen".as_ptr()),
                (gl.get_uniform_location)(id, c"u_atlas".as_ptr()),
                (gl.get_uniform_location)(id, c"u_dither".as_ptr()),
                (gl.get_uniform_location)(id, c"u_bayer".as_ptr()),
            )
        };
        if u_screen < 0 || u_atlas < 0 || (dither && (u_dither < 0 || u_bayer < 0)) {
            return Err("shader is missing u_screen, u_atlas, u_dither or u_bayer".to_owned());
        }
        // SAFETY: the program linked above; the atlas is texture unit 0 and
        // the Bayer texture unit 1.
        unsafe {
            (gl.use_program)(id);
            (gl.uniform1i)(u_atlas, 0);
            if dither {
                (gl.uniform1i)(u_bayer, BAYER_UNIT);
            }
        }
        Ok(Self {
            id,
            attributes,
            u_screen,
            u_dither: dither.then_some(u_dither),
        })
    }
}

pub struct Renderer {
    gl: Gl,
    /// The program without the dither and the one with it, each built when
    /// first needed.
    plain: Option<Program>,
    dithered: Option<Program>,
    /// The art's program; without it (the GPU refused it) the main program
    /// draws the art.
    art: Option<Program>,
    vbo: u32,
    texture: u32,
    /// The dither's thresholds (`BAYER`), on unit 1 once made; 0 before.
    bayer: u32,
    /// The dither's peak to peak, 0 for none.
    dither: f32,
}

impl Renderer {
    /// The renderer, with the dither on if `dither`. Only the program that
    /// takes is built; a dithered one the GPU refuses is logged, and the
    /// renderer goes on without the dither.
    pub fn new(gl: Gl, dither: bool) -> Result<Self, String> {
        let (mut vbo, mut texture) = (0, 0);
        // SAFETY: `vbo` and `texture` are valid out-pointers for one name each;
        // the texture is allocated with no data (null), which GL allows.
        unsafe {
            (gl.gen_buffers)(1, &mut vbo);
            (gl.gen_textures)(1, &mut texture);
            (gl.active_texture)(gl::TEXTURE0);
            (gl.bind_texture)(gl::TEXTURE_2D, texture);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE);
            (gl.tex_image_2d)(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                ATLAS_W as i32,
                ATLAS_H as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            (gl.enable)(gl::BLEND);
            (gl.blend_func)(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);
        }
        let mut renderer = Self {
            gl,
            plain: None,
            dithered: None,
            art: None,
            vbo,
            texture,
            bayer: 0,
            dither: 0.0,
        };
        if dither {
            renderer.set_dither(true);
        }
        if renderer.program().is_none() {
            renderer.plain = Some(Program::build(&renderer.gl, false)?);
        }
        match Program::build_art(&renderer.gl) {
            Ok(p) => renderer.art = Some(p),
            Err(e) => crate::info!("the art's program: {e}; the main one draws the art"),
        }
        Ok(renderer)
    }

    pub fn gl(&self) -> &Gl {
        &self.gl
    }

    /// Dithers flat and shape quads by `style::DITHER` if `on`, building
    /// the program for it if it isn't yet. A dithered program the GPU
    /// refuses is logged, and the dither stays off.
    pub fn set_dither(&mut self, on: bool) {
        if on && self.bayer == 0 {
            match self.make_bayer() {
                Ok(name) => self.bayer = name,
                Err(e) => crate::info!("dither: {e}; drawn without it"),
            }
        }
        if on && self.bayer != 0 && self.dithered.is_none() {
            match Program::build(&self.gl, true) {
                Ok(p) => self.dithered = Some(p),
                Err(e) => crate::info!("dither: {e}; drawn without it"),
            }
        }
        self.dither = if on && self.dithered.is_some() {
            style::DITHER
        } else {
            0.0
        };
        if self.dither == 0.0 && self.plain.is_none() {
            match Program::build(&self.gl, false) {
                Ok(p) => self.plain = Some(p),
                Err(e) => crate::info!("the plain program: {e}"),
            }
        }
    }

    /// Makes the dither's texture: `BAYER`, 4x4 bytes of luminance on unit
    /// 1, repeating, nearest texel. Unit 0 is left active. GL's error if it
    /// would not take it (it is deleted then).
    fn make_bayer(&self) -> Result<u32, String> {
        let gl = &self.gl;
        let before = gl.error();
        if before != gl::NO_ERROR {
            crate::info!("GL error {before:#x}");
        }
        let mut name = 0;
        // SAFETY: `name` is a valid out-pointer for one name; `BAYER` holds
        // the 4 x 4 one-byte pixels GL reads at an alignment of 1, and is a
        // constant, so it outlives the call.
        unsafe {
            (gl.gen_textures)(1, &mut name);
            (gl.active_texture)(gl::TEXTURE0 + BAYER_UNIT as u32);
            (gl.bind_texture)(gl::TEXTURE_2D, name);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT);
            (gl.pixel_storei)(gl::UNPACK_ALIGNMENT, 1);
            (gl.tex_image_2d)(
                gl::TEXTURE_2D,
                0,
                gl::LUMINANCE as i32,
                4,
                4,
                0,
                gl::LUMINANCE,
                gl::UNSIGNED_BYTE,
                BAYER.as_ptr().cast(),
            );
            (gl.active_texture)(gl::TEXTURE0);
        }
        match gl.error() {
            gl::NO_ERROR => Ok(name),
            e => {
                // SAFETY: `name` was made just above on this context.
                unsafe { (gl.delete_textures)(1, &name) };
                Err(format!("its texture: GL error {e:#x}"))
            }
        }
    }

    /// The program the next frame draws with.
    fn program(&self) -> Option<&Program> {
        if self.dither > 0.0 {
            self.dithered.as_ref()
        } else {
            self.plain.as_ref()
        }
    }

    /// Copies newly placed atlas pixels into the texture.
    pub fn upload(&self, uploads: &[Upload]) {
        let gl = &self.gl;
        for up in uploads {
            // GL reads w * h pixels from `rgba`: these hold in release too.
            assert!(up.x + up.w <= ATLAS_W && up.y + up.h <= ATLAS_H);
            assert_eq!(up.rgba.len(), (up.w * up.h * 4) as usize);
            // SAFETY: the texture was created by `new` on this context; the
            // asserts above keep the region inside it and make `rgba` hold
            // exactly w * h RGBA pixels, tightly packed, which the 4-byte
            // alignment matches.
            unsafe {
                (gl.bind_texture)(gl::TEXTURE_2D, self.texture);
                (gl.pixel_storei)(gl::UNPACK_ALIGNMENT, 4);
                (gl.tex_sub_image_2d)(
                    gl::TEXTURE_2D,
                    0,
                    up.x as i32,
                    up.y as i32,
                    up.w as i32,
                    up.h as i32,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    up.rgba.as_ptr().cast(),
                );
            }
        }
    }

    /// Clears the whole drawable to black, then draws `batch` letterboxed into it.
    pub fn draw(&self, batch: &Batch, drawable: (i32, i32)) {
        let gl = &self.gl;
        // `new` and `set_dither` leave a program for the mode they set.
        let Some(main) = self.program() else {
            return;
        };
        let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
        let bytes = (batch.verts.len() * size_of::<f32>()) as isize;
        // SAFETY: the buffer and texture were made by `new` on this
        // context; the upload reads exactly `bytes` from the batch, which
        // outlives the call.
        unsafe {
            (gl.viewport)(0, 0, drawable.0, drawable.1);
            (gl.clear_color)(0.0, 0.0, 0.0, 1.0);
            (gl.clear)(gl::COLOR_BUFFER_BIT);
            (gl.viewport)(x, y, w, h);
            (gl.active_texture)(gl::TEXTURE0);
            (gl.bind_buffer)(gl::ARRAY_BUFFER, self.vbo);
            (gl.buffer_data)(
                gl::ARRAY_BUFFER,
                bytes,
                batch.verts.as_ptr().cast(),
                gl::STREAM_DRAW,
            );
        }
        let mut current: Option<u32> = None;
        for run in batch.runs() {
            let program = match (&self.art, run.art) {
                (Some(art), true) => art,
                _ => main,
            };
            if current != Some(program.id) {
                current = Some(program.id);
                self.use_program(program);
            }
            let name = match run.texture {
                Texture::Atlas => self.texture,
                Texture::Own(name) => name,
            };
            // SAFETY: the texture is the atlas or one `create_texture` made,
            // and the run's vertices lie inside the buffer uploaded above.
            unsafe {
                (gl.bind_texture)(gl::TEXTURE_2D, name);
                (gl.draw_arrays)(gl::TRIANGLES, run.first as i32, run.count as i32);
            }
        }
    }

    /// Makes `program` current, with its uniforms and its attributes'
    /// places in the bound buffer.
    fn use_program(&self, program: &Program) {
        let gl = &self.gl;
        let stride = (FLOATS_PER_VERTEX * size_of::<f32>()) as i32;
        // SAFETY: the program and its attribute locations were made by
        // `Program::build` or `build_art` on this context, and the vertex
        // buffer is bound; every attribute's offset and size stay inside one
        // vertex (`attributes_of` takes them from ATTRIBUTES, which sum to
        // FLOATS_PER_VERTEX).
        unsafe {
            (gl.use_program)(program.id);
            (gl.uniform2f)(program.u_screen, SCREEN_W, SCREEN_H);
            if let Some(u_dither) = program.u_dither {
                (gl.uniform1f)(u_dither, dither_uniform(self.dither));
            }
            for a in &program.attributes {
                (gl.enable_vertex_attrib_array)(a.at);
                (gl.vertex_attrib_pointer)(
                    a.at,
                    a.floats,
                    gl::FLOAT,
                    gl::FALSE,
                    stride,
                    (a.offset * size_of::<f32>()) as *const c_void,
                );
            }
        }
    }

    /// A texture of its own for a `w` x `h` image, filtered linearly and
    /// clamped at its edges: 3 bytes a pixel if `rgb`, else premultiplied
    /// RGBA. Its GL name, or GL's error if it would not take the pixels (it
    /// is deleted then).
    pub fn create_texture(&self, w: u32, h: u32, rgb: bool, pixels: &[u8]) -> Result<u32, String> {
        let (format, bytes) = if rgb { (gl::RGB, 3) } else { (gl::RGBA, 4) };
        // GL reads w * h pixels: this holds in release too.
        assert_eq!(pixels.len(), (w * h * bytes) as usize, "{w}x{h}");
        let gl = &self.gl;
        // An error from before is not this upload's: logged as the loop logs
        // GL's flag, so it is not lost.
        let before = gl.error();
        if before != gl::NO_ERROR {
            crate::info!("GL error {before:#x}");
        }
        let mut name = 0;
        // SAFETY: `name` is a valid out-pointer for one name; `pixels` holds
        // exactly w * h pixels of the format, tightly packed, which an
        // alignment of 1 matches whatever the width; it outlives the call,
        // and GL copies it.
        unsafe {
            (gl.gen_textures)(1, &mut name);
            (gl.active_texture)(gl::TEXTURE0);
            (gl.bind_texture)(gl::TEXTURE_2D, name);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE);
            (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE);
            (gl.pixel_storei)(gl::UNPACK_ALIGNMENT, 1);
            (gl.tex_image_2d)(
                gl::TEXTURE_2D,
                0,
                format as i32,
                w as i32,
                h as i32,
                0,
                format,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
            (gl.pixel_storei)(gl::UNPACK_ALIGNMENT, 4);
        }
        match gl.error() {
            gl::NO_ERROR => Ok(name),
            err => {
                self.delete_texture(name);
                Err(format!("GL error {err:#x}"))
            }
        }
    }

    /// Deletes a texture `create_texture` made. GL keeps its pixels until
    /// the frames already handed over are drawn.
    pub fn delete_texture(&self, name: u32) {
        let gl = &self.gl;
        // SAFETY: `name` points at one texture name, which GL ignores if it
        // is not a texture.
        unsafe { (gl.delete_textures)(1, &name) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every vertex of the batch, as its 15 floats.
    fn vertices(batch: &Batch) -> Vec<&[f32]> {
        batch.verts.chunks(FLOATS_PER_VERTEX).collect()
    }

    /// `source` as GLSL's preprocessor leaves it for `DITHER` defined or
    /// not: lines inside `#ifdef DITHER` kept or dropped, the directives
    /// themselves gone, every other line (other `#` lines included) as it is.
    fn with_dither(source: &str, dither: bool) -> String {
        // One entry per open `#ifdef`: whether it is DITHER's.
        let mut open: Vec<bool> = Vec::new();
        let mut out = String::new();
        for line in source.split_inclusive('\n') {
            let t = line.trim();
            if t.starts_with("#if") {
                open.push(t == "#ifdef DITHER");
                if t == "#ifdef DITHER" {
                    continue;
                }
            } else if t == "#endif" && open.pop() == Some(true) {
                continue;
            }
            assert!(
                !(t == "#else" && open.last() == Some(&true)),
                "no #else for DITHER"
            );
            if dither || !open.contains(&true) {
                out.push_str(line);
            }
        }
        assert!(open.is_empty(), "an #ifdef without its #endif");
        out
    }

    /// The fragment shader before lane J's dither (838b5d8), 17 ms a frame
    /// on the B9.
    const FRAGMENT_SHADER_BEFORE_DITHER: &str = "
#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif
uniform sampler2D u_atlas;
varying vec2 v_uv;
varying vec4 v_color;
varying vec2 v_local;
varying vec4 v_shape;
varying float v_kind;
void main() {
    vec4 c = vec4(v_color.rgb * v_color.a, v_color.a);
    if (v_kind > 1.5) {
        gl_FragColor = texture2D(u_atlas, v_uv) * c;
    } else if (v_kind > 0.5) {
        vec2 q = abs(v_local) - v_shape.xy + v_shape.z;
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_shape.z;
        gl_FragColor = c * (1.0 - smoothstep(-v_shape.w, v_shape.w, d));
    } else {
        gl_FragColor = c;
    }
}
";

    #[test]
    fn without_the_dither_the_fragment_shader_is_the_one_from_before_it() {
        assert_eq!(
            with_dither(FRAGMENT_SHADER, false),
            FRAGMENT_SHADER_BEFORE_DITHER
        );
        // Nor does the vertex shader hand over the place the dither reads.
        let vs = with_dither(VERTEX_SHADER, false);
        assert!(!vs.contains("v_bayer"), "{vs}");
        assert!(vs.contains("gl_Position = vec4(ndc * a_w, 0.0, a_w);"));
    }

    #[test]
    fn with_the_dither_both_shaders_carry_its_place_and_strength() {
        let vs = with_dither(VERTEX_SHADER, true);
        let fs = with_dither(FRAGMENT_SHADER, true);
        for line in [
            "varying vec3 v_bayer;",
            "v_bayer = vec3(a_pos * (0.25 * a_w), a_w);",
        ] {
            assert!(vs.contains(line), "{line}");
        }
        for line in [
            "uniform sampler2D u_bayer;",
            "uniform float u_dither;",
            "varying vec3 v_bayer;",
            "texture2DProj(u_bayer, v_bayer).r - 0.5",
            // Images are not dithered: they leave before it.
            "gl_FragColor = texture2D(u_atlas, v_uv) * c;\n        return;",
        ] {
            assert!(fs.contains(line), "{line}");
        }
        assert!(!fs.contains("DITHER") && !vs.contains("DITHER"));
    }

    #[test]
    fn the_attributes_fill_a_vertex_exactly() {
        let floats: i32 = ATTRIBUTES.iter().map(|(_, n)| n).sum();
        assert_eq!(floats as usize, FLOATS_PER_VERTEX);
    }

    #[test]
    fn a_rect_is_two_flat_triangles() {
        let mut batch = Batch::default();
        let red = Rgba::rgb8(255, 0, 0);
        batch.gradient_rect(10.0, 20.0, 30.0, 40.0, red, red);
        assert_eq!(batch.vertex_count(), VERTICES_PER_QUAD);
        assert_eq!(batch.bounds(), (10.0, 20.0, 40.0, 60.0));
        assert!(vertices(&batch).iter().all(|v| v[14] == FLAT));
    }

    #[test]
    fn a_rounded_rect_is_one_shape_quad_a_pixel_bigger() {
        let mut batch = Batch::default();
        let white = Rgba::rgb8(255, 255, 255);
        batch.rounded_rect(100.0, 200.0, 320.0, 192.0, 12.0, white, white);
        assert_eq!(batch.quad_count(), 1);
        assert_eq!(batch.bounds(), (99.0, 199.0, 421.0, 393.0));
        for v in vertices(&batch) {
            assert_eq!(&v[10..15], &[160.0, 96.0, 12.0, 0.5, SHAPE]);
        }
        // The top-left corner sits 161 px left of and 97 px above the centre.
        assert_eq!(&vertices(&batch)[0][8..10], &[-161.0, -97.0]);
    }

    #[test]
    fn rounded_rect_clamps_its_radius_and_skips_empty_rects() {
        let mut batch = Batch::default();
        let c = Rgba::rgb8(1, 2, 3);
        batch.rounded_rect(0.0, 0.0, 0.0, 10.0, 4.0, c, c);
        assert_eq!(batch.quad_count(), 0);
        batch.rounded_rect(0.0, 0.0, 40.0, 10.0, 50.0, c, c);
        assert_eq!(vertices(&batch)[0][12], 5.0);
    }

    #[test]
    fn a_shape_keeps_its_gradient_where_it_was() {
        let mut batch = Batch::default();
        let (black, white) = (Rgba::rgb8(0, 0, 0), Rgba::rgb8(255, 255, 255));
        batch.rounded_rect(0.0, 0.0, 100.0, 100.0, 10.0, black, white);
        let vs = vertices(&batch);
        // The grown quad starts a pixel above the rect, a hundredth "before" black.
        assert!((vs[0][4] - -0.01).abs() < 1e-6);
        let bottom = vs.iter().map(|v| v[4]).fold(f32::MIN, f32::max);
        assert!((bottom - 1.01).abs() < 1e-6);
    }

    #[test]
    fn a_shadow_grows_by_its_blur_and_keeps_the_rect_as_its_shape() {
        let mut batch = Batch::default();
        batch.shadow(0.0, 0.0, 200.0, 100.0, 12.0, 24.0, Rgba(0.0, 0.0, 0.0, 0.4));
        assert_eq!(batch.quad_count(), 1);
        assert_eq!(batch.bounds(), (-24.5, -24.5, 224.5, 124.5));
        assert_eq!(
            &vertices(&batch)[0][10..15],
            &[100.0, 50.0, 12.0, 24.0, SHAPE]
        );
        batch.shadow(0.0, 0.0, 200.0, 100.0, 12.0, 24.0, Rgba(0.0, 0.0, 0.0, 0.0));
        assert_eq!(batch.quad_count(), 1, "an invisible shadow is skipped");
    }

    #[test]
    fn an_image_carries_its_atlas_region_and_tint() {
        let mut batch = Batch::default();
        let uv = Uv {
            u0: 0.25,
            v0: 0.5,
            u1: 0.75,
            v1: 1.0,
        };
        batch.image(10.0, 10.0, 64.0, 32.0, uv, Rgba(1.0, 1.0, 1.0, 0.5));
        let vs = vertices(&batch);
        assert!(vs.iter().all(|v| v[14] == IMAGE && v[7] == 0.5));
        let corners: Vec<(f32, f32, f32, f32)> =
            vs.iter().map(|v| (v[0], v[1], v[2], v[3])).collect();
        assert!(corners.contains(&(10.0, 10.0, 0.25, 0.5)));
        assert!(corners.contains(&(74.0, 42.0, 0.75, 1.0)));
        batch.image(0.0, 0.0, 0.0, 10.0, uv, Rgba(1.0, 1.0, 1.0, 1.0));
        assert_eq!(batch.quad_count(), 1, "an empty image is skipped");
    }

    /// Each run as its texture, first vertex and vertex count.
    fn runs(batch: &Batch) -> Vec<(Texture, usize, usize)> {
        batch
            .runs()
            .iter()
            .map(|r| (r.texture, r.first, r.count))
            .collect()
    }

    #[test]
    fn a_batch_draws_in_one_run_per_change_of_texture() {
        let mut batch = Batch::default();
        let (c, uv) = (Rgba::rgb8(1, 2, 3), NO_UV);
        batch.gradient_rect(0.0, 0.0, 10.0, 10.0, c, c);
        // A layer's quad takes over the run of the flat quad before it.
        batch.image_from(Texture::Own(7), 0.0, 0.0, 10.0, 10.0, uv, c);
        batch.rounded_rect(0.0, 0.0, 10.0, 10.0, 2.0, c, c);
        batch.image_from(Texture::Own(9), 0.0, 0.0, 10.0, 10.0, uv, c);
        batch.image(0.0, 0.0, 10.0, 10.0, uv, c);
        batch.image(0.0, 0.0, 10.0, 10.0, uv, c);
        batch.shadow(0.0, 0.0, 10.0, 10.0, 2.0, 4.0, c);
        assert_eq!(
            runs(&batch),
            [
                (Texture::Own(7), 0, 18),
                (Texture::Own(9), 18, 6),
                (Texture::Atlas, 24, 18)
            ]
        );
        let total: usize = batch.runs().iter().map(|r| r.count).sum();
        assert_eq!(total, batch.vertex_count());
        batch.clear();
        assert!(batch.runs().is_empty());
    }

    #[test]
    fn a_batch_of_atlas_and_flat_quads_is_one_run() {
        let mut batch = Batch::default();
        let (c, uv) = (Rgba::rgb8(1, 2, 3), NO_UV);
        batch.gradient_rect(0.0, 0.0, 10.0, 10.0, c, c);
        batch.image(0.0, 0.0, 10.0, 10.0, uv, c);
        batch.rounded_rect(0.0, 0.0, 10.0, 10.0, 2.0, c, c);
        batch.image(0.0, 0.0, 10.0, 10.0, uv, c);
        assert_eq!(runs(&batch), [(Texture::Atlas, 0, 24)]);
        let mut flat = Batch::default();
        flat.gradient_rect(0.0, 0.0, 10.0, 10.0, c, c);
        assert_eq!(runs(&flat), [(Texture::Atlas, 0, 6)]);
    }

    #[test]
    fn a_leaning_quad_is_drawn_in_perspective_and_keeps_its_own_place() {
        let c = Rgba(1.0, 1.0, 1.0, 1.0);
        let mut batch = Batch::default();
        // Turned 0.1 rad about the upright axis through its middle, so its
        // right side goes back.
        batch.set_tilt(Some(Tilt::new(100.0, 50.0, [0.1, 0.0], 1000.0)));
        batch.gradient_rect(0.0, 0.0, 200.0, 100.0, c, c);
        batch.set_tilt(None);
        batch.gradient_rect(0.0, 0.0, 200.0, 100.0, c, c);
        let vs = vertices(&batch);
        // Corners: top left, top right, bottom left, and bottom right fifth.
        let (tl, tr, bl, br) = (vs[0], vs[1], vs[2], vs[4]);
        assert!(tr[15] > 1.0 && tl[15] < 1.0, "w: {} {}", tl[15], tr[15]);
        let (left, right) = (bl[1] - tl[1], br[1] - tr[1]);
        assert!(
            right < 100.0 && left > 100.0 && right < left,
            "{left} {right}"
        );
        // Projected about the middle: the centre line stays put.
        assert!((tl[1] + bl[1] - 100.0).abs() < 1e-3);
        // The same plane seen from in front: x' = cx + dx cos / w.
        let w = (1000.0 + 100.0 * 0.1f32.sin()) / 1000.0;
        assert!((tr[0] - (100.0 + 100.0 * 0.1f32.cos() / w)).abs() < 1e-3);
        // The shape offset is the quad's own, not the projection's.
        assert_eq!((tr[8], tr[9]), (200.0, 0.0));
        // Level again, w is 1 and nothing moves.
        let level = &vs[6..12];
        assert!(level.iter().all(|v| v[15] == 1.0));
        assert_eq!((level[1][0], level[1][1]), (200.0, 0.0));
        // A cleared batch starts level.
        batch.set_tilt(Some(Tilt::new(0.0, 0.0, [0.1, 0.1], 1000.0)));
        batch.clear();
        batch.gradient_rect(0.0, 0.0, 10.0, 10.0, c, c);
        assert!(vertices(&batch).iter().all(|v| v[15] == 1.0));
    }

    #[test]
    fn colour_helpers() {
        let a = Rgba(0.0, 0.2, 1.0, 0.0);
        let b = Rgba(1.0, 0.4, 0.0, 1.0);
        assert_eq!(a.lerp(b, 0.5), Rgba(0.5, 0.3, 0.5, 0.5));
        assert_eq!(b.shade(0.5), Rgba(0.5, 0.2, 0.0, 1.0));
    }

    #[test]
    fn viewport_letterboxes_to_16_by_9() {
        assert_eq!(fit_viewport(1920, 1080), (0, 0, 1920, 1080));
        assert_eq!(fit_viewport(1280, 720), (0, 0, 1280, 720));
        // A 4:3 window keeps full width and centres the picture vertically.
        assert_eq!(fit_viewport(1024, 768), (0, 96, 1024, 576));
        // A tall-and-wide window keeps full height and centres horizontally.
        assert_eq!(fit_viewport(2000, 1080), (40, 0, 1920, 1080));
    }

    #[test]
    fn rgb8_maps_bytes_to_unit_range() {
        assert_eq!(Rgba::rgb8(255, 0, 51), Rgba(1.0, 0.0, 0.2, 1.0));
        assert_eq!(Rgba::rgb8(0, 0, 0).alpha(0.5).3, 0.5);
    }

    /// `v` as a 16-bit float holds it, rounded to nearest, ties to even:
    /// Mali's mediump.
    fn half(v: f32) -> f32 {
        if v == 0.0 {
            return 0.0;
        }
        let ulp = 2f32.powi(v.abs().log2().floor() as i32 - 10);
        (v / ulp).round_ties_even() * ulp
    }

    /// The dither threshold the shader worked out at pixel (x, y) before the
    /// texture (2026-11-08), each step rounded by `r`: `half` for mediump,
    /// nothing for highp.
    fn threshold(x: u32, y: u32, r: fn(f32) -> f32) -> f32 {
        let bayer2 = |ax: f32, ay: f32| {
            let (ax, ay) = (ax.floor(), ay.floor());
            r(r(ax * 0.5) + r(ay * r(ay * 0.75))).fract()
        };
        // `v_pos` at the pixel's centre.
        let p = |v: u32| {
            let p = r(v as f32 + 0.5).floor();
            r(p - r(4.0 * r(p * 0.25).floor()))
        };
        let (px, py) = (p(x), p(y));
        r(r(bayer2(r(px * 0.5), r(py * 0.5)) * 0.25) + bayer2(px, py))
    }

    /// The texel `BAYER` gives pixel `x`'s centre, with its place rounded by
    /// `r`: `(x + 0.5) / 4` texels, repeating every 4, nearest.
    fn texel(x: u32, r: fn(f32) -> f32) -> u32 {
        let u = r((x as f32 + 0.5) * 0.25);
        ((u - u.floor()) * 4.0).floor() as u32
    }

    #[test]
    fn the_texture_holds_the_pattern_the_shader_worked_out() {
        let exact = |v: f32| v;
        // At every pixel of the screen, the fetch less a half, times the
        // uniform, is the offset the arithmetic gave.
        for y in (0..1080).step_by(7) {
            for x in (0..1920).step_by(5) {
                let level = BAYER[(texel(y, exact) * 4 + texel(x, exact)) as usize];
                let fetched = f32::from(level) / 255.0;
                let now = (fetched - 0.5) * dither_uniform(1.0);
                let before = threshold(x, y, exact) - 15.0 / 32.0;
                assert!((now - before).abs() < 1e-6, "({x}, {y}): {now} {before}");
            }
        }
        // A highp place picks the right texel all the way across; a 16-bit
        // one past 1024 px would not, which is why the varying is highp.
        assert!((0..1920).all(|x| texel(x, exact) == x % 4));
        assert!((1024..1920).any(|x| texel(x, half) != x % 4));
    }

    #[test]
    fn the_old_arithmetic_was_a_4x4_bayer_pattern_at_highp_and_mediump() {
        let block = |x0: u32, y0: u32, r: fn(f32) -> f32| {
            let mut ts: Vec<f32> = (0..16)
                .map(|i| threshold(x0 + i % 4, y0 + i / 4, r))
                .collect();
            ts.sort_by(f32::total_cmp);
            ts
        };
        let sixteen: Vec<f32> = (0..16).map(|i| i as f32 / 16.0).collect();
        let exact = |v: f32| v;
        let mean = |ts: &[f32]| ts.iter().sum::<f32>() / ts.len() as f32;
        // Highp, as on the B9: each of the 16 levels once in every 4x4
        // block, all over the screen, centred on 15/32, so the dither adds
        // nothing on average.
        for (x0, y0) in [(0, 0), (4, 8), (1024, 0), (1916, 1076)] {
            assert_eq!(block(x0, y0, exact), sixteen, "({x0}, {y0})");
        }
        assert_eq!(mean(&sixteen), 15.0 / 32.0);
        // Mediump, on a GPU without highp: the same up to 1024 px.
        for (x0, y0) in [(0, 0), (4, 8), (1016, 1016)] {
            assert_eq!(block(x0, y0, half), sixteen, "({x0}, {y0})");
        }
        // Past 1024 a 16-bit float holds only even pixel centres. In x that
        // leaves 8 levels, still centred; in y, the bottom 56 rows, the
        // pattern leans dark by an eighth of a step, by three eighths where
        // both are past it. Why the varying is highp.
        let x_past = block(1024, 0, half);
        let mut levels = x_past.clone();
        levels.dedup();
        assert_eq!(levels.len(), 8, "{x_past:?}");
        assert_eq!(mean(&x_past), 15.0 / 32.0);
        assert_eq!(mean(&block(0, 1024, half)), 11.0 / 32.0);
        assert_eq!(mean(&block(1024, 1024, half)), 3.0 / 32.0);
        // Half an 8-bit step either way, at most.
        let reach = (15.0 / 32.0) * style::DITHER * 255.0;
        assert!(reach <= 0.5, "{reach}");
    }
}
