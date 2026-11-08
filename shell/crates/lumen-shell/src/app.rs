//! The shell's start-up: its options, the platform (LG's SDL, or the
//! platform layer's own window on the compositor, `platform`), the window
//! and its GL context, the renderer and the assets, the apps and the luna
//! watch. The home loop (`home_loop`) runs the rest, on the TV's screen
//! (`screen.rs`) and the platform's events.

use std::ffi::{CStr, c_void};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::{AppId, Launcher, MockLauncher};
use lumen_core::art::Look;
use lumen_core::control::Reduced;
use lumen_core::script::Script;

use crate::draw;
use crate::gl::{self, Gl};
use crate::heroes::{Folders, Heroes};
use crate::home_loop::{self, Events, Start, SystemClock, WallClock};
use crate::info;
use crate::local_time::TimeOfDay;
use crate::luna::{self, Ls2, LunaLauncher};
use crate::platform::launch::{Launch, Params, window_type};
use crate::platform::lifecycle::Method;
use crate::platform::{Platform, probe};
use crate::render::Renderer;
use crate::screen::{self, TvScreen, Window};
use crate::sdl::{self, Sdl};
use crate::timing::{Phase, StartTimes};
use crate::watch;

/// The file beside the executable that may name the platform.
const PLATFORM_FILE: &str = "platform";
/// The file beside the executable that keeps Reduce Motion: the words of
/// the effects the owner turned off in the Control Center, on one line.
/// Reinstalling the IPK removes it, as it does `platform`.
const MOTION_FILE: &str = "reduce-motion";

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    /// Leave after this long, in real time; used by smoke tests on the PC and
    /// the TV, and as a watchdog for automated runs.
    pub exit_after: Option<Duration>,
    /// Open a window instead of going full screen (the default on a PC).
    pub windowed: bool,
    /// Save one frame as a PNG here, 1.5 s after the start.
    pub screenshot: Option<PathBuf>,
    /// When, on the loop's clock, `screenshot` takes its frame instead: with
    /// `--fixed-dt` and `--keys`, a frame mid-motion.
    pub screenshot_at: Option<Duration>,
    /// On a PC, use a GLES2 context instead of desktop GL (always on the TV).
    pub gles: bool,
    /// Keys to play, through the same path as the remote's.
    pub keys: Option<Script>,
    /// A deterministic clock: each frame advances it by exactly this.
    pub fixed_dt: Option<Duration>,
    /// Once the keys are played and the screen settles, save it here and quit.
    pub screenshot_when_settled: Option<PathBuf>,
    /// The launch parameter `"shots"`: at each time on the loop's clock, the
    /// first settled frame from then on is saved as
    /// `lumen-shot-<ms>.png` beside the log, and the shell goes on.
    pub shots: Vec<Duration>,
    /// The launch parameter `"saver": "hold"`: the luna watch answers LG's
    /// screensaver requests `ack: false` (a bench run's lab mode).
    pub hold_saver: bool,
    /// The look's features: all on unless the launch parameters say.
    pub look: Look,
    /// What the launch parameters' `"look"` said that names no feature:
    /// left out, and logged.
    pub look_unknown: Vec<String>,
    /// The window and event layer, if the launch parameters or `--platform`
    /// name one; else `LUMEN_PLATFORM`, the `platform` file beside the
    /// executable, or SDL (`choose_platform`).
    pub platform: Option<Platform>,
    /// An ES 3.0 context on the Wayland path (`"gles": 3`, `--gles3`).
    pub gles3: bool,
    /// Measure the GPU plan's gate and leave (`"probe": "gpu"`, `--probe`).
    pub probe: bool,
    /// The overlay trial's window type (`"window": "overlay"`, `--window`).
    pub window_type: Option<String>,
    /// Start hidden until a relaunch shows the shell: SAM's preload,
    /// `"hidden": true` or `--hidden`. The Wayland path only.
    pub hidden: bool,
    /// The registration SAM asked for in a version 2 launch envelope;
    /// `None` is version 1's `registerNativeApp`.
    pub lifecycle: Option<Method>,
    /// Lines about the launch argument for the log: what was left out, and
    /// SAM's reason for a version 2 launch.
    pub launch_lines: Vec<String>,
    /// The clock in the corner, fixed (`--clock HH:MM`), for screenshots. A
    /// fixed clock (`--fixed-dt`) shows none without it, so its frames stay
    /// the same; otherwise it is the local time.
    pub clock: Option<TimeOfDay>,
    /// What the home says was playing at the start (`--playing <app id>`),
    /// for the simulator, which has no luna watch to say it.
    pub playing: Option<AppId>,
    /// Reduce Motion, as kept in `MOTION_FILE` (read by `run`).
    pub reduced: Reduced,
    /// Where Reduce Motion is kept; `None`: its changes last this run only.
    pub motion_file: Option<PathBuf>,
}

