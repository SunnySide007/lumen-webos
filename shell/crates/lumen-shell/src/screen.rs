//! The TV's screen for the home loop (`home_loop::Screen`): SDL's window and
//! its GL context, the renderer, the assets and the batch they fill. It
//! paints, swaps and reads back what the loop asks for, and times each
//! frame's parts for the slow-frame lines.

use std::path::Path;
use std::time::Instant;

use lumen_core::apps::App;
use lumen_core::home::Home;

use crate::app::{Options, on_tv};
use crate::capture;
use crate::draw;
use crate::faults;
use crate::home_loop::{Frame, Screen};
use crate::info;
use crate::render::{Batch, Renderer, SCREEN_H, SCREEN_W, fit_viewport};
use crate::sdl::{self, GlContext, Refresh, Sdl, Window};
use crate::timing::FrameSplit;

/// A window and its GL context, released in the right order on drop.
pub struct Surface<'a> {
    sdl: &'a Sdl,
    window: Window,
    ctx: GlContext,
}

impl Drop for Surface<'_> {
    fn drop(&mut self) {
        // SAFETY: both handles were created together in `open_surface` and are
        // released exactly once, context first.
        unsafe {
            self.sdl.gl_delete_context(self.ctx);
            self.sdl.destroy_window(self.window);
        }
    }
}

impl Surface<'_> {
    /// The GL drawable's size in pixels.
    pub fn drawable_size(&self) -> (i32, i32) {
        // SAFETY: the window lives as long as the surface.
        unsafe { self.sdl.drawable_size(self.window) }
    }
}

fn try_surface<'a>(sdl: &'a Sdl, opts: &Options, gles: bool) -> Result<Surface<'a>, String> {
    if gles {
        sdl.set_gl_attribute(sdl::GL_CONTEXT_PROFILE_MASK, sdl::GL_CONTEXT_PROFILE_ES);
        sdl.set_gl_attribute(sdl::GL_CONTEXT_MAJOR_VERSION, 2);
        sdl.set_gl_attribute(sdl::GL_CONTEXT_MINOR_VERSION, 0);
    } else {
        sdl.set_gl_attribute(sdl::GL_CONTEXT_PROFILE_MASK, 0);
        sdl.set_gl_attribute(sdl::GL_CONTEXT_MAJOR_VERSION, 2);
        sdl.set_gl_attribute(sdl::GL_CONTEXT_MINOR_VERSION, 1);
    }
    for attr in [sdl::GL_RED_SIZE, sdl::GL_GREEN_SIZE, sdl::GL_BLUE_SIZE] {
        sdl.set_gl_attribute(attr, 8);
    }
    sdl.set_gl_attribute(sdl::GL_DOUBLEBUFFER, 1);

    let (w, h, flags) = if opts.windowed {
        (
            1280,
            720,
            sdl::WINDOW_OPENGL | sdl::WINDOW_SHOWN | sdl::WINDOW_RESIZABLE,
        )
    } else {
        (
            SCREEN_W as i32,
            SCREEN_H as i32,
            sdl::WINDOW_OPENGL | sdl::WINDOW_SHOWN | sdl::WINDOW_FULLSCREEN,
        )
    };
    let window = sdl.create_window("Lumen", w, h, flags)?;
    // SAFETY: the window was just created with WINDOW_OPENGL.
    match unsafe { sdl.gl_create_context(window) } {
        Ok(ctx) => Ok(Surface { sdl, window, ctx }),
        Err(e) => {
            // SAFETY: the window is live and has no context.
            unsafe { sdl.destroy_window(window) };
            Err(e)
        }
    }
}

/// The TV offers GLES 2.0 only. A PC prefers desktop GL 2.1, which runs the
/// same shaders: some PC drivers give ES contexts an sRGB framebuffer that
/// brightens every dark tone (measured: navy 14,18,34 read back as 66,75,102),
/// which would make the simulator lie about how the OLED will look. Desktop GL
/// leaves that conversion off. `--gles` forces the ES path for driver checks.
pub fn open_surface<'a>(sdl: &'a Sdl, opts: &Options) -> Result<Surface<'a>, String> {
    if on_tv() || opts.gles {
        return try_surface(sdl, opts, true);
    }
    match try_surface(sdl, opts, false) {
        Ok(surface) => Ok(surface),
        Err(desktop_err) => {
            info!("no desktop GL 2.1 context ({desktop_err}); trying GLES2");
            try_surface(sdl, opts, true)
        }
    }
}

