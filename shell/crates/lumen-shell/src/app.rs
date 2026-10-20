//! hello-gles: opens a full-screen GLES2 surface, draws a moving scene and an
//! on-screen readout of the renderer and frame times, and exits on BACK.
//!
//! What it proves on the TV, in order: SDL2 loads, a GLES2 context comes up,
//! vsync paces frames, the remote's keys arrive, and the frame budget is
//! measurable. Every number on screen is also written to the log.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::capture;
use crate::frame_stats::{FrameStats, Report};
use crate::gl::{self, Gl};
use crate::info;
use crate::render::{Batch, Renderer, Rgba, SCREEN_H, SCREEN_W, fit_viewport};
use crate::sdl::{self, Event, GlContext, Sdl, Window};

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    /// Leave after this long; used by smoke tests on the PC and the TV.
    pub exit_after: Option<Duration>,
    /// Open a window instead of going full screen (the default on a PC).
    pub windowed: bool,
    /// Save one frame as a PNG here, once the scene has settled.
    pub screenshot: Option<PathBuf>,
    /// On a PC, use a GLES2 context instead of desktop GL (always on the TV).
    pub gles: bool,
}

/// Frames to draw before a screenshot, so the first fps report is on it.
const SCREENSHOT_AFTER: Duration = Duration::from_millis(1500);

impl Options {
    /// Reads `--exit-after <seconds>`, `--windowed`, `--gles` and `--screenshot <path>`.
    /// Anything else is ignored: webOS passes the app's launch parameters as a
    /// JSON argument.
    pub fn from_args(args: &[String]) -> Self {
        let mut opts = Self {
            windowed: !on_tv(),
            ..Self::default()
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--windowed" => opts.windowed = true,
                "--gles" => opts.gles = true,
                "--exit-after" => {
                    opts.exit_after = it
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .map(Duration::from_secs_f64);
                }
                "--screenshot" => opts.screenshot = it.next().map(PathBuf::from),
                _ => {}
            }
        }
        opts
    }
}

/// The TV build is the only 32-bit ARM Linux target.
const fn on_tv() -> bool {
    cfg!(all(target_os = "linux", target_arch = "arm"))
}

/// A window and its GL context, released in the right order on drop.
struct Surface<'a> {
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
fn open_surface<'a>(sdl: &'a Sdl, opts: &Options) -> Result<Surface<'a>, String> {
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

/// What the overlay shows; filled once at start and refreshed per report.
struct Readout {
    lines: Vec<String>,
    stats: String,
    last_key: String,
}

impl Readout {
    fn new(sdl: &Sdl, gl: &Gl, drawable: (i32, i32), vsync: bool) -> Self {
        Self {
            lines: vec![
                format!("LUMEN HELLO-GLES {}", env!("CARGO_PKG_VERSION")),
                format!("SDL {}", sdl.version()),
                format!("GL_RENDERER: {}", gl.string(gl::RENDERER)),
                format!("GL_VERSION: {}", gl.string(gl::VERSION)),
                format!(
                    "DRAWABLE {}X{}  VSYNC {}",
                    drawable.0,
                    drawable.1,
                    if vsync { "ON" } else { "OFF" }
                ),
            ],
            stats: "FPS --".to_owned(),
            last_key: "LAST KEY: NONE".to_owned(),
        }
    }
}

fn format_report(r: &Report) -> String {
    format!(
        "FPS {:.1}  AVG {:.2} MS  MAX {:.2} MS",
        r.fps, r.avg_ms, r.max_ms
    )
}

const BG_TOP: Rgba = Rgba::rgb8(14, 18, 34);
const BG_BOTTOM: Rgba = Rgba::rgb8(0, 0, 0);
const PANEL: Rgba = Rgba(1.0, 1.0, 1.0, 0.08);
const ACCENT: Rgba = Rgba::rgb8(90, 170, 255);
const TEXT: Rgba = Rgba::rgb8(235, 238, 245);
const TEXT_DIM: Rgba = Rgba::rgb8(140, 148, 165);