impl Options {
    /// Reads `--exit-after <seconds>`, `--windowed`, `--gles`,
    /// `--screenshot <path>`, `--screenshot-at <seconds>`, `--keys <list>`,
    /// `--fixed-dt <seconds>`, `--screenshot-when-settled <path>`,
    /// `--platform <sdl|wayland>`, `--gles3`, `--probe`, `--hidden`,
    /// `--window <type>`, `--clock <HH:MM>` and `--playing <app id>`, and
    /// the launch argument webOS passes as JSON
    /// (`read_launch`). Anything else is ignored. A bad flag is an error,
    /// since only automation and the owner's trials use them; a bad launch
    /// parameter is not, since it must never keep the TV's Home from
    /// starting.
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
                "--gles3" => opts.gles3 = true,
                "--probe" => opts.probe = true,
                "--hidden" => opts.hidden = true,
                "--platform" => {
                    let name = it.next().ok_or("--platform needs sdl or wayland")?;
                    let platform = Platform::from_name(name)
                        .ok_or_else(|| format!("--platform {name:?}: sdl or wayland"))?;
                    opts.platform = Some(platform);
                }
                "--window" => {
                    let name = it
                        .next()
                        .ok_or("--window needs a window type, such as overlay")?;
                    let kind = window_type(name).ok_or_else(|| {
                        format!("--window {name:?}: letters and underscores only")
                    })?;
                    opts.window_type = Some(kind);
                }
                "--exit-after" => {
                    opts.exit_after = it
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .and_then(|s| Duration::try_from_secs_f64(s).ok());
                }
                "--screenshot" => opts.screenshot = it.next().map(PathBuf::from),
                "--screenshot-at" => {
                    let at = it
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|s| s.is_finite() && *s >= 0.0 && *s <= 600.0)
                        .ok_or("--screenshot-at needs seconds on the loop's clock, such as 0.62")?;
                    opts.screenshot_at = Some(Duration::from_secs_f64(at));
                }
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
                "--clock" => {
                    let time = it.next().and_then(|t| TimeOfDay::parse(t));
                    opts.clock = Some(time.ok_or("--clock needs a time of day, such as 9:41")?);
                }
                "--playing" => {
                    let id = it
                        .next()
                        .filter(|id| !id.is_empty() && !id.starts_with('-'));
                    opts.playing = Some(AppId::new(id.ok_or("--playing needs an app id")?));
                }
                json if json.starts_with('{') => opts.read_launch(json),
                _ => {}
            }
        }
        Ok(opts)
    }

    /// The launch argument (`platform::launch`), in either lifecycle
    /// version's shape: its `"look"`, such as `{"look":"plain"}` or
    /// `{"look":"-dither,-tilt"}` (`Look::parse`), the platform layer's
    /// parameters (`Params`), and SAM's preload and registration method.
    /// An argument that is not a JSON object changes nothing.
    fn read_launch(&mut self, json: &str) {
        let Ok(serde_json::Value::Object(top)) = serde_json::from_str(json) else {
            return;
        };
        let launch = Launch::read(&top);
        match launch.params.get("look") {
            Some(serde_json::Value::String(text)) => {
                (self.look, self.look_unknown) = Look::parse(text);
            }
            Some(other) => {
                self.look = Look::DEFAULT;
                self.look_unknown = vec![other.to_string()];
            }
            None => {}
        }
        let params = Params::read(&launch.params);
        self.platform = params.platform.or(self.platform);
        self.gles3 |= params.gles3;
        self.probe |= params.probe;
        self.window_type = params.window_type.or(self.window_type.take());
        self.exit_after = params.exit_after.or(self.exit_after);
        self.hidden |= params.hidden || launch.preloaded();
        self.hold_saver |= params.hold_saver;
        if !params.shots.is_empty() {
            self.shots = params.shots;
        }
        self.lifecycle = launch.method;
        self.launch_lines.extend(params.refused);
        if let Some(reason) = launch.reason {
            self.launch_lines
                .push(format!("launch: lifecycle version 2, reason {reason:?}"));
        }
    }
}

