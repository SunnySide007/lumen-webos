//! The shell's start-up: its options, SDL and its hints, the window and its
//! GL context, and the apps' launcher. The home loop (`home_loop`) runs the
//! rest.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lumen_core::apps::{Launcher, MockLauncher};
use lumen_core::script::Script;

use crate::home_loop;
use crate::info;
use crate::luna::{Ls2, LunaLauncher};
use crate::render::{SCREEN_H, SCREEN_W};
use crate::sdl::{self, GlContext, Sdl, Window};
use crate::timing::{Phase, StartTimes};

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    /// Leave after this long, in real time; used by smoke tests on the PC and
    /// the TV, and as a watchdog for automated runs.
    pub exit_after: Option<Duration>,
    /// Open a window instead of going full screen (the default on a PC).
    pub windowed: bool,
    /// Save one frame as a PNG here, 1.5 s after the start.
    pub screenshot: Option<PathBuf>,
    /// On a PC, use a GLES2 context instead of desktop GL (always on the TV).
    pub gles: bool,
    /// Keys to play, through the same path as the remote's.
    pub keys: Option<Script>,
    /// A deterministic clock: each frame advances it by exactly this.
    pub fixed_dt: Option<Duration>,
    /// Once the keys are played and the screen settles, save it here and quit.
    pub screenshot_when_settled: Option<PathBuf>,
}

impl Options {
    /// Reads `--exit-after <seconds>`, `--windowed`, `--gles`,
    /// `--screenshot <path>`, `--keys <list>`, `--fixed-dt <seconds>` and
    /// `--screenshot-when-settled <path>`. Anything else is ignored: webOS
    /// passes the app's launch parameters as a JSON argument. A bad `--keys`
    /// or `--fixed-dt` is an error, since only automation uses them.
    pub fn from_args(args: &[String]) -> Result<Self, String> {
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
                        .and_then(|s| Duration::try_from_secs_f64(s).ok());
                }
                "--screenshot" => opts.screenshot = it.next().map(PathBuf::from),
                "--keys" => {
                    let list = it.next().ok_or("--keys needs a list, such as R,R,D,OK")?;
                    opts.keys = Some(Script::parse(list)?);
                }
                "--fixed-dt" => {
                    let step = it
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|s| s.is_finite() && *s > 0.0 && *s <= 1.0)
                        .ok_or("--fixed-dt needs seconds per frame, such as 0.016667")?;
                    opts.fixed_dt = Some(Duration::from_secs_f64(step));
                }
                "--screenshot-when-settled" => {
                    let path = it.next().ok_or("--screenshot-when-settled needs a path")?;
                    opts.screenshot_when_settled = Some(PathBuf::from(path));
                }
                _ => {}
            }
        }
        Ok(opts)
    }
}

/// The TV build is the only 32-bit ARM Linux target.
pub(crate) const fn on_tv() -> bool {
    cfg!(all(target_os = "linux", target_arch = "arm"))
}

/// A window and its GL context, released in the right order on drop.
pub(crate) struct Surface<'a> {
    sdl: &'a Sdl,
    pub(crate) window: Window,
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
pub(crate) fn open_surface<'a>(sdl: &'a Sdl, opts: &Options) -> Result<Surface<'a>, String> {
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

/// Runs the shell. `started` is `main()`'s first moment, where the start-up
/// line counts from.
pub fn run(opts: Options, started: Instant) -> Result<(), String> {
    info!("options {opts:?}");
    let mut start = StartTimes::new(started);
    let sdl = Sdl::load()?;
    info!("SDL {}", sdl.version());
    if on_tv() {
        info!("{}", sdl.pointer_line());
    }

    // webOS SDL: deliver BACK and EXIT to the app instead of closing it, and
    // hide the Magic Remote pointer quickly. Other SDL builds ignore these.
    sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    sdl.hint("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
    sdl.hint("SDL_WEBOS_CURSOR_SLEEP_TIME", "5000");
    sdl.init(sdl::INIT_VIDEO | sdl::INIT_EVENTS)?;
    start.mark(Phase::Sdl, Instant::now());

    let result = home_loop::run_surface(&sdl, &opts, start);
    sdl.quit();
    result
}

/// The TV's apps through luna; the simulator's mock list on a PC, or on the
/// TV with `LUMEN_LAUNCHER=mock` (a smoke test that touches no other app).
pub(crate) fn launcher() -> Box<dyn Launcher> {
    let mock = std::env::var("LUMEN_LAUNCHER").is_ok_and(|v| v == "mock");
    if on_tv() && !mock {
        info!("apps from luna");
        Box::new(LunaLauncher::new(Ls2::system()))
    } else {
        Box::new(mock_launcher())
    }
}

/// The simulator's apps, with its placeholder icons if `LUMEN_MOCK_ICONS`
/// names their folder (`tools/sim.ps1` sets it). The folder is resolved
/// first, since an icon is read only at a path that resolves to itself.
fn mock_launcher() -> MockLauncher {
    let dir = std::env::var_os("LUMEN_MOCK_ICONS")
        .map(PathBuf::from)
        .map(|dir| std::fs::canonicalize(&dir).unwrap_or(dir));
    match dir {
        Some(dir) if dir.is_dir() => {
            info!("mock apps, icons from {}", dir.display());
            MockLauncher::default().with_icons_in(&dir)
        }
        Some(dir) => {
            info!("mock apps; no icons at {}: letter tiles", dir.display());
            MockLauncher::default()
        }
        None => {
            info!("mock apps, letter tiles");
            MockLauncher::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn parse(list: &[&str]) -> Options {
        Options::from_args(&args(list)).expect("valid arguments")
    }

    #[test]
    fn parses_exit_after_and_windowed() {
        let opts = parse(&["--windowed", "--exit-after", "2.5"]);
        assert!(opts.windowed);
        assert_eq!(opts.exit_after, Some(Duration::from_millis(2500)));
        assert_eq!(opts.screenshot, None);
    }

    #[test]
    fn parses_screenshot_path() {
        let opts = parse(&["--screenshot", "/tmp/frame.png"]);
        assert_eq!(opts.screenshot, Some(PathBuf::from("/tmp/frame.png")));
    }

    #[test]
    fn ignores_webos_launch_json_and_bad_numbers() {
        let opts = parse(&[r#"{"query":"x"}"#, "--exit-after", "soon"]);
        assert_eq!(opts.exit_after, None);
        let negative = parse(&["--exit-after", "-1"]);
        assert_eq!(negative.exit_after, None);
    }

    #[test]
    fn parses_the_automation_flags() {
        let opts = parse(&[
            "--keys",
            "R,R,D,OK",
            "--fixed-dt",
            "0.02",
            "--screenshot-when-settled",
            "/tmp/settled.png",
        ]);
        assert_eq!(opts.keys, Some(Script::parse("R,R,D,OK").unwrap()));
        assert_eq!(opts.fixed_dt, Some(Duration::from_millis(20)));
        assert_eq!(
            opts.screenshot_when_settled,
            Some(PathBuf::from("/tmp/settled.png"))
        );
    }

    #[test]
    fn bad_automation_flags_are_errors() {
        for bad in [
            &["--keys", "R,HOME"][..],
            &["--keys"],
            &["--fixed-dt", "0"],
            &["--fixed-dt", "fast"],
            &["--fixed-dt", "5"],
            &["--screenshot-when-settled"],
        ] {
            assert!(Options::from_args(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