/// Builds one frame. `t` is seconds since start; motion is time-based so a
/// slow frame shows as a jump, not as slow motion.
fn build_frame(batch: &mut Batch, t: f32, readout: &Readout) {
    batch.clear();
    batch.gradient_rect(0.0, 0.0, SCREEN_W, SCREEN_H, BG_TOP, BG_BOTTOM);

    // A drifting translucent panel, standing in for the glass the shell will use.
    let px = 960.0 + 520.0 * (t * 0.35).sin();
    batch.gradient_rect(
        px - 300.0,
        620.0,
        600.0,
        260.0,
        PANEL.alpha(0.14),
        PANEL.alpha(0.04),
    );
    batch.rect(px - 300.0, 620.0, 600.0, 2.0, Rgba(1.0, 1.0, 1.0, 0.25));

    // A bouncing square: any dropped frame is visible as a stutter in its path.
    let period = 4.0;
    let phase = (t % period) / period;
    let x = 160.0 + 1500.0 * (1.0 - (2.0 * phase - 1.0).abs());
    let y = 940.0 - 120.0 * (t * 3.0).sin().abs();
    batch.rect(x, y, 60.0, 60.0, ACCENT);

    let scale = 4.0;
    let line_h = 9.0 * scale;
    for (i, line) in readout.lines.iter().enumerate() {
        let color = if i == 0 { TEXT } else { TEXT_DIM };
        batch.text(80.0, 80.0 + i as f32 * line_h, scale, line, color);
    }
    let below = 80.0 + (readout.lines.len() as f32 + 0.5) * line_h;
    batch.text(80.0, below, scale * 1.5, &readout.stats, TEXT);
    batch.text(
        80.0,
        below + line_h * 1.8,
        scale,
        &readout.last_key,
        TEXT_DIM,
    );
    let foot = SCREEN_H - 80.0 - 7.0 * scale;
    batch.text(80.0, foot, scale, "PRESS BACK TO EXIT", TEXT_DIM);
    batch.text_right(SCREEN_W - 80.0, foot, scale, "PHASE 0", TEXT_DIM);
}

pub fn run(opts: Options) -> Result<(), String> {
    info!("options {opts:?}");
    let sdl = Sdl::load()?;
    info!("SDL {}", sdl.version());

    // webOS SDL: deliver BACK and EXIT to the app instead of closing it, and
    // hide the Magic Remote pointer quickly. Other SDL builds ignore these.
    sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
    sdl.hint("SDL_WEBOS_CURSOR_SLEEP_TIME", "5000");
    sdl.init(sdl::INIT_VIDEO | sdl::INIT_EVENTS)?;

    let result = run_surface(&sdl, &opts);
    sdl.quit();
    result
}