/// The TV build is the only 32-bit ARM Linux target.
pub(crate) const fn on_tv() -> bool {
    cfg!(all(target_os = "linux", target_arch = "arm"))
}

/// The platform, and what chose it: the options (the launch parameters or
/// `--platform`), else `LUMEN_PLATFORM`, else the `platform` file beside the
/// executable (one word; reinstalling the IPK removes it), else SDL.
fn choose_platform(opts: &Options) -> (Platform, String) {
    if let Some(platform) = opts.platform {
        return (platform, "the options".to_owned());
    }
    if let Ok(name) = std::env::var("LUMEN_PLATFORM") {
        match Platform::from_name(&name) {
            Some(platform) => return (platform, "LUMEN_PLATFORM".to_owned()),
            None => info!("LUMEN_PLATFORM={name:?} is not sdl or wayland; left out"),
        }
    }
    let file = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(PLATFORM_FILE)));
    if let Some(file) = file.filter(|f| f.is_file()) {
        match read_platform_file(&file) {
            Some(platform) => return (platform, file.display().to_string()),
            None => info!("{}: not one word, sdl or wayland; left out", file.display()),
        }
    }
    (Platform::Sdl, "the default".to_owned())
}

/// What a short file of Reduce Motion's words turns off; a missing or
/// unreadable file turns off nothing.
fn read_motion_file(path: &std::path::Path) -> Reduced {
    use std::io::Read;
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        // Not text: nothing.
        let _ = file.take(256).read_to_string(&mut text);
    }
    Reduced::from_words(&text)
}

/// The platform a short file names, or `None`.
fn read_platform_file(path: &std::path::Path) -> Option<Platform> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(32)
        .read_to_string(&mut text)
        .ok()?;
    Platform::from_name(&text)
}

/// Runs the shell. `started` is `main()`'s first moment, where the start-up
/// line counts from.
pub fn run(mut opts: Options, started: Instant) -> Result<(), String> {
    info!("options {opts:?}");
    info!("look: {}", opts.look);
    for word in &opts.look_unknown {
        info!("look: {word:?} is not a feature; left out");
    }
    // A fixed clock's runs (tests, screenshots) neither read nor keep it, so
    // one run's switch never changes the next run's frames.
    if opts.motion_file.is_none() && opts.fixed_dt.is_none() {
        opts.motion_file = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(MOTION_FILE)));
    }
    if let Some(file) = &opts.motion_file {
        opts.reduced = read_motion_file(file);
    }
    info!("{}", opts.reduced);
    for line in &opts.launch_lines {
        info!("{line}");
    }
    let (platform, from) = choose_platform(&opts);
    info!("platform: {} (from {from})", platform.name());
    if platform == Platform::Wayland {
        match run_wayland(&opts, started) {
            Some(result) => return result,
            None => info!("platform: the Wayland path did not start; sdl instead"),
        }
    }
    if std::mem::take(&mut opts.hidden) {
        info!("a hidden start needs the Wayland path; starting shown");
    }
    if opts.gles3 {
        info!("an ES 3 context is the Wayland path's; SDL's stays as it was");
    }
    run_sdl(&opts, started)
}

/// The SDL path: LG's SDL 2.0.5 on the TV, the official SDL2 on a PC.
fn run_sdl(opts: &Options, started: Instant) -> Result<(), String> {
    let mut start = StartTimes::new(started);
    start.name_first(Platform::Sdl.name());
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

    let result = sdl_surface(&sdl, opts, start);
    sdl.quit();
    result
}

/// The SDL path's window, its GL context and vsync, then the shared
/// start-up and the loop, on SDL's queue.
fn sdl_surface(sdl: &Sdl, opts: &Options, start: StartTimes) -> Result<(), String> {
    let surface = screen::open_surface(sdl, opts)?;
    let vsync = sdl.gl_set_swap_interval(1);
    // The TV and `--gles` have ES 2; a PC's desktop GL is "0" to the probe.
    let es = if on_tv() || opts.gles { 2 } else { 0 };
    let mut events = sdl;
    run_home(
        surface,
        &|name| sdl.gl_proc_address(name),
        vsync,
        es,
        opts,
        start,
        &mut events,
    )
}

