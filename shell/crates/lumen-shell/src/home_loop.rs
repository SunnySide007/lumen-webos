//! The home loop (GLOSSARY.md). It reads the remote, lets
//! `lumen_core::home::Home` decide what happens, draws only while something
//! changes or moves, and waits in `SDL_WaitEventTimeout` once the screen has
//! settled (docs/specs/shell-mvp.md, "Repainting"). Every number it used to
//! show as hello-gles (GL renderer, frame times, key codes) goes to the log.
//!
//! It runs behind three adapters (decision 8): the events it reads, the
//! screen it draws on, and the wall clock. The TV's are SDL's queue
//! (`sdl.rs`), SDL's window and GL (`screen.rs`) and the system clock; the
//! tests' are in `home_loop/fake.rs`. The loop keeps every decision, and the
//! adapters only carry them out.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::apps::{App, LaunchError, Launcher};
use lumen_core::home::Home;
use lumen_core::input::Input;
use lumen_core::script::Script;

use crate::app::Options;
use crate::frame_stats::{BurstEnd, FrameStats};
use crate::gl;
use crate::info;
use crate::sdl::{Event, Refresh};
use crate::timing::{Faults, FrameSplit, StartTimes};
use crate::watch;

mod clock;
#[cfg(test)]
mod fake;
mod shell;

use clock::Clock;
use shell::Shell;

/// When `--screenshot` takes its frame.
const SCREENSHOT_AFTER: Duration = Duration::from_millis(1500);
/// How often a settled screen looks for icons while the loader has some.
const ICON_POLL: Duration = Duration::from_millis(16);
/// How long the screen stays settled before glyphs are made ahead: three
/// refreshes at 60 Hz, so the burst's last frames, at most two still queued
/// on the B9, have gone out and the GPU is done with the atlas.
const WARM_AFTER: Duration = Duration::from_millis(50);

/// The events the loop reads: SDL's queue on the TV.
pub trait Events {
    /// The next event, if one is queued, with the time it happened.
    fn poll(&mut self) -> Option<(Event, Instant)>;
    /// Waits up to `timeout` for an event, or for as long as it takes with
    /// `None`.
    fn wait(&mut self, timeout: Option<Duration>) -> Option<(Event, Instant)>;
    /// Whether `hide_pointer` can hide the Magic Remote's pointer.
    fn can_hide_pointer(&self) -> bool;
    /// Hides the pointer; false, and nothing done, where it cannot.
    fn hide_pointer(&mut self) -> bool;
}

/// What the loop draws on and asks about the display: SDL's window and GL
/// on the TV.
pub trait Screen {
    /// Paints `home`'s view into the back buffer. The view is made in here,
    /// so its time is the frame's `batch` time, as drawing it is.
    fn paint(&mut self, home: &Home);
    /// Saves the frame painted last, before its swap, as a PNG.
    fn screenshot(&mut self, path: &Path);
    /// Shows the frame painted last.
    fn swap(&mut self) -> Frame;
    /// Takes in the icons the loader has finished; true if any came.
    fn take_icons(&mut self) -> bool;
    /// Whether any icon is still on its way.
    fn icons_loading(&self) -> bool;
    /// Makes every glyph `apps`' titles need and uploads them, and logs the
    /// `warm` line.
    fn warm(&mut self, apps: &[App]);
    /// What the display says its refresh rate is.
    fn refresh(&mut self) -> Refresh;
    /// GL's error flag, which reading clears.
    fn gl_error(&mut self) -> u32;
}

/// The wall clock.
pub trait WallClock {
    fn now(&self) -> Instant;
}

/// The TV's wall clock: the system's.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl WallClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A frame the screen showed.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// When its swap returned.
    pub shown: Instant,
    /// Where its time went and what it did, its faults unset.
    pub split: FrameSplit,
    /// The fault counters read after the swap, `None` where they are not
    /// counted.
    pub faults: Option<Faults>,
    /// The quads it drew.
    pub quads: usize,
}

/// What start-up hands the loop, besides its adapters and the options.
pub struct Start<L> {
    /// The start-up phases, for the `start:` line.
    pub times: StartTimes,
    /// When the loop's clock starts.
    pub zero: Instant,
    /// When the apps were about to be listed: the loop counts its first
    /// input from then, as if input had just come.
    pub listing: Instant,
    pub launcher: L,
    /// The apps listed, or why there are none.
    pub apps: Result<Vec<App>, LaunchError>,
    /// What the luna watch thread reads and writes.
    pub shared: Arc<watch::Shared>,
    /// Whether the screen's swaps wait for vsync.
    pub vsync: bool,
}

