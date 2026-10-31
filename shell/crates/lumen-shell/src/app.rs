//! The shell's start-up: its options, SDL and its hints, the window and its
//! GL context, the renderer and the assets, the apps and the luna watch. The
//! home loop (`home_loop`) runs the rest, on the TV's screen (`screen.rs`)
//! and SDL's events.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::{Launcher, MockLauncher};
use lumen_core::script::Script;

use crate::draw;
use crate::gl::{self, Gl};
use crate::home_loop::{self, Start, SystemClock, WallClock};
use crate::info;
use crate::luna::{self, Ls2, LunaLauncher};
use crate::render::Renderer;
use crate::screen::{self, TvScreen};
use crate::sdl::{self, Sdl};
use crate::timing::{Phase, StartTimes};
use crate::watch;

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

    let result = run_surface(&sdl, &opts, start);
    sdl.quit();
    result
}

/// Start-up from the window on, then the home loop, which gets the window,
/// GL and what fills the frames as its screen, and SDL's queue as its events.
fn run_surface(sdl: &Sdl, opts: &Options, mut start: StartTimes) -> Result<(), String> {
    let surface = screen::open_surface(sdl, opts)?;
    let gl = Gl::load(|name| sdl.gl_proc_address(name))?;
    let vsync = sdl.gl_set_swap_interval(1);
    let drawable = surface.drawable_size();
    start.mark(Phase::Gl, Instant::now());

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

    let renderer = Renderer::new(gl)?;
    start.mark(Phase::Renderer, Instant::now());
    // A fixed clock wants the same frames every run, so its icons load at once.
    let assets = if opts.fixed_dt.is_some() {
        draw::Assets::new()?
    } else {
        draw::Assets::threaded()?
    };
    start.mark(Phase::Assets, Instant::now());
    // Where the loop's clock starts.
    let zero = SystemClock.now();
    if let Some(step) = opts.fixed_dt {
        info!("fixed clock: {:.3} ms a frame", step.as_secs_f64() * 1000.0);
    }
    let mut launcher = launcher();
    let listing = SystemClock.now();
    let apps = launcher.list();
    info!("{} apps", apps.as_ref().map_or(0, Vec::len));
    if let Err(e) = &apps {
        info!("listing apps failed: {e}");
    }
    start.mark(Phase::Apps, Instant::now());
    let shared = watch::Shared::new();
    // The luna watch (power state, the app in front, screensaver requests)
    // runs on its own thread for the life of the loop and stops when this
    // drops, after the loop. Nothing to watch off the TV, and nothing under
    // a fixed clock, whose "last input" times are not real time. It counts
    // "last input" from the moment the heartbeat does: set here, before it
    // starts, and to the same again by the home loop's `Shell::new`.
    let _watch = (on_tv() && opts.fixed_dt.is_none()).then(|| {
        shared.set_last_input(listing.saturating_duration_since(zero));
        watch::start(Arc::clone(&shared), luna::SELF_ID, zero)
    });
    // After the watch, so the window and its context go before it stops.
    let mut screen = TvScreen::new(surface, renderer, assets);
    let mut events = sdl;
    home_loop::run(
        &mut events,
        &mut screen,
        &SystemClock,
        opts,
        Start {
            times: start,
            zero,
            listing,
            launcher,
            apps,
            shared,
            vsync,
        },
    );
    Ok(())
}

/// The TV's apps through luna; the simulator's mock list on a PC, or on the
/// TV with `LUMEN_LAUNCHER=mock` (a smoke test that touches no other app).
fn launcher() -> Box<dyn Launcher> {
    let mock = std::env::var("LUMEN_LAUNCHER").is_ok_and(|v| v == "mock");
    if on_tv() && !mock {
        info!("apps from luna");
        Box::new(LunaLauncher::new(Ls2::system()))
    } else {
        Box::new(mock_launcher())
    }
}

/// The simulator's apps and LG Home's tile, as the TV's launcher adds it,
/// with its placeholder icons if `LUMEN_MOCK_ICONS` names their folder
/// (`tools/sim.ps1` sets it). The folder is resolved first, since an icon is
/// read only at a path that resolves to itself.
fn mock_launcher() -> MockLauncher {
    let dir = std::env::var_os("LUMEN_MOCK_ICONS")
        .map(PathBuf::from)
        .map(|dir| std::fs::canonicalize(&dir).unwrap_or(dir));
    let mock = MockLauncher::default().with_lg_home();
    match dir {
        Some(dir) if dir.is_dir() => {
            info!("mock apps, icons from {}", dir.display());
            mock.with_icons_in(&dir)
        }
        Some(dir) => {
            info!("mock apps; no icons at {}: letter tiles", dir.display());
            mock
        }
        None => {
            info!("mock apps, letter tiles");
            mock
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