/// The Wayland path (decision 15): the compositor, EGL and the shell's own
/// lifecycle registration. `None` when it could not start, before anything
/// was shown, so the SDL path can run instead; the reason is logged.
#[cfg(target_os = "linux")]
fn run_wayland(opts: &Options, started: Instant) -> Option<Result<(), String>> {
    use crate::platform::egl::Egl;
    use crate::platform::lifecycle;
    use crate::platform::wayland::{Setup, Wayland};

    /// The compositor's window type for an app: a card, which it makes full
    /// screen.
    const CARD: &str = "_WEBOS_WINDOW_TYPE_CARD";

    let mut start = StartTimes::new(started);
    start.name_first(Platform::Wayland.name());
    let app_id = std::env::var("APPID").unwrap_or_else(|_| luna::SELF_ID.to_owned());
    let display_id = std::env::var("DISPLAY_ID").unwrap_or_else(|_| "0".to_owned());
    let window_type = opts.window_type.as_deref().unwrap_or(CARD);
    if window_type != CARD {
        info!(
            "trial: window type {window_type} instead of a card; the compositor's answer \
             follows (a protocol error, no window, or the window)"
        );
    }
    let wl = match Wayland::connect(&Setup {
        app_id: &app_id,
        window_type,
        display_id: &display_id,
    }) {
        Ok(wl) => wl,
        Err(e) => {
            info!("wayland: {e}");
            return None;
        }
    };
    start.mark(Phase::Sdl, Instant::now());
    let egl = match Egl::load() {
        Ok(egl) => egl,
        Err(e) => {
            info!("egl: {e}");
            return None;
        }
    };
    let (window, made) = match wl.open_egl(egl, opts.gles3 || opts.probe) {
        Ok(opened) => opened,
        Err(e) => {
            info!("egl: {e}");
            return None;
        }
    };
    info!("egl: {}", egl.files);
    info!("{}", made.egl_line);
    info!("{}", made.config_line);
    info!("{}", wl.pointer_line());
    // SAM's relaunch, pause and close for this process, from here until the
    // loop ends; luna is the TV's only.
    let _lifecycle = on_tv().then(|| {
        let method = opts.lifecycle.unwrap_or(Method::RegisterNativeApp);
        lifecycle::start(method, &app_id, wl.lifecycle_sink())
    });
    let mut events = &wl;
    let loader = move |name: &CStr| egl.proc_address(name);
    Some(run_home(
        window,
        &loader,
        made.vsync,
        made.es,
        opts,
        start,
        &mut events,
    ))
}

#[cfg(not(target_os = "linux"))]
fn run_wayland(_: &Options, _: Instant) -> Option<Result<(), String>> {
    info!("platform: the Wayland path is the TV's, on Linux only");
    None
}