/// Runs the home loop until the shell leaves.
pub fn run<E: Events, S: Screen, W: WallClock, L: Launcher>(
    events: &mut E,
    screen: &mut S,
    wall: &W,
    opts: &Options,
    start: Start<L>,
) {
    // Motion is paced only with vsync and the real clock, from the first idle
    // on: the rate is asked for there, not before the first frame, since
    // LG's call may ask luna. A fixed clock asks SDL nothing, so its log
    // stays as it was.
    let mut refresh_due = start.vsync && opts.fixed_dt.is_none();
    let mut stats = FrameStats::new();
    let mut clock = Clock::new(wall, start.zero, opts.fixed_dt, None);
    let mut screenshot = opts.screenshot.clone();
    let mut settled_shot = opts.screenshot_when_settled.clone();
    let mut script = opts.keys.clone();
    // Every title's glyphs are made once (`warm`), while visible, when the
    // screen has stayed settled `WARM_AFTER` past a burst; a press before
    // then puts it off to the next settle. Not under a fixed clock: warming
    // changes where glyphs sit in the atlas, and so the frames.
    let mut warmed = clock.is_fixed();
    // When, in real time, it is due.
    let mut warm_at: Option<Duration> = None;
    let mut shell = Shell::new(
        start.launcher,
        start.apps,
        start.shared,
        clock.at(start.listing),
    );
    shell.can_hide_pointer = events.can_hide_pointer();
    shell.stamps_from = (!clock.is_fixed()).then_some(clock.start);
    // The last frame's quads, for the frame lines.
    let mut quads = 0;
    // Logged once, with the first frame or with why there was none.
    let mut start_line = Some(start.times);

    loop {
        // A settled screenshot that is ready must not wait for an event; one
        // that cannot be drawn yet (window hidden, icons on their way) must
        // not spin either.
        let shot_ready = settled_shot.is_some()
            && shell.visible
            && keys_done(script.as_ref(), &shell.input)
            && !screen.icons_loading();
        // A pass that starts with an idle wait: the GPU has most likely
        // finished with the atlas, so icons may be placed in it.
        let waited = !(shell.wants_frame() || shot_ready);
        let first = if !waited {
            events.poll()
        } else {
            shell.nothing_to_draw(wall.now());
            // Drawing stopped: the burst is over, and with it the `fps` window.
            let over = stats.burst_over(BurstEnd::Idle, quads, clock.take_skipped());
            for line in &over.lines {
                info!("{line}");
            }
            clock.idle();
            if std::mem::take(&mut refresh_due) {
                let refresh = screen.refresh();
                info!("{}", refresh.line());
                clock.pace(refresh.period().0);
            }
            if !warmed && !over.lines.is_empty() {
                warm_at = Some(clock.real() + WARM_AFTER);
            }
            // Every title's glyphs, once the screen has stayed settled for
            // `WARM_AFTER`, so no burst waits on an upload and the GPU is
            // most likely done with the atlas. Any wait left lands in the
            // `warm` line's time, once, outside every burst.
            if shell.visible && warm_at.is_some_and(|at| clock.real() >= at) {
                screen.warm(shell.home.apps());
                warmed = true;
                warm_at = None;
            }
            if over.read_gl_error {
                gl_error(screen);
            }
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
                (shell.visible && screen.icons_loading()).then(|| clock.now() + ICON_POLL),
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
                    events.poll()
                }
                Some(t) => {
                    // By real time: a paced clock may run ahead of it, and
                    // waiting by that would wake early and spin.
                    let wait = t.saturating_sub(clock.real());
                    events.wait(Some(until_exit.map_or(wait, |e| e.min(wait))))
                }
                None => events.wait(until_exit),
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
                shell.event(event, now, wall.now());
            }
        }
        if let Some((event, at)) = first {
            shell.event(event, now, at);
        }
        while let Some((event, at)) = events.poll() {
            shell.event(event, now, at);
        }
        // After the events, which fired the repeats due by their own stamps
        // (so a key let go just now fires none after), the repeats due by the
        // pass's input time. They act at `now`, as presses do.
        for press in shell.input.poll(clock.input_time(pass)) {
            shell.press(press, now);
        }
        if shell.take_pointer_hide() && events.hide_pointer() {
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
        if icons_arrived(screen, waited) {
            shell.home.mark_dirty();
        }
        shell.home.advance_to(clock.shows(now));
        // The settled screenshot: the keys are all played and released,
        // nothing moves and every icon is in, so draw the settled frame once
        // more and keep it.
        let take_settled = settled_shot.is_some()
            && keys_done(script.as_ref(), &shell.input)
            && !screen.icons_loading()
            && !shell.wants_frame();
        if take_settled {
            shell.home.mark_dirty();
        }
        if !shell.wants_frame() {
            // No frame to draw: a launch goes once its time has come, and
            // until then it is one of the idle deadlines.
            launch_pending(&mut shell, &mut stats, quads, now, &mut clock);
            continue;
        }

        screen.paint(&shell.home);
        if now >= SCREENSHOT_AFTER
            && let Some(path) = screenshot.take()
        {
            screen.screenshot(&path);
        }
        if take_settled && let Some(path) = settled_shot.take() {
            screen.screenshot(&path);
            info!("settled at {:.3} s on the loop's clock", now.as_secs_f64());
            shell.quit = true;
        }
        let frame = screen.swap();
        quads = frame.quads;
        if let Some(start) = start_line.take() {
            info!("{}", start.line(frame.shown));
        }
        shell.frame_shown(frame.shown);
        shell.home.frame_drawn();
        clock.frame_drawn(pass);
        if shell.quit {
            break;
        }

        let said = stats.frame(frame.shown, frame.split, frame.faults, quads);
        for line in &said.lines {
            info!("{line}");
        }
        if said.read_gl_error {
            gl_error(screen);
        }
        // The frame just drawn is on screen, showing `clock.shows(now)`: once
        // that is past the dip's deepest point, the launch may block the loop.
        launch_pending(&mut shell, &mut stats, quads, clock.shows(now), &mut clock);
    }
    shell.leaving(wall.now());
    if let Some(start) = start_line.take() {
        info!("{}", start.line_without_frame("the shell is leaving"));
    }
    let over = stats.burst_over(BurstEnd::Exit, quads, clock.take_skipped());
    for line in &over.lines {
        info!("{line}");
    }
}

