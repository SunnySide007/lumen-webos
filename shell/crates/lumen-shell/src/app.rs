//! The home screen's main loop. It reads the remote, lets
//! `lumen_core::home::Home` decide what happens, draws only while something
//! changes or moves, and waits in `SDL_WaitEventTimeout` once the screen has
//! settled (docs/specs/shell-mvp.md, "Repainting"). Every number it used to
//! show as hello-gles (GL renderer, frame times, key codes) goes to the log.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::{AppId, Launcher, MockLauncher};
use lumen_core::home::{BackAtTop, Effect, Home, LIFT};
use lumen_core::input::{Action, Input, Press, action_for, scancode};
use lumen_core::pace::Pacer;
use lumen_core::script::Script;

use crate::capture;
use crate::draw;
use crate::faults;
use crate::frame_stats::FrameStats;
use crate::gl::{self, Gl};
use crate::icons::Icons;
use crate::info;
use crate::luna::{self, Ls2, LunaLauncher};
use crate::render::{Batch, Renderer, SCREEN_H, SCREEN_W, fit_viewport};
use crate::sdl::{self, Event, GlContext, Sdl, Window};
use crate::timing::{
    self, Foreground, FrameSplit, Idle, KeyTimes, Phase, Pointer, SLOW_FRAME, SinceKey, StartTimes,
};
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

/// When `--screenshot` takes its frame.
const SCREENSHOT_AFTER: Duration = Duration::from_millis(1500);
/// How often a settled screen looks for icons while the loader has some.
const ICON_POLL: Duration = Duration::from_millis(16);
/// How long the screen stays settled before glyphs are made ahead: three
/// refreshes at 60 Hz, so the burst's last frames, at most two still queued
/// on the B9, have gone out and the GPU is done with the atlas.
const WARM_AFTER: Duration = Duration::from_millis(50);
/// How long after Select its launch waits: the press pulse's dip is at its
/// deepest 1/ω of the lift spring after the kick, 33 ms.
const LAUNCH_AFTER: Duration = Duration::from_nanos((1e9 / LIFT.omega as f64) as u64);

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

/// The loop's time. Real by default, paced by `lumen_core::pace` while frames
/// are drawn with vsync: each frame of a burst shows one period of motion
/// more than the one before, and a frame shows one period after its pass.
/// With `--fixed-dt` a virtual clock that advances exactly one step per frame
/// drawn and, while the screen is settled, jumps straight to the next
/// deadline instead of waiting for it, so the same keys always give the same
/// frames; it is never paced.
#[derive(Debug)]
struct Clock {
    start: Instant,
    step: Option<Duration>,
    virtual_now: Duration,
    /// With vsync and the real clock.
    pacer: Option<Pacer>,
}

/// One pass of the loop: its time on the loop's clock, and the real time it
/// was read at (the same unless paced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pass {
    now: Duration,
    real: Duration,
}

impl Clock {
    /// `period` paces a real clock (vsync on); a fixed step is never paced.
    fn new(step: Option<Duration>, period: Option<Duration>) -> Self {
        Self {
            start: Instant::now(),
            step,
            virtual_now: Duration::ZERO,
            pacer: period.filter(|_| step.is_none()).map(Pacer::new),
        }
    }

    /// Paces a real clock by `period` from the next burst on; a fixed step
    /// is never paced.
    fn pace(&mut self, period: Duration) {
        if self.step.is_none() {
            self.pacer = Some(Pacer::new(period));
        }
    }

    /// A pass read now. Its `now` is what presses, repeats and deadlines go
    /// by; its `real` is what a paced frame is drawn for.
    fn pass(&self) -> Pass {
        if self.step.is_some() {
            return Pass {
                now: self.virtual_now,
                real: self.virtual_now,
            };
        }
        let real = self.start.elapsed();
        Pass {
            now: self.pacer.map_or(real, |p| p.time(real)),
            real,
        }
    }

    /// The time a pass read now would have.
    fn now(&self) -> Duration {
        self.pass().now
    }

    /// The motion time the frame drawn in the pass at `now` shows: paced, one
    /// period on, the earliest vsync it can reach, so a press made in the
    /// pass already moves something in that frame; otherwise `now`.
    fn shows(&self, now: Duration) -> Duration {
        self.pacer.map_or(now, |p| p.shows(now))
    }

    /// When key repeats fall due in `pass`: its real time when paced, never
    /// after its `now`, so motion running ahead of real time fires no repeat
    /// before the key-up that would stop it.
    fn input_time(&self, pass: Pass) -> Duration {
        match self.pacer {
            Some(_) => pass.real.min(pass.now),
            None => pass.now,
        }
    }

    /// Real time since the start, whatever the clock: `--exit-after` and the
    /// idle waits use it.
    fn real(&self) -> Duration {
        self.start.elapsed()
    }

    fn is_fixed(&self) -> bool {
        self.step.is_some()
    }

    /// A frame was drawn in `pass`.
    fn frame_drawn(&mut self, pass: Pass) {
        if let Some(step) = self.step {
            self.virtual_now += step;
        }
        if let Some(pacer) = &mut self.pacer {
            pacer.drawn(pass.now, pass.real);
        }
    }

    /// The screen settled, or a launch cut the burst off: the next frame
    /// starts a burst.
    fn idle(&mut self) {
        if let Some(pacer) = &mut self.pacer {
            pacer.idle();
        }
    }