/// Start-up from GL on, the same on both platforms, then the home loop,
/// which gets `window` and what fills the frames as its screen, and
/// `events`. `loader` finds GL's functions; `es` is the context's ES
/// version (0: desktop GL), for the probe.
fn run_home<W: Window, E: Events>(
    window: W,
    loader: &dyn Fn(&CStr) -> *const c_void,
    vsync: bool,
    es: u8,
    opts: &Options,
    mut start: StartTimes,
    events: &mut E,
) -> Result<(), String> {
    let gl = Gl::load(loader)?;
    let drawable = window.drawable_size();
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

    let mut renderer = Renderer::new(gl, opts.look.dither)?;
    start.mark(Phase::Renderer, Instant::now());
    // The GPU plan's gate: measured, logged, and nothing else.
    if opts.probe {
        let art = art_folders().art;
        for line in probe::run(&mut renderer, loader, &art, es, drawable) {
            info!("{line}");
        }
        info!("probe: done; leaving");
        return Ok(());
    }
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
        watch::start(Arc::clone(&shared), luna::SELF_ID, zero, opts.hold_saver)
    });
    // Under a fixed clock heroes load at once, like its icons.
    let folders = art_folders();
    info!(
        "art from {}, the owner's from {}",
        folders.art.display(),
        folders.owner.display()
    );
    let heroes = if opts.fixed_dt.is_some() {
        Heroes::at_once(folders, opts.look)
    } else {
        Heroes::threaded(folders, opts.look)
    };
    // After the watch, so the window and its context go before it stops.
    let mut screen = TvScreen::new(window, renderer, assets, heroes);
    home_loop::run(
        events,
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

/// Where the Top Shelf heroes are: `art` and `art-owner` beside the
/// executable, as the IPK installs them, or `LUMEN_ART` and
/// `LUMEN_ART_OWNER` (`tools/sim.ps1` sets both to the checkout's folders).
fn art_folders() -> Folders {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    let folder = |var: &str, name: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .unwrap_or_else(|| beside.join(name))
    };
    Folders {
        art: folder("LUMEN_ART", "art"),
        owner: folder("LUMEN_ART_OWNER", "art-owner"),
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
    let mock = match dir {
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
    };
    arranged(
        mock,
        &std::env::var("LUMEN_MOCK_FIRST").unwrap_or_default(),
        &std::env::var("LUMEN_MOCK_COLORS").unwrap_or_default(),
    )
}

/// The simulator's knobs for screenshots of a given Top Shelf hero
/// (`tools/sim.ps1 -First -Colors`): the apps `first` names (`id,id`) moved
/// to the front, onto row 0, and the colours `colors` gives (`id=rrggbb,…`).
fn arranged(mut mock: MockLauncher, first: &str, colors: &str) -> MockLauncher {
    if first.is_empty() && colors.is_empty() {
        return mock;
    }
    let Ok(mut apps) = mock.list() else {
        return mock;
    };
    for (id, hex) in colors.split(',').filter_map(|pair| pair.split_once('=')) {
        let rgb = (hex.len() == 6)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten();
        if let (Some(rgb), Some(app)) = (rgb, apps.iter_mut().find(|a| a.id.as_str() == id)) {
            let [_, r, g, b] = rgb.to_be_bytes();
            app.color = Some([r, g, b]);
        }
    }
    for id in first.split(',').rev() {
        if let Some(at) = apps.iter().position(|a| a.id.as_str() == id) {
            let app = apps.remove(at);
            apps.insert(0, app);
        }
    }
    info!("mock apps arranged: first {first:?}, colours {colors:?}");
    MockLauncher::with_apps(apps)
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
    fn the_launch_parameters_look_turns_features_off() {
        // LG's own parameters, as a Home press gives them: the default
        // look, everything but the dither.
        let home = parse(&[r#"{"@system_native_app": true, "launchType": "homeKey", "nid": "x"}"#]);
        assert_eq!((home.look, home.look_unknown.len()), (Look::DEFAULT, 0));
        assert_eq!(parse(&[r#"{"look":"full"}"#]).look, Look::FULL);
        assert_eq!(parse(&[r#"{"look":"plain"}"#]).look, Look::PLAIN);
        let some = parse(&["--windowed", r#"{"look":"-dither,-tilt"}"#]);
        assert_eq!(
            some.look,
            Look {
                dither: false,
                tilt: false,
                ..Look::FULL
            }
        );
        // A word that names nothing is left out, and kept for the log.
        let typo = parse(&[r#"{"look":"plain hero sparkle"}"#]);
        assert_eq!(
            typo.look,
            Look {
                hero: true,
                ..Look::PLAIN
            }
        );
        assert_eq!(typo.look_unknown, ["sparkle"]);
        // A look that is not text, or parameters that are not JSON, change
        // nothing and stop nothing.
        let number = parse(&[r#"{"look":3}"#]);
        assert_eq!(
            (number.look, number.look_unknown),
            (Look::DEFAULT, vec!["3".to_owned()])
        );
        assert_eq!(parse(&[r#"{"look":"plain""#]).look, Look::DEFAULT);
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
            "--screenshot-at",
            "0.62",
            "--clock",
            "9:41",
            "--playing",
            "com.webos.app.hdmi2",
        ]);
        assert_eq!(opts.screenshot_at, Some(Duration::from_millis(620)));
        assert_eq!(opts.clock, TimeOfDay::parse("09:41"));
        assert_eq!(opts.playing, Some(AppId::new("com.webos.app.hdmi2")));
        assert_eq!(opts.keys, Some(Script::parse("R,R,D,OK").unwrap()));
        assert_eq!(opts.fixed_dt, Some(Duration::from_millis(20)));
        assert_eq!(
            opts.screenshot_when_settled,
            Some(PathBuf::from("/tmp/settled.png"))
        );
    }

    #[test]
    fn the_simulators_knobs_move_and_colour_mock_apps() {
        let mut mock = arranged(
            MockLauncher::default(),
            "com.webos.app.hdmi1,nope,spotify-beehive",
            "spotify-beehive=1db954,netflix=zz0000,amazon=12",
        );
        let apps = mock.list().unwrap();
        let ids: Vec<&str> = apps.iter().take(3).map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["com.webos.app.hdmi1", "spotify-beehive", "netflix"]);
        assert_eq!(apps[1].color, Some([0x1d, 0xb9, 0x54]));
        assert!(
            apps.iter()
                .filter(|a| a.id.as_str() != "spotify-beehive")
                .all(|a| a.color.is_none())
        );
        assert_eq!(apps.len(), MockLauncher::default().list().unwrap().len());
    }

    #[test]
    fn the_platform_layers_flags_and_parameters() {
        let opts = parse(&[
            "--platform",
            "wayland",
            "--gles3",
            "--probe",
            "--hidden",
            "--window",
            "overlay",
        ]);
        assert_eq!(opts.platform, Some(Platform::Wayland));
        assert!(opts.gles3 && opts.probe && opts.hidden);
        assert_eq!(
            opts.window_type.as_deref(),
            Some("_WEBOS_WINDOW_TYPE_OVERLAY")
        );
        // The same from the launch parameters, a trial's launch.
        let launched = parse(&[
            r#"{"platform":"wayland","gles":3,"probe":"gpu","exitAfter":20,"look":"plain"}"#,
        ]);
        assert_eq!(launched.platform, Some(Platform::Wayland));
        assert!(launched.gles3 && launched.probe && !launched.hidden);
        assert_eq!(launched.exit_after, Some(Duration::from_secs(20)));
        assert_eq!(launched.look, Look::PLAIN);
        assert_eq!(launched.lifecycle, None, "version 1: registerNativeApp");
        // A Home press's own parameters choose nothing.
        let home = parse(&[r#"{"@system_native_app": true, "launchType": "homeKey"}"#]);
        assert_eq!((home.platform, home.hidden), (None, false));
    }

    #[test]
    fn a_version_2_launch_is_read_from_its_envelope() {
        let opts = parse(&[
            r#"{"event":"launch","reason":"preload","interfaceVersion":2,
                "interfaceMethod":"registerApp","preload":"full",
                "parameters":{"look":"-dither","platform":"wayland","gles":9}}"#,
        ]);
        assert_eq!(opts.lifecycle, Some(Method::RegisterApp));
        assert!(opts.hidden, "a preload starts hidden");
        assert!(!opts.look.dither);
        assert_eq!(opts.platform, Some(Platform::Wayland));
        assert_eq!(opts.launch_lines.len(), 2, "{:?}", opts.launch_lines);
        assert!(opts.launch_lines[0].starts_with("launch parameter \"gles\": 9"));
        assert_eq!(
            opts.launch_lines[1],
            "launch: lifecycle version 2, reason \"preload\""
        );
    }

    #[test]
    fn a_platform_file_names_one_platform() {
        let dir = std::env::temp_dir().join(format!("lumen-platform-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(PLATFORM_FILE);
        for (text, want) in [
            ("wayland\n", Some(Platform::Wayland)),
            (" sdl ", Some(Platform::Sdl)),
            ("wayland please", None),
            ("", None),
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(read_platform_file(&file), want, "{text:?}");
        }
        std::fs::write(&file, "x".repeat(64) + "wayland").unwrap();
        assert_eq!(read_platform_file(&file), None, "only the first bytes");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read_platform_file(&file), None, "no file");
    }

    #[test]
    fn bad_automation_flags_are_errors() {
        for bad in [
            &["--platform", "x11"][..],
            &["--platform"],
            &["--window", "over lay"],
            &["--window"],
            &["--keys", "R,HOME"][..],
            &["--keys"],
            &["--fixed-dt", "0"],
            &["--fixed-dt", "fast"],
            &["--fixed-dt", "5"],
            &["--screenshot-when-settled"],
            &["--screenshot-at", "-1"],
            &["--screenshot-at"],
            &["--clock", "25:00"],
            &["--clock"],
            &["--playing"],
            &["--playing", "--clock"],
        ] {
            assert!(Options::from_args(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
