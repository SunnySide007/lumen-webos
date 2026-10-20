//! A batched rectangle renderer: every frame is a list of coloured quads in a
//! fixed 1920x1080 space, uploaded once and drawn with one call. On the B9 one
//! such call of about 3,000 quads held 60 fps (hello-gles; OLED55B9PUA, webOS
//! 4.10.2-31, firmware 05.50.00).

use std::ffi::c_void;

use crate::font;
use crate::gl::{self, Gl};

/// The surface the UI is designed for. The TV's graphics plane is this size;
/// on a PC it is letterboxed into the window.
pub use lumen_core::layout::{SCREEN_H, SCREEN_W};

const FLOATS_PER_VERTEX: usize = 6; // x, y, r, g, b, a
const VERTICES_PER_QUAD: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
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

/// Strips per rounded corner band, so a rounded rectangle is 13 quads.
const CORNER_STRIPS: usize = 6;

#[derive(Default)]
pub struct Batch {
    verts: Vec<f32>,
}

impl Batch {
    pub fn clear(&mut self) {
        self.verts.clear();
    }

    pub fn vertex_count(&self) -> usize {
        self.verts.len() / FLOATS_PER_VERTEX
    }

    pub fn quad_count(&self) -> usize {
        self.vertex_count() / VERTICES_PER_QUAD
    }

    fn vertex(&mut self, x: f32, y: f32, c: Rgba) {
        self.verts.extend_from_slice(&[x, y, c.0, c.1, c.2, c.3]);
    }

    /// A rectangle shaded from `top` to `bottom`.
    pub fn gradient_rect(&mut self, x: f32, y: f32, w: f32, h: f32, top: Rgba, bottom: Rgba) {
        let (x1, y1) = (x + w, y + h);
        self.vertex(x, y, top);
        self.vertex(x1, y, top);
        self.vertex(x, y1, bottom);
        self.vertex(x1, y, top);
        self.vertex(x1, y1, bottom);
        self.vertex(x, y1, bottom);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        self.gradient_rect(x, y, w, h, c, c);
    }