/// Logs GL's error flag, which holds until read, if it is set.
fn gl_error(screen: &mut impl Screen) {
    let err = screen.gl_error();
    if err != gl::NO_ERROR {
        info!("GL error {err:#x}");
    }
}

/// Launches what a press asked for, if it is due by `now`. The burst, with
/// its `fps` window, and the pacer's burst end first, so the time luna takes
/// to answer is not counted as a frame, nor as vsyncs skipped. Returns
/// whether it launched.
fn launch_pending<L: Launcher, W: WallClock>(
    shell: &mut Shell<L>,
    stats: &mut FrameStats,
    quads: usize,
    now: Duration,
    clock: &mut Clock<'_, W>,
) -> bool {
    if !shell.launch_due_at().is_some_and(|due| due <= now) {
        return false;
    }
    let over = stats.burst_over(BurstEnd::Launch, quads, clock.take_skipped());
    for line in &over.lines {
        info!("{line}");
    }
    clock.idle();
    shell.launch_if_due(now, clock.wall(), || clock.now())
}

/// Takes in the icons the loader has finished, but only on a pass that began
/// with an idle wait: placed mid-burst, an icon would be uploaded into the
/// atlas that the frames in flight still read. (Taking it in only keeps the
/// decoded image; it goes into the atlas when a frame draws it.) True if any
/// came.
fn icons_arrived(screen: &mut impl Screen, waited: bool) -> bool {
    waited && screen.take_icons()
}