    /// The vsyncs skipped since the last call, for the burst line; `None`
    /// when not paced.
    fn take_skipped(&mut self) -> Option<u32> {
        self.pacer.as_mut().map(Pacer::take_skipped)
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

/// How many of a burst's first timed frames its line names.
const FIRST_TIMED: usize = 3;
/// How many slow frames a burst logs a line for; the rest are counted.
const SLOW_LOGGED: u32 = 4;

/// The frames drawn since the screen last settled, for its `drew` line and its
/// `slow frame` lines.
#[derive(Debug, Default)]
struct Burst {
    frames: u32,
    first: Option<Instant>,
    last: Option<Instant>,
    worst: Duration,
    /// The first timed frames: frames 2, 3 and 4, since frame 1 has nothing
    /// before it to be timed from.
    first_timed: [Option<Duration>; FIRST_TIMED],
    /// Slow frames given a line of their own so far.
    slow_logged: u32,
    /// Slow frames past `SLOW_LOGGED`, only counted.
    slow_more: u32,
}

impl Burst {
    /// Counts a frame finished at `at`; returns how long it took, except for
    /// the first frame after an idle wait, which has nothing to measure from.
    fn frame(&mut self, at: Instant) -> Option<Duration> {
        let took = self.last.map(|last| at.saturating_duration_since(last));
        match took {
            Some(t) => {
                self.worst = self.worst.max(t);
                if let Some(slot) = self.first_timed.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(t);
                }
            }
            None => self.first = Some(at),
        }
        self.last = Some(at);
        self.frames += 1;
        took
    }

    /// The `slow frame` line for the frame just counted, which took `took`,
    /// if it took over `SLOW_FRAME` and the burst has logged fewer than
    /// `SLOW_LOGGED`; past that it is only counted.
    fn slow_line(&mut self, took: Duration, split: &FrameSplit) -> Option<String> {
        if took <= SLOW_FRAME {
            return None;
        }
        if self.slow_logged >= SLOW_LOGGED {
            self.slow_more = self.slow_more.saturating_add(1);
            return None;
        }
        self.slow_logged += 1;
        Some(timing::slow_frame_line(self.frames, took, split))
    }

    /// The log lines for a finished burst (slow frames not logged, then the
    /// burst itself), and a fresh start; none if no frame was drawn.
    /// `skipped` is the pacer's count of vsyncs skipped in the burst, `None`
    /// without a pacer, which leaves it out of the line.
    fn finish(&mut self, quads: usize, skipped: Option<u32>) -> Vec<String> {
        let (Some(first), Some(last)) = (self.first, self.last) else {
            return Vec::new();
        };
        let span = last.saturating_duration_since(first);
        let avg = match self.frames {
            0 | 1 => Duration::ZERO,
            n => span / (n - 1),
        };
        let mut lines = Vec::new();
        if self.slow_more > 0 {
            lines.push(timing::slow_more_line(self.slow_more));
        }
        let times: Vec<String> = self
            .first_timed
            .iter()
            .flatten()
            .map(|&t| timing::fine_ms(t))
            .collect();
        let firsts = if times.is_empty() {
            String::new()
        } else {
            format!("; first {} ms", times.join(", "))
        };
        let skips = skipped.map_or(String::new(), |k| format!("; {k} vsyncs skipped"));
        lines.push(format!(
            "drew {} frames in {:.0} ms (avg {:.2} ms, worst {:.2} ms, {quads} quads){firsts}{skips}; idle",
            self.frames,
            span.as_secs_f64() * 1000.0,
            avg.as_secs_f64() * 1000.0,
            self.worst.as_secs_f64() * 1000.0,
        ));
        *self = Self::default();
        lines
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
    /// Why the shell was last hidden, for lines about what it never showed.
    hidden_why: &'static str,
    quit: bool,
    /// An app to launch, and when on the loop's clock: once the press
    /// pulse's dip is at its deepest (`LAUNCH_AFTER`) and on screen.
    launch_due: Option<(AppId, Duration)>,
    /// The real clock's start, to put SDL's stamps on the loop's clock; none
    /// under a fixed clock, where an event counts at its pass's time.
    stamps_from: Option<Instant>,
    /// Presses on their way to the screen (`key …: shown after …` lines).
    keys: KeyTimes,
    /// Coming back to the front, on its way to a frame.
    foreground: Foreground,
    /// What arrived since the remote's last key, for focus and lifecycle lines.
    since_key: SinceKey,
    /// Whether the window has keyboard focus, from SDL's focus events, for
    /// the heartbeat line.
    focused: bool,
    /// The Magic Remote's pointer, from the 484 and 485 keys.
    pointer: Pointer,
    /// Whether this SDL can hide the pointer (`SDL_webOSCursorVisibility`).
    can_hide_pointer: bool,
    /// An arrow came while the pointer showed: the loop hides it.
    hide_pointer_due: bool,
    /// The last input and the next heartbeat (docs/specs/shell-mvp.md,
    /// "Input diagnostics").
    idle: Idle,
    /// What the luna watch thread reads and writes.
    shared: Arc<watch::Shared>,
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
        // The watch counts "last input" from the same moment the heartbeat
        // does: the start, as if input had just come.
        let shared = watch::Shared::new();
        shared.set_last_input(now);
        Self {
            home,
            input: Input::new(),
            launcher,
            visible: true,
            hidden_why: "the window was hidden",
            quit: false,
            launch_due: None,
            stamps_from: None,
            keys: KeyTimes::default(),
            foreground: Foreground::default(),
            since_key: SinceKey::default(),
            focused: false,
            pointer: Pointer::default(),
            can_hide_pointer: false,
            hide_pointer_due: false,
            idle: Idle::new(now),
            shared,
        }
    }

    /// Any input at `now`: a key (the pointer codes included) or a mouse
    /// event. The heartbeat and the watch's lines count from it.
    fn input(&mut self, now: Duration) {
        self.idle.input(now);
        self.shared.set_last_input(now);
    }

    /// The idle heartbeat line, if one is due at `now`. The power state is
    /// fetched (a lock and a copy) only when one is.
    fn heartbeat(&mut self, now: Duration) -> Option<String> {
        if now < self.idle.next_deadline() {
            return None;
        }
        let power = self.shared.power();
        self.idle
            .beat(now, self.focused, self.pointer, power.as_deref())
    }

    /// Whether the loop should hide the pointer now (an arrow came while it
    /// showed); asked once per arrow.
    fn take_pointer_hide(&mut self) -> bool {
        std::mem::take(&mut self.hide_pointer_due)
    }

    /// The loop hid the pointer. LG's 485, if it comes, says so again; if it
    /// never comes, the state is still right and no arrow asks again.
    fn pointer_hidden(&mut self) {
        self.pointer = Pointer::Hidden;
    }

    /// When SDL stamped an event, on the loop's clock: never after `now`, and
    /// `now` itself under a fixed clock.
    fn stamp(&self, at: Instant, now: Duration) -> Duration {
        self.stamps_from
            .map_or(now, |start| at.saturating_duration_since(start).min(now))
    }

    fn press(&mut self, press: Press, now: Duration) {
        match self.home.handle(press, now) {
            // The press pulse goes on screen first: the launch waits until
            // the dip is at its deepest and a frame shows it
            // (`launch_if_due`), since luna may take a third of a second to
            // answer and the loop cannot draw meanwhile. A second Select
            // before then launches nothing more.
            Some(Effect::Launch(id)) => {
                self.launch_due.get_or_insert((id, now + LAUNCH_AFTER));
            }
            Some(Effect::Exit) => {
                info!("back on the top row: leaving");
                self.quit = true;
            }
            None => {}
        }
    }

    /// When the launch a press asked for is due, on the loop's clock.
    fn launch_due_at(&self) -> Option<Duration> {
        self.launch_due.as_ref().map(|&(_, due)| due)
    }

    /// Launches the app a press asked for, if it is due by `now`, the time
    /// of the frame just shown (or of the pass, when nothing is drawn).
    /// `answered_at` reads the loop's clock once luna has answered, so the
    /// status line's time starts after a launch that took seconds. Returns
    /// whether it launched.
    fn launch_if_due(&mut self, now: Duration, answered_at: impl FnOnce() -> Duration) -> bool {
        let Some((id, _)) = self.launch_due.take_if(|(_, due)| *due <= now) else {
            return false;
        };
        let started = Instant::now();
        let result = self.launcher.launch(&id);
        let answered = Instant::now();
        info!(
            "{}",
            timing::launch_line(&id, answered.duration_since(started), &result)
        );
        self.keys.launched(started, answered);
        self.home.launched(&id, &result, answered_at());
        true
    }

    /// A launch asked for will not run: the shell was hidden, or is leaving.
    fn drop_launch(&mut self, why: &str) {
        if let Some((id, _)) = self.launch_due.take() {
            info!("{}", timing::launch_dropped_line(&id, why));
        }
    }

    /// `at` is when SDL stamped the event, for the timing lines.
    fn event(&mut self, event: Event, now: Duration, at: Instant) {
        match event {
            Event::Quit => self.quit = true,
            Event::KeyDown {
                scancode,
                keycode,
                repeat,
            } => {
                let action = action_for(scancode);
                if !repeat {
                    info!("key down scancode {scancode} keycode {keycode:#x} ({action:?})");
                }
                self.heard_key(scancode, at);
                self.input(now);
                // LG sends the pointer codes as key-downs with no key-up, so
                // SDL may mark a later one as a repeat: the state decides,
                // not the flag. A line goes out when the state changes and
                // on every fresh press, so LG's own resends still show.
                if let Some(state) = pointer_state(scancode) {
                    if self.pointer != state || !repeat {
                        self.pointer = state;
                        info!("pointer {}", state.name());
                    }
                    return;
                }
                // Keys only reach the app in front, so a real press means the
                // screen is showing (SDL repeats do not count).
                if !repeat && action.is_some() {
                    self.show("a key press", at);
                }
                // An OK pressed while the pointer shows arrives as a click
                // (B9, 2026-10-22), so an arrow puts the pointer away where
                // the SDL allows it, as moonlight-tv does.
                if !repeat
                    && matches!(action, Some(Action::Move(_)))
                    && self.pointer == Pointer::Shown
                    && self.can_hide_pointer
                {
                    self.hide_pointer_due = true;
                }
                // Held keys and their repeats go by SDL's stamp: a press
                // first fires the repeats due before it; an SDL repeat read
                // late, after a stall, still keeps its key alive. Every press
                // acts at the pass's time, `now`.
                for press in self.input.key_down(scancode, repeat, self.stamp(at, now)) {
                    // Only presses are timed, not SDL's repeats or the shell's.
                    if !press.repeat {
                        self.keys.pressed(scancode, at);
                    }
                    self.press(press, now);
                }
            }
            Event::KeyUp { scancode } => {
                // The key-up SDL makes for a pointer code when the window
                // loses focus is not input (LG sends none itself), but it
                // still counts as an event in the focus lines, as before.
                if pointer_state(scancode).is_some() {
                    self.since_key.other();
                    return;
                }
                self.input(now);
                self.heard_key(scancode, at);
                // Only the repeats due before the release fire.
                for press in self.input.key_up(scancode, self.stamp(at, now)) {
                    self.press(press, now);
                }
            }
            Event::MouseButton { button, down, x, y } => {
                self.mouse_button(button, down, x, y, now, at)
            }
            Event::MouseMotion | Event::MouseWheel => {
                self.input(now);
                self.since_key.other();
            }
            Event::Window(id) => self.window(id, at),
            Event::Background => {
                info!("{}", self.since_key.line("app background", at));
                self.since_key.other();
                self.hide("the app went to the background", at);
            }
            Event::Foreground => {
                info!("{}", self.since_key.line("app foreground", at));
                self.since_key.other();
                self.show("did enter foreground", at);
            }
            Event::Other(_) => self.since_key.other(),
        }
    }

    /// Counts a key event toward the focus lines: a remote key the shell acts
    /// on resets them, anything else (pointer codes) is one more event.
    fn heard_key(&mut self, scancode: i32, at: Instant) {
        if action_for(scancode).is_some() {
            self.since_key.key(at);
        } else {
            self.since_key.other();
        }
    }

    /// A pointer button. Button 1 down is OK, through OK's own path: it is
    /// timed as key 40, counts as a key for the focus lines, and launches
    /// once a frame shows the press pulse's dip. Other buttons are only
    /// logged.
    fn mouse_button(&mut self, button: u8, down: bool, x: i32, y: i32, now: Duration, at: Instant) {
        self.input(now);
        let stamp = self.stamp(at, now);
        if !down {
            info!("mouse button {button} up at {x},{y}");
            self.since_key.other();
            if button == 1 {
                for press in self.input.key_up(scancode::RETURN, stamp) {
                    self.press(press, now);
                }
            }
            return;
        }
        if button != 1 {
            info!("mouse button {button} down at {x},{y}: ignored");
            self.since_key.other();
            return;
        }
        info!("mouse button 1 down at {x},{y}: taken as OK");
        self.since_key.key(at);
        self.show("a click", at);
        for press in self.input.key_down(scancode::RETURN, false, stamp) {
            if !press.repeat {
                self.keys.pressed(scancode::RETURN, at);
            }
            self.press(press, now);
        }
    }

    fn window(&mut self, id: u8, at: Instant) {
        use sdl::window::*;
        if let Some(name) = window_name(id) {
            info!("{}", self.since_key.line(&format!("window {name}"), at));
        }
        self.since_key.other();
        match id {
            FOCUS_GAINED => self.focused = true,
            FOCUS_LOST => self.focused = false,
            _ => {}
        }
        match id {
            HIDDEN | MINIMIZED => self.hide("the window was hidden", at),
            SHOWN => self.show("shown", at),
            RESTORED => self.show("restored", at),
            // Exposed means visible again (a hidden shell must come back), and
            // its last frame may be gone either way.
            EXPOSED => {
                self.show("exposed", at);
                self.home.mark_dirty();
            }
            RESIZED | SIZE_CHANGED => self.home.mark_dirty(),
            // On the TV, losing focus means another app is in front. On a PC
            // an unfocused window is still on screen, so it keeps drawing.
            FOCUS_LOST if on_tv() => self.hide("the window lost focus", at),
            FOCUS_LOST => self.input.release_all(),
            FOCUS_GAINED if on_tv() => self.show("focus gained", at),
            _ => {}
        }
    }

    fn hide(&mut self, why: &'static str, at: Instant) {
        if self.visible {
            info!("{why}: drawing stops");
            // The reason that hid it, not a later one while it stays hidden.
            self.hidden_why = why;
        }
        self.visible = false;
        self.input.release_all();
        self.home.settle_now();
        for line in self.keys.not_shown(at, why) {
            info!("{line}");
        }
        if let Some(line) = self.foreground.not_shown(at, why) {
            info!("{line}");
        }
        // Launched now, the app would open over whatever hid the shell.
        self.drop_launch(why);
    }

    fn show(&mut self, cause: &'static str, at: Instant) {
        if !self.visible {
            info!("in front again ({cause}): drawing resumes");
            self.visible = true;
            self.home.mark_dirty();
            self.foreground.came_back(at);
        }
    }

    /// A frame finished swapping at `at`: the presses and the return to the
    /// front it showed.
    fn frame_shown(&mut self, at: Instant) {
        for line in self.keys.shown(at) {
            info!("{line}");
        }
        if let Some(line) = self.foreground.shown(at) {
            info!("{line}");
        }
    }

    /// The screen settled with presses that changed nothing on it.
    fn nothing_to_draw(&mut self, at: Instant) {
        for line in self.keys.not_shown(at, "nothing to draw") {
            info!("{line}");
        }
    }

    /// The loop is ending; no frame will show what still waits, and no
    /// launch still waiting runs.
    fn leaving(&mut self, at: Instant) {
        for line in self.keys.not_shown(at, "the shell is leaving") {
            info!("{line}");
        }
        self.drop_launch("the shell is leaving");
    }

    /// Whether the next pass of the loop draws a frame.
    fn wants_frame(&self) -> bool {
        self.visible && !self.home.is_settled()
    }
}

/// The pointer state a scancode announces: 484 shown, 485 hidden.
fn pointer_state(scancode: i32) -> Option<Pointer> {
    match scancode {
        scancode::WEBOS_CURSOR_SHOW => Some(Pointer::Shown),
        scancode::WEBOS_CURSOR_HIDE => Some(Pointer::Hidden),
        _ => None,
    }
}

/// The window events that get a focus line. Exposed and resized are left out:
/// a PC sends them while a window is dragged.
fn window_name(id: u8) -> Option<&'static str> {
    use sdl::window::*;
    match id {
        FOCUS_LOST => Some("focus lost"),
        FOCUS_GAINED => Some("focus gained"),
        HIDDEN => Some("hidden"),
        SHOWN => Some("shown"),
        MINIMIZED => Some("minimized"),
        RESTORED => Some("restored"),
        _ => None,
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

    let result = run_surface(&sdl, &opts, start);
    sdl.quit();
    result
}

fn run_surface(sdl: &Sdl, opts: &Options, mut start: StartTimes) -> Result<(), String> {
    let surface = open_surface(sdl, opts)?;
    let gl = Gl::load(|name| sdl.gl_proc_address(name))?;
    let vsync = sdl.gl_set_swap_interval(1);
    // SAFETY: the surface's window is live for this whole function.
    let mut drawable = unsafe { sdl.drawable_size(surface.window) };
    // Motion is paced only with vsync and the real clock, from the first idle
    // on: the rate is asked for there, not before the first frame, since
    // LG's call may ask luna. A fixed clock asks SDL nothing, so its log
    // stays as it was.
    let mut refresh_due = vsync && opts.fixed_dt.is_none();
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
    let mut assets = if opts.fixed_dt.is_some() {
        draw::Assets::new()?
    } else {
        draw::Assets::threaded()?
    };
    start.mark(Phase::Assets, Instant::now());
    let mut batch = Batch::default();
    let mut stats = FrameStats::new(Duration::from_secs(1));
    let mut burst = Burst::default();
    // The main thread's page faults at the last swap, for the next frame's.
    let mut faults_seen = faults::thread_faults();
    let mut clock = Clock::new(opts.fixed_dt, None);
    let mut screenshot = opts.screenshot.clone();
    let mut settled_shot = opts.screenshot_when_settled.clone();
    let mut script = opts.keys.clone();
    if let Some(step) = opts.fixed_dt {
        info!("fixed clock: {:.3} ms a frame", step.as_secs_f64() * 1000.0);
    }
    // Every title's glyphs are made once (`warm`), while visible, when the
    // screen has stayed settled `WARM_AFTER` past a burst; a press before
    // then puts it off to the next settle. Not under a fixed clock: warming
    // changes where glyphs sit in the atlas, and so the frames.
    let mut warmed = clock.is_fixed();
    // When, in real time, it is due.
    let mut warm_at: Option<Duration> = None;
    let mut shell = Shell::new(launcher(), clock.now());
    start.mark(Phase::Apps, Instant::now());
    shell.can_hide_pointer = sdl.can_hide_pointer();
    shell.stamps_from = (!clock.is_fixed()).then_some(clock.start);
    // The luna watch (power state, the app in front, screensaver requests)
    // runs on its own thread for the life of the loop and stops when this
    // drops, after the loop. Nothing to watch off the TV, and nothing under
    // a fixed clock, whose "last input" times are not real time.
    let _watch = (on_tv() && !clock.is_fixed())
        .then(|| watch::start(Arc::clone(&shell.shared), luna::SELF_ID, clock.start));
    // Logged once, with the first frame or with why there was none.
    let mut start_line = Some(start);

    loop {
        // A settled screenshot that is ready must not wait for an event; one
        // that cannot be drawn yet (window hidden, icons on their way) must
        // not spin either.
        let shot_ready = settled_shot.is_some()
            && shell.visible
            && keys_done(script.as_ref(), &shell.input)
            && !assets.icons.any_loading();
        // A pass that starts with an idle wait: the GPU has most likely
        // finished with the atlas, so icons may be placed in it.
        let waited = !(shell.wants_frame() || shot_ready);
        let first = if !waited {
            sdl.poll_event()
        } else {
            shell.nothing_to_draw(Instant::now());
            let burst_lines = burst.finish(batch.quad_count(), clock.take_skipped());
            for line in &burst_lines {
                info!("{line}");
            }
            clock.idle();
            if std::mem::take(&mut refresh_due) {
                // SAFETY: the surface's window is live for this whole
                // function.
                let refresh = unsafe { sdl.refresh(surface.window) };
                info!("{}", refresh.line());
                clock.pace(refresh.period().0);
            }
            if !warmed && !burst_lines.is_empty() {
                warm_at = Some(clock.real() + WARM_AFTER);
            }
            if shell.visible && warm_at.is_some_and(|at| clock.real() >= at) {
                warm(&shell.home, &mut assets, &renderer);
                warmed = true;
                warm_at = None;
            }
            // GL's error flag holds until read. Each `fps` line reads it too,
            // but a burst shorter than a second gives none, so read it here.
            if !burst_lines.is_empty() {
                let err = renderer.gl().error();
                if err != gl::NO_ERROR {
                    info!("GL error {err:#x}");
                }
            }
            // Drawing stopped: the next `fps` window starts with the next
            // burst, so it never spans an idle wait or a hide.
            stats.reset();
            // Deadlines on the loop's clock; --exit-after is in real time.
            let next = [
                shell.input.next_deadline(),
                shell.home.next_deadline(),
                // A launch waiting for its press's dip, should the screen
                // settle first.
                shell.launch_due_at(),
                script.as_ref().and_then(Script::next_deadline),
                // Only while it can be drawn: a hidden shell must not spin.
                screenshot
                    .as_ref()
                    .filter(|_| shell.visible)
                    .map(|_| SCREENSHOT_AFTER),
                // The same for icons; showing the shell again wakes the loop.
                (shell.visible && assets.icons.any_loading()).then(|| clock.now() + ICON_POLL),
                // The idle heartbeat. Not under a fixed clock, which would
                // jump 30 s at a time through it: there it only comes if the
                // keys or frames take the clock past it.
                (!clock.is_fixed()).then(|| shell.idle.next_deadline()),
                // Glyphs made ahead, once the burst's frames have gone out.
                warm_at.filter(|_| shell.visible),
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
                    // By real time: a paced clock may run ahead of it, and
                    // waiting by that would wake early and spin.
                    let wait = t.saturating_sub(clock.real());
                    sdl.wait_event(Some(until_exit.map_or(wait, |e| e.min(wait))))
                }
                None => sdl.wait_event(until_exit),
            }
        };
        let pass = clock.pass();
        let now = pass.now;
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
                shell.event(event, now, Instant::now());
            }
        }
        if let Some((event, at)) = first {
            shell.event(event, now, at);
        }
        while let Some((event, at)) = sdl.poll_event() {
            shell.event(event, now, at);
        }
        // After the events, which fired the repeats due by their own stamps
        // (so a key let go just now fires none after), the repeats due by the
        // pass's input time. They act at `now`, as presses do.
        for press in shell.input.poll(clock.input_time(pass)) {
            shell.press(press, now);
        }
        if shell.take_pointer_hide() && sdl.set_pointer_visible(false) {
            shell.pointer_hidden();
        }
        if let Some(line) = shell.heartbeat(now) {
            info!("{line}");
        }
        if !shell.visible
            && let Some(start) = start_line.take()
        {
            info!("{}", start.line_without_frame(shell.hidden_why));
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
        if icons_arrived(&mut assets.icons, waited) {
            shell.home.mark_dirty();
        }
        shell.home.advance_to(clock.shows(now));
        // The settled screenshot: the keys are all played and released,
        // nothing moves and every icon is in, so draw the settled frame once
        // more and keep it.
        let take_settled = settled_shot.is_some()
            && keys_done(script.as_ref(), &shell.input)
            && !assets.icons.any_loading()
            && !shell.wants_frame();
        if take_settled {
            shell.home.mark_dirty();
        }
        if !shell.wants_frame() {
            // No frame to draw: a launch goes once its time has come, and
            // until then it is one of the idle deadlines.
            launch_pending(
                &mut shell,
                &mut burst,
                &mut stats,
                batch.quad_count(),
                now,
                &mut clock,
            );
            continue;
        }

        // SAFETY: the window is live; see above.
        drawable = unsafe { sdl.drawable_size(surface.window) };
        let batch_from = Instant::now();
        draw::home(&mut batch, &shell.home.view(), &mut assets);
        let upload_from = Instant::now();
        let uploads = assets.atlas.take_uploads();
        renderer.upload(&uploads);
        let upload_count = uploads.len();
        let upload_bytes = uploads.iter().map(|u| u.rgba.len()).sum();
        // Freed here, inside the upload's time, not in the next frame's.
        drop(uploads);
        let submit_from = Instant::now();
        renderer.draw(&batch, drawable);
        let submitted = Instant::now();
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
        let swap_from = Instant::now();
        // SAFETY: the window is live and its context is current.
        unsafe { sdl.gl_swap_window(surface.window) };
        let shown = Instant::now();
        let faults_now = faults::thread_faults();
        let split = FrameSplit {
            batch: upload_from.saturating_duration_since(batch_from),
            upload: submit_from.saturating_duration_since(upload_from),
            submit: submitted.saturating_duration_since(submit_from),
            swap: shown.saturating_duration_since(swap_from),
            uploads: upload_count,
            upload_bytes,
            glyphs: assets.text.take_rasterised(),
            faults: faults_now
                .zip(faults_seen)
                .map(|(later, earlier)| later.since(earlier)),
        };
        faults_seen = faults_now;
        if let Some(start) = start_line.take() {
            info!("{}", start.line(shown));
        }
        shell.frame_shown(shown);
        shell.home.frame_drawn();
        clock.frame_drawn(pass);
        if shell.quit {
            break;
        }

        let took = burst.frame(shown);
        if let Some(line) = took.and_then(|t| burst.slow_line(t, &split)) {
            info!("{line}");
        }
        if let Some(took) = took
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
        // The frame just drawn is on screen, showing `clock.shows(now)`: once
        // that is past the dip's deepest point, the launch may block the loop.
        launch_pending(
            &mut shell,
            &mut burst,
            &mut stats,
            batch.quad_count(),
            clock.shows(now),
            &mut clock,
        );
    }
    shell.leaving(Instant::now());
    if let Some(start) = start_line.take() {
        info!("{}", start.line_without_frame("the shell is leaving"));
    }
    for line in burst.finish(batch.quad_count(), clock.take_skipped()) {
        info!("{line}");
    }
    drop(surface);
    Ok(())
}

