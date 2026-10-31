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
//! The batch is drawn in runs, one for each change of texture: the heroes'
//! quads come first, under everything, so a frame has one run per hero layer
//! on screen and one for the atlas. The shader keeps its one sampler, and the
//! vertices go to GL in one upload.

use std::ffi::c_void;

use lumen_core::style::{self, Color};

use crate::atlas::{ATLAS_H, ATLAS_W, Upload, Uv};
use crate::gl::{self, Gl};

/// The surface the UI is designed for. The TV's graphics plane is this size;
/// on a PC it is letterboxed into the window.
pub use lumen_core::layout::{SCREEN_H, SCREEN_W};

/// x, y; u, v; r, g, b, a; the pixel's offset from the shape's centre; the
/// shape's half width, half height, corner radius and edge softness; kind.
const FLOATS_PER_VERTEX: usize = 15;
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

/// Vertices drawn with one texture bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub texture: Texture,
    pub first: usize,
    pub count: usize,
    /// Whether any of its quads samples the texture; flat and shape quads
    /// draw the same with any texture bound, so they join any run.
    sampled: bool,
}

/// A quad's corners, and what they carry besides position.
struct Quad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: Uv,
    top: Rgba,
    bottom: Rgba,
    shape: Shape,
    kind: f32,
    /// What an image quad samples; `None` for flat and shape quads.
    texture: Option<Texture>,
}

#[derive(Default)]
pub struct Batch {
    verts: Vec<f32>,
    runs: Vec<Run>,
}

impl Batch {
    pub fn clear(&mut self) {
        self.verts.clear();
        self.runs.clear();
    }

    /// The runs to draw, in order: each a texture and the vertices drawn
    /// with it bound.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// Counts a quad's six vertices into a run: the last one if it can take
    /// them, else a new one. `texture` is what the quad samples, `None` for
    /// a flat or shape quad.
    fn add_to_run(&mut self, texture: Option<Texture>) {
        let first = self.vertex_count() - VERTICES_PER_QUAD;
        match (self.runs.last_mut(), texture) {
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
            (q.x, q.y, q.uv.u0, q.uv.v0, q.top),
            (x1, q.y, q.uv.u1, q.uv.v0, q.top),
            (q.x, y1, q.uv.u0, q.uv.v1, q.bottom),
            (x1, y1, q.uv.u1, q.uv.v1, q.bottom),
        ];
        for i in [0, 1, 2, 1, 3, 2] {
            let (x, y, u, v, c) = corners[i];
            self.verts.extend_from_slice(&[
                x,
                y,
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
            ]);
        }
        self.add_to_run(q.texture);
    }