/// Every scripted key has been pressed and released.
fn keys_done(script: Option<&Script>, input: &Input) -> bool {
    script.is_none_or(Script::is_done) && !input.is_holding()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use lumen_core::apps::{AppId, MockLauncher};
    use lumen_core::input::scancode;

    use crate::sdl::window;
    use fake::{Act, Acts, FakeEvents, FakeScreen, ManualClock, Painted, SharedLauncher, down};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The fakes for one run of the loop: no events, 60 Hz swaps, the mock's
    /// apps, the real clock unpaced (no vsync) and no flags.
    struct Setup {
        clock: ManualClock,
        acts: Acts,
        events: FakeEvents,
        screen: FakeScreen,
        launcher: SharedLauncher,
        opts: Options,
        vsync: bool,
    }

    /// What a run left: its log lines (the raw `key down` lines left out),
    /// what the loop asked of the fakes, the frames shown and the launches.
    struct Ran {
        lines: Vec<String>,
        acts: Vec<Act>,
        frames: Vec<Painted>,
        warm_uploads: usize,
        launched: Vec<AppId>,
    }

    impl Setup {
        fn new() -> Self {
            let clock = ManualClock::new();
            let acts = Acts::default();
            Self {
                events: FakeEvents::new(&clock, &acts),
                screen: FakeScreen::new(&clock, &acts),
                launcher: SharedLauncher::new(MockLauncher::default(), &clock, &acts),
                opts: Options::default(),
                vsync: false,
                clock,
                acts,
            }
        }

        fn events(mut self, f: impl FnOnce(FakeEvents) -> FakeEvents) -> Self {
            self.events = f(self.events);
            self
        }

        fn run(mut self) -> Ran {
            let zero = self.clock.now();
            let apps = self.launcher.list();
            let start = Start {
                times: StartTimes::new(zero),
                zero,
                listing: zero,
                launcher: self.launcher.clone(),
                apps,
                shared: watch::Shared::new(),
                vsync: self.vsync,
            };
            let lines = crate::log::capture(|| {
                super::run(
                    &mut self.events,
                    &mut self.screen,
                    &self.clock,
                    &self.opts,
                    start,
                );
            });
            Ran {
                lines: lines
                    .into_iter()
                    .filter(|line| !line.starts_with("key down"))
                    .collect(),
                acts: self.acts.take(),
                frames: self.screen.frames,
                warm_uploads: self.screen.warm_uploads,
                launched: self.launcher.launched(),
            }
        }
    }

    impl Ran {
        /// When luna was asked for the first launch.
        fn launched_at(&self) -> Duration {
            self.acts
                .iter()
                .find_map(|act| match act {
                    Act::Launch(at) => Some(*at),
                    _ => None,
                })
                .expect("a launch")
        }

        /// The frames shown up to `at`.
        fn frames_by(&self, at: Duration) -> impl Iterator<Item = &Painted> {
            self.frames.iter().filter(move |f| f.shown <= at)
        }

        fn has_line(&self, start: &str) -> bool {
            self.lines.iter().any(|line| line.starts_with(start))
        }

        /// The idle waits with a timeout: when each began, and when it was
        /// to run out.
        fn waits(&self) -> impl Iterator<Item = (Duration, Duration)> + '_ {
            self.acts.iter().filter_map(|act| match act {
                Act::Wait(at, Some(timeout)) => Some((*at, *at + *timeout)),
                _ => None,
            })
        }

        /// Each `drew` line's count of vsyncs skipped, `None` where it has
        /// none.
        fn skipped(&self) -> Vec<Option<u32>> {
            self.lines
                .iter()
                .filter(|line| line.starts_with("drew "))
                .map(|line| {
                    line.split("; ")
                        .find_map(|part| part.strip_suffix(" vsyncs skipped")?.parse().ok())
                })
                .collect()
        }
    }

    fn up(code: i32) -> Event {
        Event::KeyUp { scancode: code }
    }

    // The three bugs 5726aa3 fixed in the loop, now reached through it.

    #[test]
    fn the_launch_waits_for_a_frame_that_shows_the_dip() {
        // Paced as on the TV, and unpaced: either way the frame before the
        // launch shows the press pulse's dip, not the press at dt 0.
        for vsync in [true, false] {
            let mut setup =
                Setup::new().events(|e| e.tap(1_000, scancode::RETURN).at(3_000, Event::Quit));
            setup.vsync = vsync;
            let ran = setup.run();
            assert_eq!(ran.launched, [AppId::new("netflix")], "vsync {vsync}");
            let at = ran.launched_at();
            assert!(at >= ms(1_033), "vsync {vsync}: launched at {at:?}");
            let (index, lift) = ran
                .frames_by(at)
                .last()
                .and_then(|f| f.focused)
                .expect("a frame before the launch");
            assert_eq!(index, 0, "vsync {vsync}");
            assert!(
                lift < 0.8,
                "vsync {vsync}: the frame before the launch dips: {lift}"
            );
        }
    }

    #[test]
    fn a_release_read_after_the_next_repeat_fell_due_fires_none() {
        // Right held from 1 s repeats at 1.4 s. It is let go at 1.515 s, just
        // before the repeat due at 1.52 s, but the pass that reads the
        // release runs after that: the release's stamp decides. (The 1.4 s
        // repeat's burst draws a frame every 16.7 ms, so passes run at about
        // 1.517 s and 1.533 s, either side of the release's arrival.)
        let ran = Setup::new()
            .events(|e| {
                e.at(1_000, down(scancode::RIGHT))
                    .late(1_515, 1_525, up(scancode::RIGHT))
                    .at(3_000, Event::Quit)
            })
            .run();
        let focused = |f: &Painted| f.focused.map(|(index, _)| index);
        assert_eq!(ran.frames.iter().filter_map(focused).max(), Some(2));
        assert_eq!(ran.frames.last().and_then(focused), Some(2));
    }

    #[test]
    fn the_fps_window_starts_again_after_a_hide() {
        // Taps 0.3 s apart keep one burst going for most of a second, then
        // the window is hidden; back in front, one more tap. Neither burst
        // fills a second on its own, so neither has an `fps` line.
        let ran = Setup::new()
            .events(|e| {
                e.tap(1_000, scancode::RIGHT)
                    .tap(1_300, scancode::RIGHT)
                    .tap(1_600, scancode::RIGHT)
                    .at(1_800, Event::Window(window::HIDDEN))
                    .at(3_000, Event::Window(window::SHOWN))
                    .tap(3_000, scancode::RIGHT)
                    .at(5_000, Event::Quit)
            })
            .run();
        assert!(ran.has_line("the window was hidden: drawing stops"));
        assert!(!ran.has_line("fps "), "{:?}", ran.lines);
        // GL's flag is still read at each burst's end.
        let reads = ran.acts.iter().filter(|a| matches!(a, Act::GlRead(_)));
        assert!(reads.count() >= 3, "start-up, the hide and the last tap");
    }

    // The loop's clock: real, paced or fixed.

    #[test]
    fn a_fixed_clock_never_waits_and_moves_one_step_per_frame() {
        let mut setup = Setup::new();
        setup.vsync = true;
        setup.opts.fixed_dt = Some(ms(20));
        setup.opts.keys = Some(Script::parse("R").expect("a key"));
        setup.opts.screenshot_when_settled = Some(PathBuf::from("settled.png"));
        let ran = setup.run();
        assert!(
            !ran.acts
                .iter()
                .any(|a| matches!(a, Act::Wait(..) | Act::Refresh(_) | Act::Warm(_))),
            "no waits, no pacing and no warm under a fixed clock"
        );
        // R goes down at 0.5 s; each frame then moves the clock 20 ms, and
        // the settled frame is drawn at a whole number of them.
        let settled = ran
            .lines
            .iter()
            .find_map(|l| l.strip_prefix("settled at "))
            .and_then(|l| l.strip_suffix(" s on the loop's clock"))
            .and_then(|s| s.parse::<f64>().ok())
            .expect("a settled line");
        let steps = settled / 0.020;
        assert!((steps - steps.round()).abs() < 1e-6, "{settled} s");
        assert!(settled > 0.5, "after the key: {settled} s");
    }

    #[test]
    fn a_real_clock_waits_by_the_wall_clock_until_the_next_deadline() {
        let ran = Setup::new().events(|e| e.at(60_000, Event::Quit)).run();
        // After the first frame, the first wait runs out when the glyphs are
        // due, `WARM_AFTER` on; then nothing is due but the idle heartbeat,
        // 30 s after the start, and the next wait runs out exactly there.
        let ends: Vec<_> = ran.waits().map(|(_, end)| end).collect();
        assert_eq!(
            ends[..2],
            [fake::FRAME + WARM_AFTER, Duration::from_secs(30)],
            "{:?}",
            ran.acts
        );
        assert!(ran.has_line("no input for 30 s;"));
        // Paced, with swaps faster than the period, a tap's motion runs ahead
        // of real time. The wait after its burst still goes by real time, out
        // to the heartbeat 30 s after the tap.
        let mut setup =
            Setup::new().events(|e| e.tap(1_000, scancode::RIGHT).at(60_000, Event::Quit));
        setup.vsync = true;
        setup.screen = setup.screen.frame_time(ms(5));
        let ran = setup.run();
        let after_tap = ran.waits().find(|&(at, _)| at > ms(1_000));
        assert_eq!(
            after_tap.map(|(_, end)| end),
            Some(Duration::from_secs(31)),
            "{:?}",
            ran.acts
        );
    }

    #[test]
    fn a_paced_clock_shows_each_frame_one_period_after_its_pass() {
        // Unpaced, the first frame after a press is drawn at the press's own
        // time and shows the new tile still at rest; paced, it shows one
        // period on, already lifting.
        for vsync in [false, true] {
            let mut setup =
                Setup::new().events(|e| e.tap(1_000, scancode::RIGHT).at(3_000, Event::Quit));
            setup.vsync = vsync;
            let ran = setup.run();
            let refreshes = ran.acts.iter().filter(|a| matches!(a, Act::Refresh(_)));
            assert_eq!(
                refreshes.count(),
                usize::from(vsync),
                "read once, at the first idle"
            );
            let first = ran
                .frames
                .iter()
                .find(|f| f.focused.is_some_and(|(index, _)| index == 1))
                .and_then(|f| f.focused)
                .expect("a frame with the new focus");
            assert_eq!(first.1 > 0.0, vsync, "vsync {vsync}: lift {}", first.1);
        }
    }

    // What the screen says of each frame.

    #[test]
    fn a_slow_swap_is_a_slow_frame_with_no_fault_reading() {
        // Every swap takes 30 ms, so each frame of the tap's burst after its
        // first is slow; the fake reads no fault counters.
        let mut setup =
            Setup::new().events(|e| e.tap(1_000, scancode::RIGHT).at(3_000, Event::Quit));
        setup.screen = setup.screen.frame_time(ms(30));
        let ran = setup.run();
        let slow: Vec<_> = ran
            .lines
            .iter()
            .filter(|l| l.starts_with("slow frame "))
            .collect();
        assert!(!slow.is_empty(), "{:?}", ran.lines);
        assert!(slow.iter().all(|l| l.ends_with("; faults -")), "{slow:?}");
    }

    #[test]
    fn a_paced_burst_counts_the_vsyncs_its_frames_skipped() {
        // Two taps a second apart, paced at 60 Hz. With swaps of one period
        // neither burst skips a vsync, the settle between them included;
        // with 30 ms swaps both do. Unpaced, no line has a count, and nor has
        // the start-up frame's, drawn before the rate is read.
        let skipped = |vsync: bool, swap: Duration| {
            let mut setup = Setup::new().events(|e| {
                e.tap(1_000, scancode::RIGHT)
                    .tap(2_000, scancode::RIGHT)
                    .at(3_000, Event::Quit)
            });
            setup.vsync = vsync;
            setup.screen = setup.screen.frame_time(swap);
            setup.run().skipped()
        };
        assert_eq!(skipped(true, fake::FRAME), [None, Some(0), Some(0)]);
        let slow = skipped(true, ms(30));
        assert!(
            matches!(slow[..], [None, Some(a), Some(b)] if a > 0 && b > 0),
            "{slow:?}"
        );
        assert_eq!(skipped(false, ms(30)), [None, None, None]);
    }

    // A launch, and what waits for one.

    #[test]
    fn the_launch_ends_the_burst_first() {
        // Luna takes 0.3 s to answer, which no frame and no burst counts.
        let mut setup =
            Setup::new().events(|e| e.tap(1_000, scancode::RETURN).at(3_000, Event::Quit));
        setup.launcher = setup.launcher.taking(ms(300));
        let ran = setup.run();
        let launch = ran
            .lines
            .iter()
            .position(|l| l.starts_with("launch netflix: answered in 300 ms"))
            .expect("a launch line");
        assert!(
            ran.lines[launch - 1].starts_with("drew "),
            "{:?}",
            ran.lines
        );
        assert!(!ran.has_line("slow frame"), "{:?}", ran.lines);
    }

    #[test]
    fn icons_are_taken_in_only_after_an_idle_wait() {
        // The icon arrives in the middle of a tap's burst.
        let mut setup =
            Setup::new().events(|e| e.tap(1_000, scancode::RIGHT).at(3_000, Event::Quit));
        setup.screen = setup.screen.icon_at(1_100);
        let ran = setup.run();
        let mut waited = false;
        let mut taken = None;
        for act in &ran.acts {
            match act {
                Act::Wait(..) => waited = true,
                Act::Swap(_) => waited = false,
                Act::TakeIcons(at, came) => {
                    assert!(waited, "a look for icons mid-burst at {at:?}");
                    if *came {
                        taken = Some(*at);
                    }
                }
                _ => {}
            }
        }
        let taken = taken.expect("the icon came in");
        let burst_end = ran.frames_by(ms(1_100)).count();
        assert!(
            ran.frames[burst_end..].iter().any(|f| f.shown <= taken),
            "held back to the idle"
        );
        assert!(ran.frames.iter().any(|f| f.shown > taken), "and then drawn");
    }

    #[test]
    fn the_settled_screenshot_waits_for_the_keys_to_be_played_and_released() {
        // Right is held from 0.1 s to 1.4 s, past the scripted keys (and short
        // of the 1.5 s a key may go without SDL repeats before it counts as
        // released). Its repeats reach the row's end, and the screen settles
        // at about 1.12 s with it still held.
        let mut setup = Setup::new().events(|e| {
            e.at(100, down(scancode::RIGHT))
                .at(1_400, up(scancode::RIGHT))
        });
        setup.opts.keys = Some(Script::parse("D").expect("a key"));
        setup.opts.screenshot_when_settled = Some(PathBuf::from("settled.png"));
        let ran = setup.run();
        let shots: Vec<_> = ran
            .acts
            .iter()
            .filter_map(|a| match a {
                Act::Screenshot(at) => Some(*at),
                _ => None,
            })
            .collect();
        assert_eq!(shots.len(), 1, "one shot, then the loop leaves");
        assert!(
            ran.acts
                .iter()
                .any(|a| matches!(a, Act::Wait(at, _) if (ms(1_000)..ms(1_400)).contains(at))),
            "settled, it waits while the key is held: {:?}",
            ran.acts
        );
        assert_eq!(shots[0], ms(1_400), "in the pass that reads the release");
        assert!(ran.has_line("settled at "));
    }

    #[test]
    fn warm_titles_leave_no_upload_for_a_burst() {
        // A walk over the mock grid, paced as on the TV: the glyphs are made
        // 50 ms into the first settle, and no burst after uploads any.
        let walk = [
            scancode::RIGHT,
            scancode::RIGHT,
            scancode::RIGHT,
            scancode::RIGHT,
            scancode::DOWN,
            scancode::RIGHT,
            scancode::DOWN,
            scancode::LEFT,
            scancode::UP,
            scancode::UP,
            scancode::RETURN,
            scancode::WEBOS_BACK,
        ];
        let walked = |fixed_dt: Option<Duration>| {
            let mut setup = Setup::new().events(|mut e| {
                for (n, &code) in (1..).zip(&walk) {
                    e = e.tap(500 * n, code);
                }
                e.at(60_000, Event::Quit)
            });
            setup.vsync = true;
            setup.opts.fixed_dt = fixed_dt;
            setup.run()
        };
        let ran = walked(None);
        assert_eq!(ran.launched.len(), 1, "OK launched");
        let first_wait = ran
            .acts
            .iter()
            .find_map(|a| match a {
                Act::Wait(at, _) => Some(*at),
                _ => None,
            })
            .expect("a first settle");
        let warmed = ran
            .acts
            .iter()
            .find_map(|a| match a {
                Act::Warm(at) => Some(*at),
                _ => None,
            })
            .expect("a warm");
        assert_eq!(warmed, first_wait + WARM_AFTER);
        assert!(ran.warm_uploads > 0);
        let after: Vec<_> = ran.frames.iter().filter(|f| f.shown > warmed).collect();
        assert!(after.len() > 100, "the walk drew {} frames", after.len());
        assert!(after.iter().all(|f| f.uploads == 0), "{after:?}");
        // Cold, as a fixed clock leaves it, the same walk uploads mid-burst.
        let cold = walked(Some(fake::FRAME));
        assert!(cold.frames.iter().skip(1).map(|f| f.uploads).sum::<usize>() > 0);
    }
}