/// Launches what a press asked for, if it is due by `now`. The burst, the
/// `fps` window and the pacer's burst end first, so the time luna takes to
/// answer is not counted as a frame, nor as vsyncs skipped. Returns whether
/// it launched.
fn launch_pending<L: Launcher>(
    shell: &mut Shell<L>,
    burst: &mut Burst,
    stats: &mut FrameStats,
    quads: usize,
    now: Duration,
    clock: &mut Clock,
) -> bool {
    if !shell.launch_due_at().is_some_and(|due| due <= now) {
        return false;
    }
    for line in burst.finish(quads, clock.take_skipped()) {
        info!("{line}");
    }
    stats.reset();
    clock.idle();
    shell.launch_if_due(now, || clock.now())
}

/// Makes every title's glyphs (`draw::warm`) and uploads them once the screen
/// has stayed settled for `WARM_AFTER`, so no burst waits on an upload and
/// the GPU is most likely done with the atlas. Any wait left lands in the
/// `warm` line's time, once, outside every burst.
fn warm(home: &Home, assets: &mut draw::Assets, renderer: &Renderer) {
    let from = Instant::now();
    draw::warm::warm(home.apps(), assets);
    let uploads = assets.atlas.take_uploads();
    renderer.upload(&uploads);
    let glyphs = assets.text.take_rasterised();
    info!("{}", draw::warm::line(glyphs, &uploads, from.elapsed()));
}