    /// A rectangle shaded from `top` to `bottom`.
    pub fn gradient_rect(&mut self, x: f32, y: f32, w: f32, h: f32, top: Rgba, bottom: Rgba) {
        self.push(&Quad {
            x,
            y,
            w,
            h,
            uv: NO_UV,
            top,
            bottom,
            shape: NO_SHAPE,
            kind: FLAT,
            texture: None,
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
        self.push(&Quad {
            x: x - margin,
            y: y - margin,
            w: w + 2.0 * margin,
            h: h + 2.0 * margin,
            uv: NO_UV,
            top: top.lerp(bottom, -margin / h),
            bottom: top.lerp(bottom, 1.0 + margin / h),
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
        if w <= 0.0 || h <= 0.0 || tint.3 <= 0.0 {
            return;
        }
        self.push(&Quad {
            x,
            y,
            w,
            h,
            uv,
            top: tint,
            bottom: tint,
            shape: NO_SHAPE,
            kind: IMAGE,
            texture: Some(texture),
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
uniform vec2 u_screen;
varying vec2 v_pos;
varying vec2 v_uv;
varying vec4 v_color;
varying vec2 v_local;
varying vec4 v_shape;
varying float v_kind;
void main() {
    gl_Position = vec4(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0, 0.0, 1.0);
    v_pos = a_pos;
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
// threshold from the pixel's place, centred, times `u_dither` (peak to
// peak) and the pixel's alpha, so the 8-bit framebuffer's rounding breaks a
// gradient's bands into a pattern finer than the eye resolves. Images, the
// heroes and the atlas, are left as they are.
//
// The place is `v_pos`, the design-space position, which on the TV is the
// framebuffer's pixel: a varying takes the shader's highp, where GLSL ES
// declares `gl_FragCoord` mediump. In 16-bit floats a position past 1024
// holds only even pixel centres, which would leave 8 levels in x and bias
// the pattern in y; `tests` work the arithmetic both ways. Mod 4 is
// p - 4 floor(p / 4), exact for whole numbers either way.
const FRAGMENT_SHADER: &str = "
#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif
uniform sampler2D u_atlas;
uniform float u_dither;
varying vec2 v_pos;
varying vec2 v_uv;
varying vec4 v_color;
varying vec2 v_local;
varying vec4 v_shape;
varying float v_kind;
float bayer2(vec2 a) {
    a = floor(a);
    return fract(dot(a, vec2(0.5, a.y * 0.75)));
}
void main() {
    vec4 c = vec4(v_color.rgb * v_color.a, v_color.a);
    if (v_kind > 1.5) {
        gl_FragColor = texture2D(u_atlas, v_uv) * c;
        return;
    } else if (v_kind > 0.5) {
        vec2 q = abs(v_local) - v_shape.xy + v_shape.z;
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_shape.z;
        gl_FragColor = c * (1.0 - smoothstep(-v_shape.w, v_shape.w, d));
    } else {
        gl_FragColor = c;
    }
    if (u_dither > 0.0) {
        vec2 p = floor(v_pos);
        p -= 4.0 * floor(p * 0.25);
        float t = bayer2(p * 0.5) * 0.25 + bayer2(p);
        gl_FragColor.rgb += (t - 15.0 / 32.0) * u_dither * gl_FragColor.a;
    }
}
";

/// Each attribute: its name and how many floats it takes, in vertex order.
const ATTRIBUTES: [(&std::ffi::CStr, i32); 6] = [
    (c"a_pos", 2),
    (c"a_uv", 2),
    (c"a_color", 4),
    (c"a_local", 2),
    (c"a_shape", 4),
    (c"a_kind", 1),
];

pub struct Renderer {
    gl: Gl,
    program: u32,
    vbo: u32,
    texture: u32,
    attributes: [u32; ATTRIBUTES.len()],
    u_screen: i32,
    u_dither: i32,
    /// The dither's peak to peak, 0 for none.
    dither: f32,
}

impl Renderer {
    pub fn new(gl: Gl) -> Result<Self, String> {
        let vs = gl.compile_shader(gl::VERTEX_SHADER, VERTEX_SHADER)?;
        let fs = gl.compile_shader(gl::FRAGMENT_SHADER, FRAGMENT_SHADER)?;
        let program = gl.link_program(vs, fs)?;
        let mut attributes = [0; ATTRIBUTES.len()];
        for (slot, (name, _)) in attributes.iter_mut().zip(ATTRIBUTES) {
            // SAFETY: the program linked and the name is a NUL-terminated literal.
            let at = unsafe { (gl.get_attrib_location)(program, name.as_ptr()) };
            *slot = u32::try_from(at)
                .map_err(|_| format!("shader is missing {}", name.to_string_lossy()))?;
        }
        // SAFETY: as above.
        let (u_screen, u_atlas, u_dither) = unsafe {
            (
                (gl.get_uniform_location)(program, c"u_screen".as_ptr()),
                (gl.get_uniform_location)(program, c"u_atlas".as_ptr()),
                (gl.get_uniform_location)(program, c"u_dither".as_ptr()),
            )
        };
        if u_screen < 0 || u_atlas < 0 || u_dither < 0 {
            return Err("shader is missing u_screen, u_atlas or u_dither".to_owned());
        }
        let (mut vbo, mut texture) = (0, 0);
        // SAFETY: `vbo` and `texture` are valid out-pointers for one name each;
        // the texture is allocated with no data (null), which GL allows; the
        // program is the one linked above.
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
            (gl.use_program)(program);
            (gl.uniform1i)(u_atlas, 0);
            (gl.enable)(gl::BLEND);
            (gl.blend_func)(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);
        }
        Ok(Self {
            gl,
            program,
            vbo,
            texture,
            attributes,
            u_screen,
            u_dither,
            dither: 0.0,
        })
    }

    pub fn gl(&self) -> &Gl {
        &self.gl
    }

    /// Dithers flat and shape quads by `style::DITHER` if `on`.
    pub fn set_dither(&mut self, on: bool) {
        self.dither = if on { style::DITHER } else { 0.0 };
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
        let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
        let stride = (FLOATS_PER_VERTEX * size_of::<f32>()) as i32;
        let bytes = (batch.verts.len() * size_of::<f32>()) as isize;
        // SAFETY: the program, buffer, texture and attribute locations were
        // created by `new` on this context; the upload reads exactly `bytes`
        // from the batch, which outlives the call; every attribute's offset
        // and size stay inside one vertex (they sum to FLOATS_PER_VERTEX).
        unsafe {
            (gl.viewport)(0, 0, drawable.0, drawable.1);
            (gl.clear_color)(0.0, 0.0, 0.0, 1.0);
            (gl.clear)(gl::COLOR_BUFFER_BIT);
            (gl.viewport)(x, y, w, h);
            (gl.use_program)(self.program);
            (gl.uniform2f)(self.u_screen, SCREEN_W, SCREEN_H);
            (gl.uniform1f)(self.u_dither, self.dither);
            (gl.active_texture)(gl::TEXTURE0);
            (gl.bind_texture)(gl::TEXTURE_2D, self.texture);
            (gl.bind_buffer)(gl::ARRAY_BUFFER, self.vbo);
            (gl.buffer_data)(
                gl::ARRAY_BUFFER,
                bytes,
                batch.verts.as_ptr().cast(),
                gl::STREAM_DRAW,
            );
            let mut offset = 0;
            for (&at, (_, floats)) in self.attributes.iter().zip(ATTRIBUTES) {
                (gl.enable_vertex_attrib_array)(at);
                (gl.vertex_attrib_pointer)(
                    at,
                    floats,
                    gl::FLOAT,
                    gl::FALSE,
                    stride,
                    (offset * size_of::<f32>()) as *const c_void,
                );
                offset += floats as usize;
            }
            for run in batch.runs() {
                let name = match run.texture {
                    Texture::Atlas => self.texture,
                    Texture::Own(name) => name,
                };
                (gl.bind_texture)(gl::TEXTURE_2D, name);
                (gl.draw_arrays)(gl::TRIANGLES, run.first as i32, run.count as i32);
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

    /// The fragment shader's dither threshold at pixel (x, y), each step
    /// rounded by `r`: `half` for mediump, nothing for highp.
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

    #[test]
    fn the_dither_is_a_4x4_bayer_pattern_at_highp_and_mediump() {
        // The shader holds the arithmetic these tests work.
        for line in [
            "return fract(dot(a, vec2(0.5, a.y * 0.75)));",
            "vec2 p = floor(v_pos);",
            "p -= 4.0 * floor(p * 0.25);",
            "float t = bayer2(p * 0.5) * 0.25 + bayer2(p);",
            "gl_FragColor.rgb += (t - 15.0 / 32.0) * u_dither * gl_FragColor.a;",
        ] {
            assert!(FRAGMENT_SHADER.contains(line), "{line}");
        }
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