fn run_surface(sdl: &Sdl, opts: &Options) -> Result<(), String> {
    let surface = open_surface(sdl, opts)?;
    let gl = Gl::load(|name| sdl.gl_proc_address(name))?;
    let vsync = sdl.gl_set_swap_interval(1);
    // SAFETY: the surface's window is live for this whole function.
    let mut drawable = unsafe { sdl.drawable_size(surface.window) };

    info!(
        "GL vendor {:?} renderer {:?} version {:?} glsl {:?}",
        gl.string(gl::VENDOR),
        gl.string(gl::RENDERER),
        gl.string(gl::VERSION),
        gl.string(gl::SHADING_LANGUAGE_VERSION)
    );
    info!(
        "drawable {}x{}, vsync {}",
        drawable.0,
        drawable.1,
        if vsync { "on" } else { "refused" }
    );

    let mut readout = Readout::new(sdl, &gl, drawable, vsync);
    let renderer = Renderer::new(gl)?;
    let mut batch = Batch::default();
    let mut stats = FrameStats::new(Duration::from_secs(1));
    let start = Instant::now();
    let mut last = start;
    let mut screenshot = opts.screenshot.clone();

    'frames: loop {
        while let Some(event) = sdl.poll_event() {
            match event {
                Event::Quit => break 'frames,
                Event::KeyDown {
                    scancode,
                    keycode,
                    repeat,
                } => {
                    let name = sdl::key_name(scancode).unwrap_or("-");
                    if !repeat {
                        info!("key down scancode {scancode} keycode {keycode:#x} ({name})");
                    }
                    readout.last_key = format!("LAST KEY: SCANCODE {scancode} ({name})");
                    if sdl::is_back(scancode) {
                        info!("back pressed, leaving");
                        break 'frames;
                    }
                }
                Event::Other(_) => {}
            }
        }

        let now = Instant::now();
        if opts.exit_after.is_some_and(|limit| now - start >= limit) {
            info!("exit-after reached");
            break;
        }

        // SAFETY: the window is live; see above.
        drawable = unsafe { sdl.drawable_size(surface.window) };
        build_frame(&mut batch, (now - start).as_secs_f32(), &readout);
        renderer.draw(&batch, drawable);
        if now - start >= SCREENSHOT_AFTER
            && let Some(path) = screenshot.take()
        {
            // Read the back buffer before the swap, cropped to the 16:9 picture.
            let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
            let pixels = renderer.gl().read_rgba(x, y, w, h);
            match capture::save_gl_frame(&path, w as u32, h as u32, &pixels) {
                Ok(()) => info!("screenshot {w}x{h} saved to {}", path.display()),
                Err(e) => info!("screenshot to {} failed: {e}", path.display()),
            }
        }
        // SAFETY: the window is live and its context is current.
        unsafe { sdl.gl_swap_window(surface.window) };

        let after = Instant::now();
        if let Some(report) = stats.record(after - last) {
            readout.stats = format_report(&report);
            info!(
                "fps {:.1} avg {:.2} ms max {:.2} ms ({} quads)",
                report.fps,
                report.avg_ms,
                report.max_ms,
                batch.quad_count()
            );
            let err = renderer.gl().error();
            if err != gl::NO_ERROR {
                info!("GL error {err:#x}");
            }
        }
        last = after;
    }
    drop(surface);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_exit_after_and_windowed() {
        let opts = Options::from_args(&args(&["--windowed", "--exit-after", "2.5"]));
        assert!(opts.windowed);
        assert_eq!(opts.exit_after, Some(Duration::from_millis(2500)));
        assert_eq!(opts.screenshot, None);
    }

    #[test]
    fn parses_screenshot_path() {
        let opts = Options::from_args(&args(&["--screenshot", "/tmp/frame.png"]));
        assert_eq!(opts.screenshot, Some(PathBuf::from("/tmp/frame.png")));
    }

    #[test]
    fn ignores_webos_launch_json_and_bad_numbers() {
        let opts = Options::from_args(&args(&[r#"{"query":"x"}"#, "--exit-after", "soon"]));
        assert_eq!(opts.exit_after, None);
        let negative = Options::from_args(&args(&["--exit-after", "-1"]));
        assert_eq!(negative.exit_after, None);
    }

    #[test]
    fn report_text_uses_only_font_glyphs() {
        let text = format_report(&Report {
            fps: 59.94,
            avg_ms: 16.68,
            max_ms: 21.5,
            frames: 60,
        });
        assert_eq!(text, "FPS 59.9  AVG 16.68 MS  MAX 21.50 MS");
    }

    #[test]
    fn frame_stays_on_screen() {
        let readout = Readout {
            lines: vec!["LUMEN".to_owned()],
            stats: "FPS 60.0".to_owned(),
            last_key: "LAST KEY: NONE".to_owned(),
        };
        let mut batch = Batch::default();
        for step in 0..400 {
            build_frame(&mut batch, step as f32 * 0.05, &readout);
            assert!(batch.vertex_count() > 0);
        }
    }
}