/// Takes in the icons the loader has finished, but only on a pass that began
/// with an idle wait: placed mid-burst, an icon would be uploaded into the
/// atlas that the frames in flight still read. (`receive` only keeps the
/// decoded image; it goes into the atlas when a frame draws it.) True if
/// any came.
fn icons_arrived(icons: &mut Icons, waited: bool) -> bool {
    waited && icons.receive()
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
    use std::path::Path;

    use lumen_core::apps::{App, LaunchError};

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
        let mut clock = Clock::new(Some(ms(20)), Some(ms(16)));
        assert!(clock.pacer.is_none(), "a fixed step is never paced");
        clock.pace(ms(16));
        assert!(clock.pacer.is_none(), "not later either");
        let pass = clock.pass();
        assert_eq!(pass.now, Duration::ZERO);
        assert_eq!(clock.shows(ms(0)), ms(0), "a frame shows its pass");
        assert_eq!(clock.input_time(pass), pass.now);
        clock.frame_drawn(pass);
        clock.frame_drawn(clock.pass());
        assert_eq!(clock.now(), ms(40));
        clock.jump_to(ms(500));
        assert_eq!(clock.now(), ms(500));
        clock.jump_to(ms(100));
        assert_eq!(clock.now(), ms(500), "never backwards");
        assert_eq!(clock.take_skipped(), None, "no skips in the burst line");
    }

    #[test]
    fn a_real_clock_ignores_frames_and_jumps() {
        let mut clock = Clock::new(None, None);
        assert!(!clock.is_fixed() && clock.pacer.is_none());
        let pass = clock.pass();
        assert_eq!(clock.input_time(pass), pass.now);
        clock.frame_drawn(pass);
        clock.jump_to(Duration::from_secs(60));
        assert!(clock.now() < Duration::from_secs(60));
        assert_eq!(clock.shows(ms(5)), ms(5));
        assert_eq!(clock.take_skipped(), None);
    }

    #[test]
    fn a_paced_clock_shows_each_frame_one_period_after_its_pass() {
        // Long enough that a slow test machine never misses a "vsync".
        let period = Duration::from_secs(1);
        let mut clock = Clock::new(None, None);
        clock.pace(period);
        let first = clock.pass();
        assert_eq!(first.now, first.real, "a burst starts from real time");
        assert_eq!(clock.shows(first.now), first.now + period);
        clock.frame_drawn(first);
        // The next pass is where the frame left the motion, however soon
        // the loop comes round; key repeats still go by real time.
        let next = clock.pass();
        assert_eq!(next.now, first.now + period);
        assert!(next.real < next.now);
        assert_eq!(clock.input_time(next), next.real);
        assert_eq!(clock.take_skipped(), Some(0));
        clock.idle();
        assert_eq!(clock.take_skipped(), Some(0));
    }

    #[test]
    fn a_paced_burst_line_counts_the_vsyncs_skipped() {
        let mut burst = Burst::default();
        let t0 = Instant::now();
        let _ = burst.frame(t0);
        let _ = burst.frame(t0 + ms(17));
        assert_eq!(
            burst.finish(10, Some(2)),
            [
                "drew 2 frames in 17 ms (avg 17.00 ms, worst 17.00 ms, 10 quads); first 17.00 ms; 2 vsyncs skipped; idle"
            ]
        );
        let _ = burst.frame(t0);
        assert_eq!(
            burst.finish(10, Some(0)),
            [
                "drew 1 frames in 0 ms (avg 0.00 ms, worst 0.00 ms, 10 quads); 0 vsyncs skipped; idle"
            ],
            "on every paced burst, 0 included"
        );
    }

    #[test]
    fn icons_that_arrive_mid_burst_wait_for_the_next_idle() {
        // An icon is read only at a path that resolves to itself.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/mock-icons/play.png")
            .canonicalize()
            .expect("the simulator's icons are in the repo");
        let (mut icons, mut atlas) = (Icons::threaded(), crate::atlas::Atlas::default());
        assert!(icons.get(&path, &mut atlas).is_none(), "on its way");
        // Mid-burst, however long the loader has had, nothing is taken in.
        let started = Instant::now();
        while started.elapsed() < ms(50) {
            assert!(!icons_arrived(&mut icons, false));
            std::thread::sleep(ms(1));
        }
        assert!(icons.is_loading(&path), "held back");
        // After an idle wait it comes in.
        while !icons_arrived(&mut icons, true) {
            assert!(started.elapsed() < Duration::from_secs(10), "never came");
            std::thread::sleep(ms(1));
        }
        assert!(!icons.is_loading(&path));
        assert!(icons.get(&path, &mut atlas).is_some());
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

    /// `n` ms after one moment shared by every test, as SDL would stamp it.
    fn at(n: u64) -> Instant {
        static ZERO: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *ZERO.get_or_init(Instant::now) + ms(n)
    }

    /// An event `n` ms in, on the loop's clock and SDL's alike.
    fn send<L: Launcher>(shell: &mut Shell<L>, event: Event, n: u64) {
        shell.event(event, ms(n), at(n));
    }

    fn down(code: i32) -> Event {
        Event::KeyDown {
            scancode: code,
            keycode: 0,
            repeat: false,
        }
    }

    fn key<L: Launcher>(shell: &mut Shell<L>, code: i32, n: u64) {
        send(shell, down(code), n);
        send(shell, Event::KeyUp { scancode: code }, n);
    }

    /// The repeats the loop fires after a pass's events, `n` ms in.
    fn poll<L: Launcher>(shell: &mut Shell<L>, n: u64) {
        for press in shell.input.poll(ms(n)) {
            shell.press(press, ms(n));
        }
    }

    #[test]
    fn keys_move_focus_and_ok_launches_through_the_launcher() {
        let mut shell = shell();
        key(&mut shell, scancode::RIGHT, 0);
        assert_eq!(shell.home.focused(), Some(1));
        key(&mut shell, scancode::RETURN, 100);
        assert!(
            !shell.launch_if_due(ms(120), || ms(120)),
            "not before the dip"
        );
        assert!(shell.launch_if_due(ms(134), || ms(134)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("amazon")]);
        assert_eq!(shell.home.view().status, Some("Opening Prime Video"));
        assert!(!shell.quit);
    }

    #[test]
    fn ok_shows_the_pulse_before_launching() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        assert!(shell.launcher.launched().is_empty(), "not before a frame");
        assert!(shell.wants_frame(), "the press pulse is drawn first");
        shell.frame_shown(at(17));
        assert!(!shell.keys.is_waiting(), "the press is on screen");
        assert!(
            !shell.launch_if_due(ms(17), || ms(17)),
            "the dip is not deepest yet"
        );
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
        assert!(
            !shell.launch_if_due(ms(50), || ms(50)),
            "one launch per press"
        );
    }

    #[test]
    fn a_second_ok_before_the_launch_launches_once_on_the_first_ones_time() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        key(&mut shell, scancode::RETURN, 5);
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert!(!shell.launch_if_due(ms(50), || ms(50)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
    }

    #[test]
    fn back_on_the_top_row_quits() {
        let mut shell = shell();
        key(&mut shell, scancode::WEBOS_BACK, 0);
        assert!(shell.quit);
    }

    #[test]
    fn back_on_the_top_row_settles_the_press_as_leaving() {
        let mut shell = shell();
        key(&mut shell, scancode::WEBOS_BACK, 0);
        assert!(shell.keys.is_waiting());
        shell.leaving(at(3));
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn a_failed_launch_says_so() {
        let mut shell = Shell::new(MockLauncher::default().failing_on("netflix"), ms(0));
        key(&mut shell, scancode::RETURN, 0);
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.home.view().status, Some("Could not open Netflix"));
    }

    #[test]
    fn a_press_waits_for_a_frame_but_sdl_and_shell_repeats_do_not() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        assert!(shell.keys.is_waiting());
        shell.frame_shown(at(17));
        assert!(!shell.keys.is_waiting());
        poll(&mut shell, 400);
        assert_eq!(shell.home.focused(), Some(2), "the shell repeated");
        assert!(!shell.keys.is_waiting(), "its repeats are not timed");
        let sdl_repeat = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, sdl_repeat, 450);
        assert!(!shell.keys.is_waiting(), "nor are SDL's");
    }

    #[test]
    fn unmapped_keys_are_not_timed() {
        let mut shell = shell();
        // 484 arrives around Magic Remote pointer movement on the B9.
        key(&mut shell, 484, 0);
        assert!(!shell.keys.is_waiting());
    }

    fn click(button: u8, down: bool) -> Event {
        Event::MouseButton {
            button,
            down,
            x: 1408,
            y: 431,
        }
    }

    #[test]
    fn a_left_click_is_ok_through_oks_own_path() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, click(1, true), 0);
            send(&mut shell, click(1, false), 150);
        });
        assert_eq!(
            lines,
            [
                "mouse button 1 down at 1408,431: taken as OK",
                "mouse button 1 up at 1408,431",
            ]
        );
        assert!(shell.wants_frame(), "the press pulse is drawn first");
        assert!(shell.launcher.launched().is_empty(), "not before the frame");
        assert!(shell.keys.is_waiting(), "timed like a press");
        let shown = said(|| shell.frame_shown(at(17)));
        assert_eq!(shown, ["key 40: shown after 17 ms"]);
        assert!(!shell.launch_if_due(ms(17), || ms(17)), "OK's own wait");
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
        assert!(!shell.input.is_holding(), "the up released it");
    }

    #[test]
    fn a_click_counts_as_a_key_for_the_focus_lines_and_shows_the_shell() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        let lines = said(|| {
            send(&mut shell, click(1, true), 1_000);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 3_500);
        });
        assert_eq!(lines[0], "mouse button 1 down at 1408,431: taken as OK");
        assert_eq!(lines[1], "in front again (a click): drawing resumes");
        assert_eq!(
            lines[2],
            "window focus lost: last key 2.5 s ago, 0 other events since"
        );
    }

    #[test]
    fn other_buttons_are_logged_and_ignored() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, click(3, true), 0);
            send(&mut shell, click(3, false), 100);
        });
        assert_eq!(
            lines,
            [
                "mouse button 3 down at 1408,431: ignored",
                "mouse button 3 up at 1408,431",
            ]
        );
        assert!(!shell.wants_frame());
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn the_pointer_codes_track_the_pointer_and_their_key_ups_are_not_input() {
        let mut shell = shell();
        let lines = said(|| {
            send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 1_000);
            send(&mut shell, down(scancode::WEBOS_CURSOR_HIDE), 6_000);
        });
        assert_eq!(lines, ["pointer shown", "pointer hidden"]);
        assert_eq!(shell.pointer, Pointer::Hidden);
        assert_eq!(shell.idle.last_input(), ms(6_000));
        // SDL's keyboard reset at a focus loss releases them; LG never does.
        // Such a key-up is not input, but it is still one more event in the
        // focus lines, as it was before the pointer was tracked.
        let up = Event::KeyUp {
            scancode: scancode::WEBOS_CURSOR_HIDE,
        };
        let lines = said(|| {
            send(&mut shell, up, 120_000);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 120_000);
        });
        assert_eq!(shell.idle.last_input(), ms(6_000));
        assert!(!shell.input.is_holding());
        assert_eq!(
            lines[0],
            "window focus lost: no key yet, 3 other events since"
        );
        // LG sends no key-up for 484, so SDL may flag the next one as a
        // repeat: the code, not the flag, sets the state and counts as input.
        let repeated_show = Event::KeyDown {
            scancode: scancode::WEBOS_CURSOR_SHOW,
            keycode: 0,
            repeat: true,
        };
        let lines = said(|| send(&mut shell, repeated_show, 130_000));
        assert_eq!(lines, ["pointer shown"]);
        assert_eq!(shell.pointer, Pointer::Shown);
        assert_eq!(shell.idle.last_input(), ms(130_000));
        // A repeat that changes nothing logs nothing; a fresh 484 does.
        assert!(said(|| send(&mut shell, repeated_show, 131_000)).is_empty());
        assert_eq!(
            said(|| send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 132_000)),
            ["pointer shown"]
        );
        assert!(!shell.keys.is_waiting(), "no action, nothing timed");
    }

    #[test]
    fn an_arrow_hides_a_shown_pointer_where_the_sdl_can() {
        let mut shell = shell();
        send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 0);
        key(&mut shell, scancode::RIGHT, 100);
        assert!(!shell.take_pointer_hide(), "this SDL cannot hide it");
        shell.can_hide_pointer = true;
        key(&mut shell, scancode::RIGHT, 200);
        assert!(shell.take_pointer_hide());
        assert!(!shell.take_pointer_hide(), "asked once per arrow");
        // Not for OK, nor for a repeat.
        key(&mut shell, scancode::RETURN, 300);
        assert!(!shell.take_pointer_hide());
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, held_right, 350);
        assert!(!shell.take_pointer_hide());
        // Once the loop has hidden it, no arrow asks again until LG shows it.
        shell.pointer_hidden();
        assert_eq!(shell.pointer, Pointer::Hidden);
        key(&mut shell, scancode::DOWN, 400);
        assert!(!shell.take_pointer_hide());
        send(&mut shell, down(scancode::WEBOS_CURSOR_SHOW), 450);
        key(&mut shell, scancode::DOWN, 500);
        assert!(shell.take_pointer_hide());
        // Nor once LG itself has hidden it.
        send(&mut shell, down(scancode::WEBOS_CURSOR_HIDE), 600);
        key(&mut shell, scancode::DOWN, 700);
        assert!(!shell.take_pointer_hide());
    }

    #[test]
    fn the_heartbeat_counts_from_the_last_input_of_any_kind() {
        let mut shell = shell();
        assert_eq!(shell.heartbeat(ms(29_000)), None);
        send(&mut shell, Event::MouseMotion, 10_000);
        assert_eq!(shell.heartbeat(ms(30_000)), None, "the motion was input");
        send(&mut shell, Event::Window(sdl::window::FOCUS_GAINED), 10_001);
        assert_eq!(
            shell.heartbeat(ms(40_000)).as_deref(),
            Some("no input for 30 s; focused yes; pointer hidden; power unknown")
        );
        assert_eq!(shell.idle.next_deadline(), ms(70_000));
        send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 50_000);
        assert_eq!(
            shell.heartbeat(ms(70_000)).as_deref(),
            Some("no input for 60 s; focused no; pointer hidden; power unknown")
        );
        // A key-up is input too, and so are an SDL repeat and the wheel.
        send(
            &mut shell,
            Event::KeyUp {
                scancode: scancode::RIGHT,
            },
            80_000,
        );
        assert_eq!(shell.idle.last_input(), ms(80_000));
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        send(&mut shell, held_right, 81_000);
        assert_eq!(shell.idle.last_input(), ms(81_000));
        send(&mut shell, Event::MouseWheel, 82_000);
        assert_eq!(shell.idle.last_input(), ms(82_000));
        // The watch counts from the start too, as if input had just come.
        assert_eq!(
            Shell::new(MockLauncher::default(), ms(5))
                .shared
                .last_input(),
            ms(5)
        );
        assert_eq!(
            shell.shared.last_input(),
            ms(82_000),
            "shared with the watch"
        );
    }

    #[test]
    fn a_launch_then_background_leaves_the_press_unshown() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        // No frame shows the press before the launch; then the app is put away.
        assert!(shell.launch_if_due(ms(34), || ms(34)));
        assert!(shell.keys.is_waiting());
        send(&mut shell, Event::Background, 320);
        assert!(!shell.keys.is_waiting(), "settled as not shown");
    }

    #[test]
    fn a_press_with_nothing_to_draw_settles_at_idle() {
        let mut shell = Shell::new(Broken, ms(0));
        shell.home.frame_drawn();
        key(&mut shell, scancode::RIGHT, 0);
        assert!(!shell.wants_frame(), "an empty grid does not move");
        assert!(shell.keys.is_waiting());
        shell.nothing_to_draw(at(1));
        assert!(!shell.keys.is_waiting());
    }

    #[test]
    fn foreground_is_timed_from_the_event_that_brought_the_shell_back() {
        let mut shell = shell();
        assert!(
            !shell.foreground.is_waiting(),
            "the start-up is not a return"
        );
        send(&mut shell, Event::Background, 0);
        send(&mut shell, Event::Foreground, 100);
        assert!(shell.foreground.is_waiting());
        shell.frame_shown(at(140));
        assert!(!shell.foreground.is_waiting());
    }

    #[test]
    fn a_key_while_hidden_starts_the_foreground_clock() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        key(&mut shell, scancode::RIGHT, 10);
        assert!(shell.foreground.is_waiting());
        assert!(shell.keys.is_waiting());
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 20);
        assert!(!shell.foreground.is_waiting() && !shell.keys.is_waiting());
    }

    /// The lines `f` logs, leaving out the raw `key down` lines.
    fn said(f: impl FnOnce()) -> Vec<String> {
        crate::log::capture(f)
            .into_iter()
            .filter(|line| !line.starts_with("key down"))
            .collect()
    }

    #[test]
    fn a_launch_reads_in_order_pulse_first() {
        let mut shell = shell();
        let lines = said(|| {
            key(&mut shell, scancode::RETURN, 0);
            shell.frame_shown(at(17));
            shell.launch_if_due(ms(34), || ms(34));
            send(&mut shell, Event::Background, 335);
        });
        assert_eq!(
            lines,
            [
                "key 40: shown after 17 ms",
                "launch netflix: answered in 0 ms",
                "app background: last key 0.3 s ago, 0 other events since",
                "the app went to the background: drawing stops",
            ]
        );
    }

    /// The mock's apps, launched after `ms` of waiting for an answer.
    struct Slow(MockLauncher, u64);

    impl Launcher for Slow {
        fn list(&mut self) -> Result<Vec<App>, LaunchError> {
            self.0.list()
        }
        fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
            std::thread::sleep(ms(self.1));
            self.0.launch(id)
        }
    }

    #[test]
    fn a_press_waiting_through_a_slow_launch_names_it() {
        let mut shell = Shell::new(Slow(MockLauncher::default(), 5), ms(0));
        shell.home.frame_drawn();
        shell.event(down(scancode::RETURN), ms(0), Instant::now());
        let lines = said(|| {
            // No frame shows the press before the launch, which blocks 5 ms.
            shell.launch_if_due(ms(34), || ms(39));
            shell.event(Event::Background, ms(40), Instant::now());
        });
        let line = lines
            .iter()
            .find(|line| line.starts_with("key 40:"))
            .expect("a line for the press");
        assert!(
            line.contains(" ms in launch): the app went to the background"),
            "{line}"
        );
    }

    #[test]
    fn the_launch_ends_the_burst_first() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        let mut burst = Burst::default();
        let t0 = Instant::now();
        let _ = burst.frame(t0);
        let _ = burst.frame(t0 + ms(16));
        let mut stats = FrameStats::new(Duration::from_secs(1));
        assert_eq!(stats.record(ms(900)), None);
        let mut clock = Clock::new(None, None);
        assert!(
            said(|| assert!(!launch_pending(
                &mut shell,
                &mut burst,
                &mut stats,
                10,
                ms(16),
                &mut clock
            )))
            .is_empty(),
            "not due: the burst goes on"
        );
        let lines = said(|| {
            assert!(launch_pending(
                &mut shell,
                &mut burst,
                &mut stats,
                10,
                ms(34),
                &mut clock
            ));
        });
        assert_eq!(
            lines[0],
            "drew 2 frames in 16 ms (avg 16.00 ms, worst 16.00 ms, 10 quads); first 16.00 ms; idle"
        );
        assert!(lines[1].starts_with("launch netflix: answered in "));
        assert!(
            burst.finish(0, None).is_empty(),
            "the wait for luna is no frame"
        );
        assert_eq!(stats.record(ms(200)), None, "the fps window started again");
    }

    #[test]
    fn a_focus_line_counts_what_came_since_the_last_key() {
        let mut shell = shell();
        key(&mut shell, scancode::RIGHT, 0);
        let lines = said(|| {
            send(&mut shell, Event::Other(0x400), 100);
            // A pointer code is not a remote key.
            send(&mut shell, down(485), 200);
            send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 5_300);
        });
        assert_eq!(
            lines,
            [
                "pointer hidden",
                "window focus lost: last key 5.3 s ago, 2 other events since"
            ]
        );
    }

    #[test]
    fn coming_back_names_its_cause_and_the_first_hide_is_kept() {
        let mut shell = shell();
        send(&mut shell, Event::Background, 0);
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 10);
        assert_eq!(shell.hidden_why, "the app went to the background");
        let lines = said(|| send(&mut shell, Event::Foreground, 20));
        assert_eq!(
            lines,
            [
                "app foreground: no key yet, 2 other events since",
                "in front again (did enter foreground): drawing resumes",
            ]
        );
    }

    #[test]
    fn focus_lines_name_only_the_window_events_that_matter() {
        use sdl::window::*;
        assert_eq!(window_name(FOCUS_LOST), Some("focus lost"));
        assert_eq!(window_name(RESTORED), Some("restored"));
        for noisy in [EXPOSED, RESIZED, SIZE_CHANGED] {
            assert_eq!(window_name(noisy), None);
        }
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
        key(&mut shell, scancode::DOWN, 0);
        assert!(shell.wants_frame());
        send(&mut shell, Event::Background, 10);
        assert!(!shell.visible && !shell.wants_frame());
        assert!(!shell.home.is_animating(), "springs jumped to rest");
        send(&mut shell, Event::Foreground, 20);
        assert!(shell.wants_frame(), "one fresh frame on return");
    }

    #[test]
    fn a_hidden_window_stops_drawing_and_drops_held_keys() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        assert!(shell.input.is_holding());
        send(&mut shell, Event::Window(sdl::window::MINIMIZED), 10);
        assert!(!shell.visible && !shell.input.is_holding());
        send(&mut shell, Event::Window(sdl::window::RESTORED), 20);
        assert!(shell.visible && shell.wants_frame());
    }

    #[test]
    fn a_key_while_hidden_means_the_shell_is_in_front_again() {
        let mut shell = shell();
        send(&mut shell, Event::Window(sdl::window::HIDDEN), 0);
        key(&mut shell, scancode::RIGHT, 10);
        assert!(shell.visible);
        assert_eq!(shell.home.focused(), Some(1));
    }

    #[test]
    fn losing_focus_drops_keys_and_stops_drawing_only_on_the_tv() {
        let mut shell = shell();
        send(&mut shell, down(scancode::LEFT), 0);
        send(&mut shell, Event::Window(sdl::window::FOCUS_LOST), 10);
        assert!(!shell.input.is_holding());
        assert_eq!(shell.visible, !on_tv());
    }

    #[test]
    fn held_arrows_repeat_through_poll() {
        let mut shell = shell();
        send(&mut shell, down(scancode::RIGHT), 0);
        poll(&mut shell, 400);
        poll(&mut shell, 520);
        assert_eq!(shell.home.focused(), Some(3));
    }

    #[test]
    fn key_events_and_clicks_reach_input_by_their_stamps() {
        // Let go at 510, read in the pass at 525: the repeat due at 520 is
        // not owed, by a key-up or a click's release alike.
        for release in [
            Event::KeyUp {
                scancode: scancode::RIGHT,
            },
            click(1, false),
        ] {
            let mut shell = shell();
            shell.stamps_from = Some(at(0));
            send(&mut shell, down(scancode::RIGHT), 0);
            poll(&mut shell, 400);
            shell.event(release, ms(525), at(510));
            assert_eq!(shell.home.focused(), Some(2), "{release:?}");
        }
        // SDL's repeats, queued through a 3 s stall and read at its end,
        // keep the key alive by their own stamps.
        let mut shell = shell();
        shell.stamps_from = Some(at(0));
        send(&mut shell, down(scancode::RIGHT), 0);
        let held_right = Event::KeyDown {
            scancode: scancode::RIGHT,
            keycode: 0,
            repeat: true,
        };
        for t in (500..=2_950).step_by(50) {
            shell.event(held_right, ms(3_000), at(t));
        }
        assert!(shell.input.is_holding(), "not dropped as stuck");
    }

    #[test]
    fn a_press_that_comes_with_a_repeat_is_the_one_timed() {
        for ok in [down(scancode::RETURN), click(1, true)] {
            let mut shell = shell();
            send(&mut shell, down(scancode::DOWN), 0);
            shell.frame_shown(at(17));
            // Down's first repeat is due at 400: OK at 450 fires it first.
            send(&mut shell, ok, 450);
            assert_eq!(
                said(|| shell.frame_shown(at(467))),
                ["key 40: shown after 17 ms"],
                "{ok:?}"
            );
        }
    }

    #[test]
    fn a_stamp_after_the_pass_counts_at_the_pass() {
        let mut shell = shell();
        assert_eq!(shell.stamp(at(40), ms(25)), ms(25), "no clock: the pass");
        shell.stamps_from = Some(at(0));
        assert_eq!(shell.stamp(at(10), ms(25)), ms(10));
        assert_eq!(shell.stamp(at(40), ms(25)), ms(25), "clamped");
    }

    /// The focused tile's lift in a frame drawn now.
    fn focused_lift<L: Launcher>(shell: &Shell<L>) -> f32 {
        let view = shell.home.view();
        view.tiles
            .iter()
            .find(|tile| tile.focused)
            .expect("a focused tile")
            .lift
    }

    #[test]
    fn the_launch_waits_for_a_frame_that_shows_the_dip() {
        let mut shell = shell();
        let (mut burst, mut stats) = (Burst::default(), FrameStats::new(Duration::from_secs(1)));
        let mut clock = Clock::new(None, None);
        key(&mut shell, scancode::RETURN, 0);
        // Frames as the loop draws them at 60 Hz: springs stepped to the
        // pass's time, the frame shown, then the launch if it is due.
        let mut lift_before_launch = None;
        for n in 0..10_u64 {
            let now = Duration::from_micros(n * 16_667);
            shell.home.advance_to(now);
            assert!(shell.wants_frame(), "frame {n}");
            let lift = focused_lift(&shell);
            shell.frame_shown(at(n * 17));
            shell.home.frame_drawn();
            if launch_pending(&mut shell, &mut burst, &mut stats, 0, now, &mut clock) {
                lift_before_launch = Some((n, lift));
                break;
            }
            assert!(shell.launcher.launched().is_empty(), "frame {n}");
        }
        let (n, lift) = lift_before_launch.expect("launched within 10 frames");
        assert_eq!(n, 2, "the frame at 33.3 ms");
        assert!(lift < 0.8, "the last frame before the launch dips: {lift}");
        assert_eq!(shell.launcher.launched(), &[AppId::new("netflix")]);
    }

    #[test]
    fn a_launch_asked_for_just_before_a_hide_is_dropped() {
        let mut shell = shell();
        let lines = said(|| {
            key(&mut shell, scancode::RETURN, 0);
            send(&mut shell, Event::Window(sdl::window::HIDDEN), 5);
        });
        assert!(
            lines.contains(&"launch netflix: dropped: the window was hidden".to_owned()),
            "{lines:?}"
        );
        assert!(!shell.launch_if_due(ms(100), || ms(100)));
        assert!(
            shell.launcher.launched().is_empty(),
            "nothing over LG's Home"
        );
    }

    #[test]
    fn a_launch_asked_for_just_before_leaving_is_dropped_with_a_line() {
        let mut shell = shell();
        key(&mut shell, scancode::RETURN, 0);
        key(&mut shell, scancode::WEBOS_BACK, 1);
        assert!(shell.quit, "BACK on the top row");
        let lines = said(|| shell.leaving(at(2)));
        assert_eq!(
            lines,
            [
                "key 40: not shown after 2 ms: the shell is leaving",
                "key 482: not shown after 1 ms: the shell is leaving",
                "launch netflix: dropped: the shell is leaving",
            ]
        );
        assert!(shell.launcher.launched().is_empty());
    }

    #[test]
    fn a_burst_reports_frames_and_times_then_resets() {
        let mut burst = Burst::default();
        assert!(burst.finish(0, None).is_empty());
        let t0 = Instant::now();
        assert_eq!(burst.frame(t0), None, "the first frame has no duration");
        assert_eq!(burst.frame(t0 + ms(16)), Some(ms(16)));
        assert_eq!(burst.frame(t0 + ms(40)), Some(ms(24)));
        assert_eq!(
            burst.finish(1234, None),
            [
                "drew 3 frames in 40 ms (avg 20.00 ms, worst 24.00 ms, 1234 quads); first 16.00, 24.00 ms; idle"
            ]
        );
        assert!(burst.finish(0, None).is_empty());
    }

    #[test]
    fn a_burst_names_only_its_first_three_timed_frames() {
        let mut burst = Burst::default();
        let t0 = Instant::now();
        for n in [0, 16, 40, 57, 73] {
            let _ = burst.frame(t0 + ms(n));
        }
        assert_eq!(
            burst.finish(10, None),
            [
                "drew 5 frames in 73 ms (avg 18.25 ms, worst 24.00 ms, 10 quads); first 16.00, 24.00, 17.00 ms; idle"
            ]
        );
    }

    #[test]
    fn a_one_frame_burst_has_no_first_times() {
        let mut burst = Burst::default();
        let _ = burst.frame(Instant::now());
        assert_eq!(
            burst.finish(24, None),
            ["drew 1 frames in 0 ms (avg 0.00 ms, worst 0.00 ms, 24 quads); idle"]
        );
    }

    fn swap_only(n: u64) -> FrameSplit {
        FrameSplit {
            swap: ms(n),
            ..FrameSplit::default()
        }
    }

    #[test]
    fn a_frame_over_25_ms_gets_a_line_with_its_place_in_the_burst() {
        let mut burst = Burst::default();
        let t0 = Instant::now();
        let _ = burst.frame(t0);
        let took = burst.frame(t0 + ms(25)).expect("timed");
        assert_eq!(
            burst.slow_line(took, &swap_only(25)),
            None,
            "25 ms is not over"
        );
        let took = burst.frame(t0 + ms(59)).expect("timed");
        let line = burst.slow_line(took, &swap_only(30)).expect("over 25 ms");
        assert!(
            line.starts_with("slow frame 3: 34.00 ms = loop 4.00 + "),
            "{line}"
        );
    }

    #[test]
    fn at_most_four_slow_frames_a_burst_are_logged_and_the_rest_counted() {
        let mut burst = Burst::default();
        let t0 = Instant::now();
        let _ = burst.frame(t0);
        let mut logged = 0;
        for i in 1..=6 {
            let took = burst.frame(t0 + ms(30 * i)).expect("timed");
            logged += usize::from(burst.slow_line(took, &swap_only(30)).is_some());
        }
        assert_eq!(logged, 4);
        let lines = burst.finish(10, None);
        assert_eq!(lines[0], "slow frames: 2 more not logged");
        assert!(
            lines[1].starts_with("drew 7 frames in 180 ms "),
            "{}",
            lines[1]
        );
        // The next burst starts its own count.
        let _ = burst.frame(t0 + ms(400));
        let took = burst.frame(t0 + ms(430)).expect("timed");
        assert!(burst.slow_line(took, &swap_only(30)).is_some());
    }
}