/// The TV's screen: the window and GL, and what fills the frames.
pub struct TvScreen<'a> {
    /// First, so the window and its context go before the rest.
    surface: Surface<'a>,
    renderer: Renderer,
    assets: draw::Assets,
    batch: Batch,
    /// The drawable's size when the frame painted last was painted.
    drawable: (i32, i32),
    /// The frame painted last, until its swap.
    painted: Painted,
}

/// When the parts of a frame's painting began and ended, and its uploads.
#[derive(Debug, Clone, Copy)]
struct Painted {
    batch_from: Instant,
    upload_from: Instant,
    submit_from: Instant,
    submitted: Instant,
    uploads: usize,
    upload_bytes: usize,
}

impl<'a> TvScreen<'a> {
    pub fn new(surface: Surface<'a>, renderer: Renderer, assets: draw::Assets) -> Self {
        let at = Instant::now();
        Self {
            drawable: surface.drawable_size(),
            surface,
            renderer,
            assets,
            batch: Batch::default(),
            painted: Painted {
                batch_from: at,
                upload_from: at,
                submit_from: at,
                submitted: at,
                uploads: 0,
                upload_bytes: 0,
            },
        }
    }
}

impl Screen for TvScreen<'_> {
    fn paint(&mut self, home: &Home) {
        self.drawable = self.surface.drawable_size();
        let batch_from = Instant::now();
        draw::home(&mut self.batch, &home.view(), &mut self.assets);
        let upload_from = Instant::now();
        let uploads = self.assets.atlas.take_uploads();
        self.renderer.upload(&uploads);
        let upload_count = uploads.len();
        let upload_bytes = uploads.iter().map(|u| u.rgba.len()).sum();
        // Freed here, inside the upload's time, not in the next frame's.
        drop(uploads);
        let submit_from = Instant::now();
        self.renderer.draw(&self.batch, self.drawable);
        self.painted = Painted {
            batch_from,
            upload_from,
            submit_from,
            submitted: Instant::now(),
            uploads: upload_count,
            upload_bytes,
        };
    }

    /// Reads the back buffer before the swap, cropped to the 16:9 picture.
    fn screenshot(&mut self, path: &Path) {
        let (x, y, w, h) = fit_viewport(self.drawable.0, self.drawable.1);
        let pixels = self.renderer.gl().read_rgba(x, y, w, h);
        match capture::save_gl_frame(path, w as u32, h as u32, &pixels) {
            Ok(()) => info!("screenshot {w}x{h} saved to {}", path.display()),
            Err(e) => info!("screenshot to {} failed: {e}", path.display()),
        }
    }

    fn swap(&mut self) -> Frame {
        let swap_from = Instant::now();
        // SAFETY: the window is live and its context is current.
        unsafe { self.surface.sdl.gl_swap_window(self.surface.window) };
        let shown = Instant::now();
        // The main thread's page faults so far: `FrameStats::frame` sets the
        // split's to the change since the last frame.
        let faults = faults::thread_faults();
        let p = self.painted;
        Frame {
            shown,
            split: FrameSplit {
                batch: p.upload_from.saturating_duration_since(p.batch_from),
                upload: p.submit_from.saturating_duration_since(p.upload_from),
                submit: p.submitted.saturating_duration_since(p.submit_from),
                swap: shown.saturating_duration_since(swap_from),
                uploads: p.uploads,
                upload_bytes: p.upload_bytes,
                glyphs: self.assets.text.take_rasterised(),
                faults: None,
            },
            faults,
            quads: self.batch.quad_count(),
        }
    }

    fn take_icons(&mut self) -> bool {
        self.assets.icons.receive()
    }

    fn icons_loading(&self) -> bool {
        self.assets.icons.any_loading()
    }

    /// Makes every title's glyphs (`draw::warm`) and uploads them, then logs
    /// what it made and how long that and the uploads took.
    fn warm(&mut self, apps: &[App]) {
        let from = Instant::now();
        draw::warm::warm(apps, &mut self.assets);
        let uploads = self.assets.atlas.take_uploads();
        self.renderer.upload(&uploads);
        let glyphs = self.assets.text.take_rasterised();
        info!("{}", draw::warm::line(glyphs, &uploads, from.elapsed()));
    }

    fn refresh(&mut self) -> Refresh {
        // SAFETY: the window lives as long as the surface.
        unsafe { self.surface.sdl.refresh(self.surface.window) }
    }

    fn gl_error(&mut self) -> u32 {
        self.renderer.gl().error()
    }
}