    /// A rectangle with corners of radius `r`, shaded from `top` to `bottom`.
    /// Each corner band is `CORNER_STRIPS` horizontal strips inset to follow
    /// the curve, plus one rectangle for the middle: 13 quads. Shader-drawn
    /// corners come with textures (milestone B2).
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
        let at = |yy: f32| top.lerp(bottom, (yy - y) / h);
        let band = r / CORNER_STRIPS as f32;
        for i in 0..CORNER_STRIPS {
            // How far the corner's circle is from the side, at the strip's middle.
            let dy = r - (i as f32 + 0.5) * band;
            let inset = r - (r * r - dy * dy).max(0.0).sqrt();
            let upper = y + i as f32 * band;
            let lower = y + h - (i as f32 + 1.0) * band;
            for sy in [upper, lower] {
                self.gradient_rect(x + inset, sy, w - 2.0 * inset, band, at(sy), at(sy + band));
            }
        }
        self.gradient_rect(x, y + r, w, h - 2.0 * r, at(y + r), at(y + h - r));
    }

    /// Diagnostic text in the 5x7 font, each font pixel `scale` screen pixels.
    pub fn text(&mut self, x: f32, y: f32, scale: f32, text: &str, c: Rgba) {
        font::for_each_pixel(text, |px, py| {
            self.rect(
                x + px as f32 * scale,
                y + py as f32 * scale,
                scale,
                scale,
                c,
            );
        });
    }

    /// Like `text`, but `right` is where the text ends.
    pub fn text_right(&mut self, right: f32, y: f32, scale: f32, text: &str, c: Rgba) {
        self.text(right - font::width(text) as f32 * scale, y, scale, text, c);
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

// No #version line: GLSL ES 1.00 and desktop GLSL 1.10 both accept this, and
// the precision statement only exists where GL_ES is defined.
const VERTEX_SHADER: &str = "
attribute vec2 a_pos;
attribute vec4 a_color;
uniform vec2 u_screen;
varying vec4 v_color;
void main() {
    gl_Position = vec4(a_pos.x / u_screen.x * 2.0 - 1.0, 1.0 - a_pos.y / u_screen.y * 2.0, 0.0, 1.0);
    v_color = a_color;
}
";

const FRAGMENT_SHADER: &str = "
#ifdef GL_ES
precision mediump float;
#endif
varying vec4 v_color;
void main() {
    gl_FragColor = v_color;
}
";

pub struct Renderer {
    gl: Gl,
    program: u32,
    vbo: u32,
    a_pos: u32,
    a_color: u32,
    u_screen: i32,
}

impl Renderer {
    pub fn new(gl: Gl) -> Result<Self, String> {
        let vs = gl.compile_shader(gl::VERTEX_SHADER, VERTEX_SHADER)?;
        let fs = gl.compile_shader(gl::FRAGMENT_SHADER, FRAGMENT_SHADER)?;
        let program = gl.link_program(vs, fs)?;
        // SAFETY: the program linked and the names are NUL-terminated literals.
        let (a_pos, a_color, u_screen) = unsafe {
            (
                (gl.get_attrib_location)(program, c"a_pos".as_ptr()),
                (gl.get_attrib_location)(program, c"a_color".as_ptr()),
                (gl.get_uniform_location)(program, c"u_screen".as_ptr()),
            )
        };
        if a_pos < 0 || a_color < 0 || u_screen < 0 {
            return Err("shader is missing a_pos, a_color or u_screen".to_owned());
        }
        let mut vbo = 0;
        // SAFETY: `vbo` is a valid out-pointer for one buffer name.
        unsafe {
            (gl.gen_buffers)(1, &mut vbo);
            (gl.enable)(gl::BLEND);
            (gl.blend_func)(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        }
        Ok(Self {
            gl,
            program,
            vbo,
            a_pos: a_pos as u32,
            a_color: a_color as u32,
            u_screen,
        })
    }

    pub fn gl(&self) -> &Gl {
        &self.gl
    }

    /// Clears the whole drawable to black, then draws `batch` letterboxed into it.
    pub fn draw(&self, batch: &Batch, drawable: (i32, i32)) {
        let gl = &self.gl;
        let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
        let stride = (FLOATS_PER_VERTEX * size_of::<f32>()) as i32;
        let bytes = (batch.verts.len() * size_of::<f32>()) as isize;
        // SAFETY: the program, buffer and attribute locations were created by
        // `new` on this context; the upload reads exactly `bytes` from the
        // batch, which outlives the call; offsets stay inside one vertex.
        unsafe {
            (gl.viewport)(0, 0, drawable.0, drawable.1);
            (gl.clear_color)(0.0, 0.0, 0.0, 1.0);
            (gl.clear)(gl::COLOR_BUFFER_BIT);
            (gl.viewport)(x, y, w, h);
            (gl.use_program)(self.program);
            (gl.uniform2f)(self.u_screen, SCREEN_W, SCREEN_H);
            (gl.bind_buffer)(gl::ARRAY_BUFFER, self.vbo);
            (gl.buffer_data)(
                gl::ARRAY_BUFFER,
                bytes,
                batch.verts.as_ptr().cast(),
                gl::STREAM_DRAW,
            );
            (gl.enable_vertex_attrib_array)(self.a_pos);
            (gl.enable_vertex_attrib_array)(self.a_color);
            (gl.vertex_attrib_pointer)(
                self.a_pos,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                std::ptr::null::<c_void>(),
            );
            (gl.vertex_attrib_pointer)(
                self.a_color,
                4,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (2 * size_of::<f32>()) as *const c_void,
            );
            (gl.draw_arrays)(gl::TRIANGLES, 0, batch.vertex_count() as i32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs_and_ys(batch: &Batch) -> (Vec<f32>, Vec<f32>) {
        batch
            .verts
            .chunks(FLOATS_PER_VERTEX)
            .map(|v| (v[0], v[1]))
            .unzip()
    }

    #[test]
    fn a_rounded_rect_is_13_quads_inside_its_bounds() {
        let mut batch = Batch::default();
        let white = Rgba::rgb8(255, 255, 255);
        batch.rounded_rect(100.0, 200.0, 320.0, 192.0, 12.0, white, white);
        assert_eq!(batch.quad_count(), 2 * CORNER_STRIPS + 1);
        let (xs, ys) = xs_and_ys(&batch);
        assert!(xs.iter().all(|&x| (100.0..=420.0).contains(&x)));
        assert!(ys.iter().all(|&y| (200.0..=392.0).contains(&y)));
        // The outermost strips are inset most; the middle spans the full width.
        let top_strip_left = batch.verts[0];
        assert!(top_strip_left > 100.0 + 12.0 / 2.0);
        assert!(xs.contains(&100.0) && xs.contains(&420.0));
    }

    #[test]
    fn rounded_rect_clamps_its_radius_and_skips_empty_rects() {
        let mut batch = Batch::default();
        let c = Rgba::rgb8(1, 2, 3);
        batch.rounded_rect(0.0, 0.0, 0.0, 10.0, 4.0, c, c);
        assert_eq!(batch.quad_count(), 0);
        batch.rounded_rect(0.0, 0.0, 40.0, 10.0, 50.0, c, c);
        let (xs, ys) = xs_and_ys(&batch);
        assert!(xs.iter().all(|&x| (0.0..=40.0).contains(&x)));
        assert!(ys.iter().all(|&y| (0.0..=10.0).contains(&y)));
    }

    #[test]
    fn rounded_rect_shades_from_top_to_bottom() {
        let mut batch = Batch::default();
        let (black, white) = (Rgba::rgb8(0, 0, 0), Rgba::rgb8(255, 255, 255));
        batch.rounded_rect(0.0, 0.0, 100.0, 100.0, 10.0, black, white);
        let first_red = batch.verts[2];
        let reds: Vec<f32> = batch
            .verts
            .chunks(FLOATS_PER_VERTEX)
            .map(|v| v[2])
            .collect();
        assert_eq!(first_red, 0.0);
        assert!(reds.iter().any(|&r| (r - 1.0).abs() < 1e-6));
    }

    #[test]
    fn colour_helpers() {
        let a = Rgba(0.0, 0.2, 1.0, 0.0);
        let b = Rgba(1.0, 0.4, 0.0, 1.0);
        assert_eq!(a.lerp(b, 0.5), Rgba(0.5, 0.3, 0.5, 0.5));
        assert_eq!(b.shade(0.5), Rgba(0.5, 0.2, 0.0, 1.0));
    }

    #[test]
    fn a_rect_is_two_triangles() {
        let mut batch = Batch::default();
        batch.rect(10.0, 20.0, 30.0, 40.0, Rgba::rgb8(255, 0, 0));
        assert_eq!(batch.vertex_count(), VERTICES_PER_QUAD);
        let xs: Vec<f32> = batch
            .verts
            .chunks(FLOATS_PER_VERTEX)
            .map(|v| v[0])
            .collect();
        assert!(xs.iter().all(|&x| x == 10.0 || x == 40.0));
    }

    #[test]
    fn text_draws_one_quad_per_lit_pixel() {
        let mut lit = 0;
        font::for_each_pixel("LUMEN 60", |_, _| lit += 1);
        let mut batch = Batch::default();
        batch.text(0.0, 0.0, 4.0, "LUMEN 60", Rgba::rgb8(255, 255, 255));
        assert_eq!(batch.quad_count(), lit);
    }

    #[test]
    fn right_aligned_text_ends_at_the_edge() {
        let mut batch = Batch::default();
        batch.text_right(1000.0, 0.0, 2.0, "I", Rgba::rgb8(255, 255, 255));
        let max_x = batch
            .verts
            .chunks(FLOATS_PER_VERTEX)
            .map(|v| v[0])
            .fold(f32::MIN, f32::max);
        // "I" lights columns 1..=3 of 5, so its ink ends one font pixel short.
        assert_eq!(max_x, 1000.0 - 2.0);
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
}
