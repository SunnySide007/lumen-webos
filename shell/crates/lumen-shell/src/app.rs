//! The home screen's main loop. It reads the remote, lets
//! `lumen_core::home::Home` decide what happens, draws only while something
//! changes or moves, and waits in `SDL_WaitEventTimeout` once the screen has
//! settled (docs/specs/shell-mvp.md, "Repainting"). Every number it used to
//! show as hello-gles (GL renderer, frame times, key codes) goes to the log.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lumen_core::apps::{Launcher, MockLauncher};
use lumen_core::home::{BackAtTop, Effect, Home};
use lumen_core::input::{Input, Press, action_for};
use lumen_core::script::Script;

use crate::capture;
use crate::draw;
use crate::frame_stats::FrameStats;
use crate::gl::{self, Gl};
use crate::info;
use crate::render::{Batch, Renderer, SCREEN_H, SCREEN_W, fit_viewport};
use crate::sdl::{self, Event, GlContext, Sdl, Window};

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

/// When `--screenshot` takes its frame.
const SCREENSHOT_AFTER: Duration = Duration::from_millis(1500);

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

/// The loop's time. Real by default; with `--fixed-dt` a virtual clock that
/// advances exactly one step per frame drawn and, while the screen is
/// settled, jumps straight to the next deadline instead of waiting for it,
/// so the same keys always give the same frames.
#[derive(Debug)]
struct Clock {
    start: Instant,
    step: Option<Duration>,
    virtual_now: Duration,
}

impl Clock {
    fn new(step: Option<Duration>) -> Self {
        Self {
            start: Instant::now(),
            step,
            virtual_now: Duration::ZERO,
        }
    }

    fn now(&self) -> Duration {
        match self.step {
            Some(_) => self.virtual_now,
            None => self.start.elapsed(),
        }
    }

    /// Real time since the start, whatever the clock: `--exit-after` uses it.
    fn real(&self) -> Duration {
        self.start.elapsed()
    }

    fn is_fixed(&self) -> bool {
        self.step.is_some()
    }

    fn frame_drawn(&mut self) {
        if let Some(step) = self.step {
            self.virtual_now += step;
        }
    }

    /// With a fixed step, moves the clock on to `t` (never back).
    fn jump_to(&mut self, t: Duration) {
        if self.is_fixed() {
            self.virtual_now = self.virtual_now.max(t);
        }
    }
}

/// The TV build is the only 32-bit ARM Linux target.
pub(crate) const fn on_tv() -> bool {
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

/// The frames drawn since the screen last settled, for one log line.
#[derive(Debug, Default)]
struct Burst {
    frames: u32,
    first: Option<Instant>,
    last: Option<Instant>,
    worst: Duration,
}

impl Burst {
    /// Counts a frame finished at `at`; returns how long it took, except for
    /// the first frame after an idle wait, which has nothing to measure from.
    fn frame(&mut self, at: Instant) -> Option<Duration> {
        let took = self.last.map(|last| at.saturating_duration_since(last));
        match took {
            Some(t) => self.worst = self.worst.max(t),
            None => self.first = Some(at),
        }
        self.last = Some(at);
        self.frames += 1;
        took
    }

    /// The log line for a finished burst, and a fresh start; `None` if no
    /// frame was drawn.
    fn finish(&mut self, quads: usize) -> Option<String> {
        let (Some(first), Some(last)) = (self.first, self.last) else {
            return None;
        };
        let span = last.saturating_duration_since(first);
        let avg = match self.frames {
            0 | 1 => Duration::ZERO,
            n => span / (n - 1),
        };
        let line = format!(
            "drew {} frames in {:.0} ms (avg {:.2} ms, worst {:.2} ms, {quads} quads); idle",
            self.frames,
            span.as_secs_f64() * 1000.0,
            avg.as_secs_f64() * 1000.0,
            self.worst.as_secs_f64() * 1000.0,
        );
        *self = Self::default();
        Some(line)
    }
}

/// Everything the loop decides, apart from SDL and GL, so it can be tested.
struct Shell<L: Launcher> {
    home: Home,
    input: Input,
    launcher: L,
    /// False while the window is hidden or the app is in the background:
    /// nothing is drawn then, not even an animation in flight.
    visible: bool,
    quit: bool,
}

impl<L: Launcher> Shell<L> {
    fn new(mut launcher: L, now: Duration) -> Self {
        let (apps, failed) = match launcher.list() {
            Ok(apps) => (apps, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        info!("{} apps", apps.len());
        let mut home = Home::new(apps, BackAtTop::Exit);
        if let Some(e) = failed {
            info!("listing apps failed: {e}");
            home.show_status(format!("Could not list apps: {e}"), now);
        }
        Self {
            home,
            input: Input::new(),
            launcher,
            visible: true,
            quit: false,
        }
    }

    /// Fires the key repeats due by `now`. Runs before each batch of events.
    fn tick(&mut self, now: Duration) {
        for press in self.input.poll(now) {
            self.press(press, now);
        }
    }

    fn press(&mut self, press: Press, now: Duration) {
        match self.home.handle(press, now) {
            Some(Effect::Launch(id)) => {
                let result = self.launcher.launch(&id);
                match &result {
                    Ok(()) => info!("launched {id}"),
                    Err(e) => info!("launching {id} failed: {e}"),
                }
                self.home.launched(&id, &result, now);
            }
            Some(Effect::Exit) => {
                info!("back on the top row: leaving");
                self.quit = true;
            }
            None => {}
        }
    }

    fn event(&mut self, event: Event, now: Duration) {
        match event {
            Event::Quit => self.quit = true,
            Event::KeyDown {
                scancode,
                keycode,
                repeat,
            } => {
                if !repeat {
                    let action = action_for(scancode);
                    info!("key down scancode {scancode} keycode {keycode:#x} ({action:?})");
                }
                // Keys only reach the app in front, so a real press means the
                // screen is showing (pointer noise and SDL repeats do not count).
                if !repeat && action_for(scancode).is_some() {
                    self.show();
                }
                if let Some(press) = self.input.key_down(scancode, repeat, now) {
                    self.press(press, now);
                }
            }
            Event::KeyUp { scancode } => self.input.key_up(scancode),
            Event::Window(id) => self.window(id),
            Event::Background => self.hide("the app went to the background"),
            Event::Foreground => self.show(),
            Event::Other(_) => {}
        }
    }

    fn window(&mut self, id: u8) {
        use sdl::window::*;
        match id {
            HIDDEN | MINIMIZED => self.hide("the window was hidden"),
            SHOWN | RESTORED => self.show(),
            // Exposed means visible again (a hidden shell must come back), and
            // its last frame may be gone either way.
            EXPOSED => {
                self.show();
                self.home.mark_dirty();
            }
            RESIZED | SIZE_CHANGED => self.home.mark_dirty(),
            // On the TV, losing focus means another app is in front. On a PC
            // an unfocused window is still on screen, so it keeps drawing.
            FOCUS_LOST if on_tv() => self.hide("the window lost focus"),
            FOCUS_LOST => self.input.release_all(),
            FOCUS_GAINED if on_tv() => self.show(),
            _ => {}
        }
    }

    fn hide(&mut self, why: &str) {
        if self.visible {
            info!("{why}: drawing stops");
        }
        self.visible = false;
        self.input.release_all();
        self.home.settle_now();
    }

    fn show(&mut self) {
        if !self.visible {
            info!("in front again: drawing resumes");
            self.visible = true;
            self.home.mark_dirty();
        }
    }

    /// Whether the next pass of the loop draws a frame.
    fn wants_frame(&self) -> bool {
        self.visible && !self.home.is_settled()
    }
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

    let renderer = Renderer::new(gl)?;
    let mut batch = Batch::default();
    let mut stats = FrameStats::new(Duration::from_secs(1));
    let mut burst = Burst::default();
    let mut clock = Clock::new(opts.fixed_dt);
    let mut screenshot = opts.screenshot.clone();
    let mut settled_shot = opts.screenshot_when_settled.clone();
    let mut script = opts.keys.clone();
    if let Some(step) = opts.fixed_dt {
        info!("fixed clock: {:.3} ms a frame", step.as_secs_f64() * 1000.0);
    }
    let mut shell = Shell::new(MockLauncher::default(), clock.now());

    loop {
        // A settled screenshot that is ready must not wait for an event; one
        // that cannot be drawn yet (window hidden) must not spin either.
        let shot_ready =
            settled_shot.is_some() && shell.visible && keys_done(script.as_ref(), &shell.input);
        let first = if shell.wants_frame() || shot_ready {
            sdl.poll_event()
        } else {
            if let Some(line) = burst.finish(batch.quad_count()) {
                info!("{line}");
            }
            // Deadlines on the loop's clock; --exit-after is in real time.
            let next = [
                shell.input.next_deadline(),
                shell.home.next_deadline(),
                script.as_ref().and_then(Script::next_deadline),
                // Only while it can be drawn: a hidden shell must not spin.
                screenshot
                    .as_ref()
                    .filter(|_| shell.visible)
                    .map(|_| SCREENSHOT_AFTER),
            ]
            .into_iter()
            .flatten()
            .min();
            let until_exit = opts.exit_after.map(|e| e.saturating_sub(clock.real()));
            match next {
                Some(t) if clock.is_fixed() => {
                    clock.jump_to(t);
                    sdl.poll_event()
                }
                Some(t) => {
                    let wait = t.saturating_sub(clock.now());
                    sdl.wait_event(Some(until_exit.map_or(wait, |e| e.min(wait))))
                }
                None => sdl.wait_event(until_exit),
            }
        };
        let now = clock.now();
        shell.tick(now);
        if let Some(script) = script.as_mut() {
            for key in script.due(now) {
                let event = if key.down {
                    Event::KeyDown {
                        scancode: key.scancode,
                        keycode: 0,
                        repeat: false,
                    }
                } else {
                    Event::KeyUp {
                        scancode: key.scancode,
                    }
                };
                shell.event(event, now);
            }
        }
        if let Some(event) = first {
            shell.event(event, now);
        }
        while let Some(event) = sdl.poll_event() {
            shell.event(event, now);
        }
        if shell.quit {
            break;
        }
        if opts.exit_after.is_some_and(|limit| clock.real() >= limit) {
            info!("exit-after reached");
            break;
        }
        if screenshot.is_some() && now >= SCREENSHOT_AFTER {
            shell.home.mark_dirty();
        }
        shell.home.advance_to(now);
        // The settled screenshot: the keys are all played and released and
        // nothing moves, so draw the settled frame once more and keep it.
        let take_settled = settled_shot.is_some()
            && keys_done(script.as_ref(), &shell.input)
            && !shell.wants_frame();
        if take_settled {
            shell.home.mark_dirty();
        }
        if !shell.wants_frame() {
            continue;
        }

        // SAFETY: the window is live; see above.
        drawable = unsafe { sdl.drawable_size(surface.window) };
        draw::home(&mut batch, &shell.home.view());
        renderer.draw(&batch, drawable);
        if now >= SCREENSHOT_AFTER
            && let Some(path) = screenshot.take()
        {
            save_screenshot(&renderer, drawable, &path);
        }
        if take_settled && let Some(path) = settled_shot.take() {
            save_screenshot(&renderer, drawable, &path);
            info!("settled at {:.3} s on the loop's clock", now.as_secs_f64());
            shell.quit = true;
        }
        // SAFETY: the window is live and its context is current.
        unsafe { sdl.gl_swap_window(surface.window) };
        shell.home.frame_drawn();
        clock.frame_drawn();
        if shell.quit {
            break;
        }

        if let Some(took) = burst.frame(Instant::now())
            && let Some(report) = stats.record(took)
        {
            info!(
                "fps {:.1} avg {:.2} ms max {:.2} ms ({} frames, {} quads)",
                report.fps,
                report.avg_ms,
                report.max_ms,
                report.frames,
                batch.quad_count()
            );
            let err = renderer.gl().error();
            if err != gl::NO_ERROR {
                info!("GL error {err:#x}");
            }
        }
    }
    if let Some(line) = burst.finish(batch.quad_count()) {
        info!("{line}");
    }
    drop(surface);
    Ok(())
}

/// Every scripted key has been pressed and released.
fn keys_done(script: Option<&Script>, input: &Input) -> bool {
    script.is_none_or(Script::is_done) && !input.is_holding()
}

/// Reads the back buffer before the swap, cropped to the 16:9 picture.
fn save_screenshot(renderer: &Renderer, drawable: (i32, i32), path: &std::path::Path) {
    let (x, y, w, h) = fit_viewport(drawable.0, drawable.1);
    let pixels = renderer.gl().read_rgba(x, y, w, h);
    match capture::save_gl_frame(path, w as u32, h as u32, &pixels) {
        Ok(()) => info!("screenshot {w}x{h} saved to {}", path.display()),
        Err(e) => info!("screenshot to {} failed: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::apps::{App, AppId, LaunchError};
    use lumen_core::input::scancode;

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

    #[test]
    fn a_fixed_clock_moves_one_step_per_frame_and_jumps_forward_only() {
        let mut clock = Clock::new(Some(ms(20)));
        assert_eq!(clock.now(), Duration::ZERO);
        clock.frame_drawn();
        clock.frame_drawn();
        assert_eq!(clock.now(), ms(40));
        clock.jump_to(ms(500));
        assert_eq!(clock.now(), ms(500));
        clock.jump_to(ms(100));
        assert_eq!(clock.now(), ms(500), "never backwards");
    }

    #[test]
    fn a_real_clock_ignores_frames_and_jumps() {
        let mut clock = Clock::new(None);
        assert!(!clock.is_fixed());
        clock.frame_drawn();
        clock.jump_to(Duration::from_secs(60));
        assert!(clock.now() < Duration::from_secs(60));
    }

    #[test]
    fn keys_are_done_once_played_and_released() {
        let mut script = Script::parse("R").unwrap();
        let mut input = Input::new();
        assert!(keys_done(None, &input));
        assert!(!keys_done(Some(&script), &input));
        assert_eq!(script.due(Duration::from_secs(5)).len(), 2);
        assert!(keys_done(Some(&script), &input));
        let _ = input.key_down(scancode::LEFT, false, ms(0));
        assert!(!keys_done(Some(&script), &input), "a key is still held");
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn shell() -> Shell<MockLauncher> {
        let mut shell = Shell::new(MockLauncher::default(), Duration::ZERO);
        shell.home.frame_drawn();
        shell
    }

    fn down(code: i32) -> Event {
        Event::KeyDown {
            scancode: code,
            keycode: 0,
            repeat: false,
        }
    }

    fn key(shell: &mut Shell<MockLauncher>, code: i32, at: Duration) {
        shell.event(down(code), at);
        shell.event(Event::KeyUp { scancode: code }, at);
    }

    #[test]
    fn keys_move_focus_and_ok_launches_through_the_launcher() {
        let mut shell = shell();
        key(&mut shell, scancode::RIGHT, ms(0));
        assert_eq!(shell.home.focused(), Some(1));
        key(&mut shell, scancode::RETURN, ms(100));
        assert_eq!(shell.launcher.launched(), &[AppId::new("amazon")]);
        assert_eq!(shell.home.view().status, Some("Opening Prime Video"));
        assert!(!shell.quit);
    }

    #[test]
    fn back_on_the_top_row_quits() {
        let mut shell = shell();
        key(&mut shell, scancode::WEBOS_BACK, ms(0));
        assert!(shell.quit);
    }

    #[test]
    fn a_failed_launch_says_so() {
        let mut shell = Shell::new(MockLauncher::default().failing_on("netflix"), ms(0));
        key(&mut shell, scancode::RETURN, ms(0));
        assert_eq!(shell.home.view().status, Some("Could not open Netflix"));
    }

    struct Broken;

    impl Launcher for Broken {
        fn list(&mut self) -> Result<Vec<App>, LaunchError> {
            Err(LaunchError("no luna".to_owned()))
        }
        fn launch(&mut self, _: &AppId) -> Result<(), LaunchError> {
            unreachable!("nothing to launch")
        }
    }

    #[test]
    fn a_failed_listing_shows_the_empty_state_and_the_error() {
        let shell = Shell::new(Broken, ms(0));
        let view = shell.home.view();
        assert!(view.empty);
        assert_eq!(view.status, Some("Could not list apps: no luna"));
    }

    #[test]
    fn going_to_the_background_stops_drawing_and_settles() {
        let mut shell = shell();
        key(&mut shell, scancode::DOWN, ms(0));
        assert!(shell.wants_frame());
        shell.event(Event::Background, ms(10));
        assert!(!shell.visible && !shell.wants_frame());
        assert!(!shell.home.is_animating(), "springs jumped to rest");
        shell.event(Event::Foreground, ms(20));
        assert!(shell.wants_frame(), "one fresh frame on return");
    }

    #[test]
    fn a_hidden_window_stops_drawing_and_drops_held_keys() {
        let mut shell = shell();
        shell.event(down(scancode::RIGHT), ms(0));
        assert!(shell.input.is_holding());
        shell.event(Event::Window(sdl::window::MINIMIZED), ms(10));
        assert!(!shell.visible && !shell.input.is_holding());
        shell.event(Event::Window(sdl::window::RESTORED), ms(20));
        assert!(shell.visible && shell.wants_frame());
    }

    #[test]
    fn a_key_while_hidden_means_the_shell_is_in_front_again() {
        let mut shell = shell();
        shell.event(Event::Window(sdl::window::HIDDEN), ms(0));
        key(&mut shell, scancode::RIGHT, ms(10));
        assert!(shell.visible);
        assert_eq!(shell.home.focused(), Some(1));
    }

    #[test]
    fn losing_focus_drops_keys_and_stops_drawing_only_on_the_tv() {
        let mut shell = shell();
        shell.event(down(scancode::LEFT), ms(0));
        shell.event(Event::Window(sdl::window::FOCUS_LOST), ms(10));
        assert!(!shell.input.is_holding());
        assert_eq!(shell.visible, !on_tv());
    }

    #[test]
    fn held_arrows_repeat_through_tick() {
        let mut shell = shell();
        shell.event(down(scancode::RIGHT), ms(0));
        shell.tick(ms(400));
        shell.tick(ms(520));
        assert_eq!(shell.home.focused(), Some(3));
    }

    #[test]
    fn a_burst_reports_frames_and_times_then_resets() {
        let mut burst = Burst::default();
        assert_eq!(burst.finish(0), None);
        let t0 = Instant::now();
        assert_eq!(burst.frame(t0), None, "the first frame has no duration");
        assert_eq!(burst.frame(t0 + ms(16)), Some(ms(16)));
        assert_eq!(burst.frame(t0 + ms(40)), Some(ms(24)));
        let line = burst.finish(1234).expect("frames were drawn");
        assert_eq!(
            line,
            "drew 3 frames in 40 ms (avg 20.00 ms, worst 24.00 ms, 1234 quads); idle"
        );
        assert_eq!(burst.finish(0), None);
    }
}
