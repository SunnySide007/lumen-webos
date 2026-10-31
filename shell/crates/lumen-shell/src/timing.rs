//! The shell's timing lines (docs/specs/look-and-feel.md on main, "Log
//! lines"): how long a press takes to reach the screen, how long start-up and
//! coming back to the front take, and how long a launch waits for luna. Each
//! type builds its lines as strings, so the formats are pinned by the tests
//! below and `tools/frame-stats.sh` (on main) can rely on them; the loop only
//! logs them.
//!
//! "Shown" means `SDL_GL_SwapWindow` returned. With vsync the frame is then
//! queued, not yet on the panel. On the B9 the queue holds at least two
//! frames (a burst's first two go in without a wait, frame 2 about 9 ms
//! after frame 1, and frame 3 waits; stage 1 run, 2026-10-22), so the screen
//! may lag the line by up to two refreshes (33 ms at 60 Hz), more if the
//! queue is deeper; the remote-to-screen time is measured with a slow-motion
//! video instead (target 3). With motion paced by vsync, a frame shows its
//! pass plus one period, so the first frame shown after a press has already
//! moved; unpaced (`--fixed-dt`, no vsync) it is drawn at the press's own
//! time and may still show the screen as it was.

use std::time::{Duration, Instant};

use lumen_core::apps::{AppId, LaunchError};

/// The start-up phases, in the order they end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// SDL loaded and initialised.
    Sdl,
    /// The window, its GL context and vsync.
    Gl,
    /// Shaders compiled and buffers made.
    Renderer,
    /// The font and the atlas, and the icon loader started.
    Assets,
    /// The apps listed (luna on the TV).
    Apps,
}

impl Phase {
    const ALL: [Self; 5] = [
        Self::Sdl,
        Self::Gl,
        Self::Renderer,
        Self::Assets,
        Self::Apps,
    ];

    /// The phase's name in the `start:` line.
    const fn name(self) -> &'static str {
        match self {
            Self::Sdl => "sdl",
            Self::Gl => "gl",
            Self::Renderer => "renderer",
            Self::Assets => "assets",
            Self::Apps => "apps",
        }
    }
}

/// When each start-up phase ended, counted from `main()`'s first line.
#[derive(Debug)]
pub struct StartTimes {
    zero: Instant,
    marks: [Option<Instant>; Phase::ALL.len()],
}

impl StartTimes {
    /// Phases are counted from `zero`.
    pub fn new(zero: Instant) -> Self {
        Self {
            zero,
            marks: [None; Phase::ALL.len()],
        }
    }

    /// `phase` ended at `at`.
    pub fn mark(&mut self, phase: Phase, at: Instant) {
        self.marks[phase as usize] = Some(at);
    }

    /// `start: sdl N, gl N, renderer N, assets N, apps N, first frame N ms`.
    pub fn line(&self, first_frame: Instant) -> String {
        format!(
            "start: {}, first frame {} ms",
            self.phases(),
            whole_ms(first_frame.saturating_duration_since(self.zero))
        )
    }

    /// `start: sdl N, gl N, renderer N, assets N, apps N ms; no first frame: <why>`.
    pub fn line_without_frame(&self, why: &str) -> String {
        format!("start: {} ms; no first frame: {why}", self.phases())
    }

    /// Each phase's end from the start; `-` for one never reached.
    fn phases(&self) -> String {
        let mut out = String::new();
        for (phase, mark) in Phase::ALL.iter().zip(self.marks) {
            if !out.is_empty() {
                out.push_str(", ");
            }
            let shown = mark.map_or_else(
                || "-".to_owned(),
                |at| whole_ms(at.saturating_duration_since(self.zero)),
            );
            out.push_str(&format!("{} {shown}", phase.name()));
        }
        out
    }
}

/// A time in whole milliseconds, rounded, as every timing line shows it.
fn whole_ms(d: Duration) -> String {
    format!("{:.0}", d.as_secs_f64() * 1000.0)
}

/// `launch <id>: answered in N ms`, or `failed after N ms: <error>`.
pub fn launch_line(id: &AppId, took: Duration, result: &Result<(), LaunchError>) -> String {
    match result {
        Ok(()) => format!("launch {id}: answered in {} ms", whole_ms(took)),
        Err(e) => format!("launch {id}: failed after {} ms: {e}", whole_ms(took)),
    }
}

/// `launch <id>: dropped: <why>`: a launch asked for but never called, since
/// the shell was hidden or left first. `why` is one of the `not shown` lines'.
pub fn launch_dropped_line(id: &AppId, why: &str) -> String {
    format!("launch {id}: dropped: {why}")
}

/// A frame longer than this gets a `slow frame` line: a frame and a half at
/// 60 Hz, the same limit as `tools/frame-stats.sh`'s `--miss-ms` default for a
/// missed vsync (kept equal by hand).
pub const SLOW_FRAME: Duration = Duration::from_millis(25);

/// Page faults: minor ones found the page in memory, major ones read it back
/// from swap or disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults {
    pub minor: u64,
    pub major: u64,
}

impl Faults {
    /// The faults taken since `earlier`, a reading of the same counters.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            minor: self.minor.saturating_sub(earlier.minor),
            major: self.major.saturating_sub(earlier.major),
        }
    }
}

/// Where a frame's time went: the parts of the loop that are timed, and what
/// they did. The rest of the frame (events, springs, icons arriving and, in
/// `--screenshot` runs, saving one) is its `loop` part. A launch never is:
/// it ends the burst first, so the frame after it is untimed.
///
/// The wait for a free buffer can land in `submit` or in the swap, and may be
/// for work an earlier frame handed to the GPU, so a slow part is not always
/// its own frame's cost.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameSplit {
    /// Building the frame's batch (`draw::home`), glyphs rasterised included.
    pub batch: Duration,
    /// Copying new atlas pixels into the texture.
    pub upload: Duration,
    /// Handing the batch to GL. On the B9 (OLED55B9PUA, firmware 05.50.00,
    /// webOS 4.10.2, Mali-T820) the wait for a free buffer, so for the next
    /// vsync, lands here, not in the swap: slow frames spent p50 29.16 ms here
    /// and at most 1.80 ms in the swap (stage 1 run, 2026-10-22).
    pub submit: Duration,
    /// Inside `SDL_GL_SwapWindow`.
    pub swap: Duration,
    /// Atlas uploads in this frame (new glyphs and icons).
    pub uploads: usize,
    /// Their bytes, padding included; shown in KiB, rounded up.
    pub upload_bytes: usize,
    /// Glyphs rasterised while building this frame.
    pub glyphs: u32,
    /// Since the last frame; `None` where they are not counted.
    pub faults: Option<Faults>,
}

/// `slow frame <n>: <ms> ms = loop … + batch … + upload … + submit … + swap … ms;
/// <u> uploads, <k> KB, <g> glyphs; faults <minor> minor, <major> major` for
/// the `n`th frame of a burst, which took `took` from the swap before it.
pub fn slow_frame_line(n: u32, took: Duration, split: &FrameSplit) -> String {
    let timed = split.batch + split.upload + split.submit + split.swap;
    let faults = split.faults.map_or_else(
        || "-".to_owned(),
        |f| format!("{} minor, {} major", f.minor, f.major),
    );
    format!(
        "slow frame {n}: {} ms = loop {} + batch {} + upload {} + submit {} + swap {} ms; \
         {} uploads, {} KB, {} glyphs; faults {faults}",
        fine_ms(took),
        fine_ms(took.saturating_sub(timed)),
        fine_ms(split.batch),
        fine_ms(split.upload),
        fine_ms(split.submit),
        fine_ms(split.swap),
        split.uploads,
        split.upload_bytes.div_ceil(1024),
        split.glyphs,
    )
}

/// `slow frames: <k> more not logged`, for those past a burst's cap.
pub fn slow_more_line(more: u32) -> String {
    format!("slow frames: {more} more not logged")
}

/// A time in milliseconds to two places, as the frame lines show it.
pub fn fine_ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1000.0)
}

/// How many presses wait for a frame at once; more are only counted.
const KEYS_TIMED: usize = 8;

#[derive(Debug, Clone, Copy)]
struct Waiting {
    scancode: i32,
    at: Instant,
    in_launch: Duration,
}

/// Presses read but not yet on screen, each with the time SDL stamped it.
#[derive(Debug, Default)]
pub struct KeyTimes {
    waiting: [Option<Waiting>; KEYS_TIMED],
    untimed: u32,
    last_launch: Option<(Instant, Instant)>,
}

/// Launch time below this is not worth naming in a key's line.
const LAUNCH_NAMED: Duration = Duration::from_millis(1);

impl KeyTimes {
    /// A press went down at `at`; its time runs until a frame shows it. A
    /// press SDL stamped during the last launch, but read only after it, owes
    /// its wait to that launch.
    pub fn pressed(&mut self, scancode: i32, at: Instant) {
        let in_launch = match self.last_launch {
            Some((start, end)) if at < end => end.saturating_duration_since(at.max(start)),
            _ => Duration::ZERO,
        };
        match self.waiting.iter_mut().find(|w| w.is_none()) {
            Some(slot) => {
                *slot = Some(Waiting {
                    scancode,
                    at,
                    in_launch,
                })
            }
            None => self.untimed = self.untimed.saturating_add(1),
        }
    }

    /// The loop was blocked in a launch from `start` to `end`.
    pub fn launched(&mut self, start: Instant, end: Instant) {
        for w in self.waiting.iter_mut().flatten() {
            w.in_launch += end.saturating_duration_since(w.at.max(start));
        }
        self.last_launch = Some((start, end));
    }

    #[cfg(test)]
    pub fn is_waiting(&self) -> bool {
        self.untimed > 0 || self.waiting.iter().any(Option::is_some)
    }

    /// A frame finished swapping at `at`: one line per press it showed.
    pub fn shown(&mut self, at: Instant) -> Vec<String> {
        self.settle(at, "shown after", None)
    }

    /// No frame will show the waiting presses, for the reason `why`.
    pub fn not_shown(&mut self, at: Instant, why: &str) -> Vec<String> {
        self.settle(at, "not shown after", Some(why))
    }

    /// Lines for every waiting press, which then waits no more. With nothing
    /// waiting this allocates nothing, so it can run after every frame.
    fn settle(&mut self, at: Instant, verb: &str, why: Option<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        for w in self.waiting.iter_mut().filter_map(Option::take) {
            let mut line = format!(
                "key {}: {verb} {} ms",
                w.scancode,
                whole_ms(at.saturating_duration_since(w.at))
            );
            if w.in_launch >= LAUNCH_NAMED {
                line.push_str(&format!(" ({} ms in launch)", whole_ms(w.in_launch)));
            }
            if let Some(why) = why {
                line.push_str(&format!(": {why}"));
            }
            lines.push(line);
        }
        if self.untimed > 0 {
            lines.push(format!("keys: {} more presses not timed", self.untimed));
            self.untimed = 0;
        }
        lines
    }
}

/// The time from coming back to the front to the first frame shown.
#[derive(Debug, Default)]
pub struct Foreground {
    since: Option<Instant>,
}

impl Foreground {
    /// The shell came back to the front at `at`. A second sign of it before
    /// a frame keeps the first time.
    pub fn came_back(&mut self, at: Instant) {
        self.since.get_or_insert(at);
    }

    #[cfg(test)]
    pub fn is_waiting(&self) -> bool {
        self.since.is_some()
    }

    /// `foreground: first frame after N ms`, once per return.
    pub fn shown(&mut self, at: Instant) -> Option<String> {
        let since = self.since.take()?;
        Some(format!(
            "foreground: first frame after {} ms",
            whole_ms(at.saturating_duration_since(since))
        ))
    }

    /// `foreground: not shown after N ms: <why>`.
    pub fn not_shown(&mut self, at: Instant, why: &str) -> Option<String> {
        let since = self.since.take()?;
        Some(format!(
            "foreground: not shown after {} ms: {why}",
            whole_ms(at.saturating_duration_since(since))
        ))
    }
}

/// What the shell heard since the remote's last key, for the focus and
/// lifecycle lines: a focus lost long after the last key was not the owner's.
#[derive(Debug, Default)]
pub struct SinceKey {
    last_key: Option<Instant>,
    others: u32,
}

impl SinceKey {
    /// A remote key (one the shell acts on) went down or up at `at`.
    pub fn key(&mut self, at: Instant) {
        self.last_key = Some(at);
        self.others = 0;
    }

    /// Any other event.
    pub fn other(&mut self) {
        self.others = self.others.saturating_add(1);
    }

    /// `<what>: last key S.s s ago, M other events since`, or
    /// `<what>: no key yet, M other events since`.
    pub fn line(&self, what: &str, at: Instant) -> String {
        match self.last_key {
            Some(key) => format!(
                "{what}: last key {:.1} s ago, {} other events since",
                at.saturating_duration_since(key).as_secs_f64(),
                self.others
            ),
            None => format!("{what}: no key yet, {} other events since", self.others),
        }
    }
}

/// Whether the Magic Remote's pointer is on screen, from the 484 and 485 keys
/// LG's SDL sends as it appears and goes away. Hidden until the first 484:
/// LG hides it a few seconds after the remote stops moving.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pointer {
    Shown,
    #[default]
    Hidden,
}

impl Pointer {
    /// `shown` or `hidden`, as the lines say it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shown => "shown",
            Self::Hidden => "hidden",
        }
    }
}

/// The idle heartbeat comes this long after the last input, and then this
/// often, until `IDLE_LONG` without input; after that every `IDLE_LONG`
/// (docs/specs/look-and-feel.md, "Log lines"). So a 10-minute gap gets 20
/// lines and an evening without the remote a few.
pub const IDLE_BEAT: Duration = Duration::from_secs(30);
pub const IDLE_LONG: Duration = Duration::from_secs(600);

/// When the last input came and when the next heartbeat is due, on the loop's
/// clock. Input is any key, the pointer codes included, and any mouse event.
/// The heartbeat bounds a gap in the keys without a clock read: the owner says
/// "I was pressing at that line".
#[derive(Debug)]
pub struct Idle {
    last_input: Duration,
    next: Duration,
}

impl Idle {
    /// Starts the clock at `now`, as if input had just come.
    pub fn new(now: Duration) -> Self {
        Self {
            last_input: now,
            next: now + IDLE_BEAT,
        }
    }

    /// Input came at `now`.
    pub fn input(&mut self, now: Duration) {
        self.last_input = now;
        self.next = now + IDLE_BEAT;
    }

    /// When the last input came: the hero plan tells a burst input began by it.
    pub fn last_input(&self) -> Duration {
        self.last_input
    }

    /// When the next heartbeat is due, for the loop's wait.
    pub fn next_deadline(&self) -> Duration {
        self.next
    }

    /// The heartbeat line if one is due at `now`, which then schedules the
    /// next: `no input for N s; focused yes; pointer hidden; power Active`.
    /// A late wake-up (a long frame) gives one line, not the missed ones.
    pub fn beat(
        &mut self,
        now: Duration,
        focused: bool,
        pointer: Pointer,
        power: Option<&str>,
    ) -> Option<String> {
        if now < self.next {
            return None;
        }
        let quiet = now.saturating_sub(self.last_input);
        let step = if quiet < IDLE_LONG {
            IDLE_BEAT
        } else {
            IDLE_LONG
        };
        let beats = quiet.as_secs() / step.as_secs() + 1;
        self.next = self.last_input + step * u32::try_from(beats).unwrap_or(u32::MAX);
        Some(idle_line(quiet, focused, pointer, power))
    }
}

/// `no input for <N> s; focused <yes|no>; pointer <shown|hidden>; power
/// <state|unknown>`, with `N` in whole seconds.
pub fn idle_line(quiet: Duration, focused: bool, pointer: Pointer, power: Option<&str>) -> String {
    format!(
        "no input for {} s; focused {}; pointer {}; power {}",
        quiet.as_secs(),
        if focused { "yes" } else { "no" },
        pointer.name(),
        power.unwrap_or("unknown")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_heartbeat_comes_every_30_s_without_input() {
        let mut idle = Idle::new(secs(10));
        assert_eq!(idle.next_deadline(), secs(40));
        assert_eq!(idle.beat(secs(39), true, Pointer::Hidden, None), None);
        assert_eq!(
            idle.beat(secs(40), true, Pointer::Hidden, Some("Active"))
                .as_deref(),
            Some("no input for 30 s; focused yes; pointer hidden; power Active")
        );
        assert_eq!(idle.next_deadline(), secs(70));
        assert_eq!(
            idle.beat(secs(70) + ms(9), false, Pointer::Shown, None)
                .as_deref(),
            Some("no input for 60 s; focused no; pointer shown; power unknown")
        );
    }

    #[test]
    fn input_resets_the_heartbeat() {
        let mut idle = Idle::new(secs(0));
        idle.input(secs(25));
        assert_eq!(idle.last_input(), secs(25));
        assert_eq!(idle.beat(secs(30), true, Pointer::Hidden, None), None);
        assert_eq!(idle.next_deadline(), secs(55));
        assert!(idle.beat(secs(55), true, Pointer::Hidden, None).is_some());
    }

    #[test]
    fn a_late_wake_up_gives_one_line_and_the_next_whole_beat() {
        let mut idle = Idle::new(secs(0));
        // The loop slept through the 30 s and 60 s beats.
        let line = idle.beat(secs(95), true, Pointer::Hidden, None);
        assert_eq!(
            line.as_deref(),
            Some("no input for 95 s; focused yes; pointer hidden; power unknown")
        );
        assert_eq!(idle.next_deadline(), secs(120));
    }

    #[test]
    fn after_ten_minutes_the_heartbeat_slows_to_every_ten() {
        let mut idle = Idle::new(secs(0));
        assert!(idle.beat(secs(570), true, Pointer::Hidden, None).is_some());
        assert_eq!(idle.next_deadline(), secs(600));
        assert_eq!(
            idle.beat(secs(600), true, Pointer::Hidden, Some("Screen Saver"))
                .as_deref(),
            Some("no input for 600 s; focused yes; pointer hidden; power Screen Saver")
        );
        assert_eq!(idle.next_deadline(), secs(1200));
        assert!(idle.beat(secs(1200), true, Pointer::Hidden, None).is_some());
        assert_eq!(idle.next_deadline(), secs(1800));
        // Input brings the 30 s beat back.
        idle.input(secs(1500));
        assert_eq!(idle.next_deadline(), secs(1530));
    }

    #[test]
    fn the_pointer_names_its_state() {
        assert_eq!(Pointer::Shown.name(), "shown");
        assert_eq!(Pointer::default(), Pointer::Hidden);
    }

    #[test]
    fn the_start_line_lists_each_phase_from_mains_first_line() {
        let zero = Instant::now();
        let mut start = StartTimes::new(zero);
        start.mark(Phase::Sdl, zero + ms(152));
        start.mark(Phase::Gl, zero + ms(380));
        start.mark(Phase::Renderer, zero + ms(905));
        start.mark(Phase::Assets, zero + ms(1390));
        start.mark(Phase::Apps, zero + ms(1446));
        assert_eq!(
            start.line(zero + ms(1504)),
            "start: sdl 152, gl 380, renderer 905, assets 1390, apps 1446, first frame 1504 ms"
        );
    }

    #[test]
    fn a_start_without_a_frame_says_why() {
        let zero = Instant::now();
        let mut start = StartTimes::new(zero);
        start.mark(Phase::Sdl, zero + ms(10));
        start.mark(Phase::Gl, zero + ms(20));
        start.mark(Phase::Renderer, zero + ms(30));
        start.mark(Phase::Assets, zero + ms(40));
        start.mark(Phase::Apps, zero + ms(50));
        assert_eq!(
            start.line_without_frame("the window lost focus"),
            "start: sdl 10, gl 20, renderer 30, assets 40, apps 50 ms; no first frame: the window lost focus"
        );
    }

    #[test]
    fn a_phase_never_reached_shows_a_dash() {
        let zero = Instant::now();
        let mut start = StartTimes::new(zero);
        start.mark(Phase::Sdl, zero + ms(10));
        assert_eq!(
            start.line_without_frame("the shell is leaving"),
            "start: sdl 10, gl -, renderer -, assets -, apps - ms; no first frame: the shell is leaving"
        );
    }

    #[test]
    fn a_launch_line_says_how_long_luna_took_to_answer() {
        let id = AppId::new("com.webos.app.hdmi1");
        assert_eq!(
            launch_line(&id, ms(320), &Ok(())),
            "launch com.webos.app.hdmi1: answered in 320 ms"
        );
    }

    #[test]
    fn a_failed_launch_line_says_after_how_long_and_why() {
        let id = AppId::new("netflix");
        let err = Err(LaunchError("no reply in 5000 ms".to_owned()));
        assert_eq!(
            launch_line(&id, ms(5003), &err),
            "launch netflix: failed after 5003 ms: no reply in 5000 ms"
        );
    }

    #[test]
    fn a_dropped_launch_line_says_why() {
        let id = AppId::new("netflix");
        assert_eq!(
            launch_dropped_line(&id, "the window was hidden"),
            "launch netflix: dropped: the window was hidden"
        );
    }

    #[test]
    fn a_sub_millisecond_launch_reads_0_ms() {
        let id = AppId::new("amazon");
        assert_eq!(
            launch_line(&id, Duration::from_micros(80), &Ok(())),
            "launch amazon: answered in 0 ms"
        );
    }

    fn micros(n: u64) -> Duration {
        Duration::from_micros(n)
    }

    #[test]
    fn a_slow_frame_line_splits_its_time_and_says_what_it_did() {
        let split = FrameSplit {
            batch: micros(400),
            upload: micros(2200),
            submit: micros(500),
            swap: micros(25_000),
            uploads: 3,
            upload_bytes: 12 * 1024,
            glyphs: 9,
            faults: Some(Faults { minor: 4, major: 1 }),
        };
        assert_eq!(
            slow_frame_line(2, micros(28_410), &split),
            "slow frame 2: 28.41 ms = loop 0.31 + batch 0.40 + upload 2.20 + submit 0.50 + swap 25.00 ms; \
             3 uploads, 12 KB, 9 glyphs; faults 4 minor, 1 major"
        );
    }

    #[test]
    fn a_slow_frame_without_fault_counts_or_uploads_says_so() {
        let split = FrameSplit {
            swap: micros(30_000),
            upload_bytes: 1,
            ..FrameSplit::default()
        };
        assert_eq!(
            slow_frame_line(5, micros(30_000), &split),
            "slow frame 5: 30.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 30.00 ms; \
             0 uploads, 1 KB, 0 glyphs; faults -"
        );
    }

    #[test]
    fn parts_longer_than_the_frame_leave_no_negative_loop() {
        let split = FrameSplit {
            swap: micros(30_000),
            ..FrameSplit::default()
        };
        assert!(slow_frame_line(2, micros(29_000), &split).contains("= loop 0.00 + "));
    }

    #[test]
    fn slow_frames_past_the_cap_are_counted() {
        assert_eq!(slow_more_line(3), "slow frames: 3 more not logged");
    }

    #[test]
    fn faults_since_an_earlier_reading_never_go_below_zero() {
        let earlier = Faults {
            minor: 10,
            major: 2,
        };
        let later = Faults {
            minor: 14,
            major: 2,
        };
        assert_eq!(later.since(earlier), Faults { minor: 4, major: 0 });
        assert_eq!(earlier.since(later), Faults { minor: 0, major: 0 });
    }

    #[test]
    fn a_press_is_shown_by_the_next_swap() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        keys.pressed(79, t);
        assert!(keys.is_waiting());
        assert_eq!(keys.shown(t + ms(17)), ["key 79: shown after 17 ms"]);
        assert!(!keys.is_waiting());
        assert!(
            keys.shown(t + ms(34)).is_empty(),
            "each press is shown once"
        );
    }

    #[test]
    fn two_presses_before_one_frame_both_get_a_line() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        keys.pressed(79, t);
        keys.pressed(81, t + ms(5));
        assert_eq!(
            keys.shown(t + ms(20)),
            ["key 79: shown after 20 ms", "key 81: shown after 15 ms"]
        );
    }

    #[test]
    fn a_launch_while_waiting_is_named() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        keys.pressed(40, t);
        keys.launched(t + ms(15), t + ms(335));
        assert_eq!(
            keys.not_shown(t + ms(335), "the window lost focus"),
            ["key 40: not shown after 335 ms (320 ms in launch): the window lost focus"]
        );
    }

    #[test]
    fn a_press_read_after_a_launch_counts_the_launch_it_waited_through() {
        // Late enough that `t - 5 ms` exists even just after boot.
        let t = Instant::now() + ms(10);
        let mut keys = KeyTimes::default();
        keys.launched(t, t + ms(320));
        // Stamped by SDL 100 ms into the launch, read once it was answered.
        keys.pressed(79, t + ms(100));
        // One before the launch began counts only the launch.
        keys.pressed(80, t - ms(5));
        assert_eq!(
            keys.shown(t + ms(340)),
            [
                "key 79: shown after 240 ms (220 ms in launch)",
                "key 80: shown after 345 ms (320 ms in launch)",
            ]
        );
        keys.pressed(81, t + ms(400));
        assert_eq!(
            keys.shown(t + ms(417)),
            ["key 81: shown after 17 ms"],
            "a press after the launch owes it nothing"
        );
    }

    #[test]
    fn a_press_hidden_before_any_frame_says_why() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        keys.pressed(82, t);
        assert_eq!(
            keys.not_shown(t + ms(40), "the app went to the background"),
            ["key 82: not shown after 40 ms: the app went to the background"]
        );
        assert!(!keys.is_waiting());
    }

    #[test]
    fn presses_beyond_the_buffer_are_counted() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        for i in 0..11 {
            keys.pressed(79, t + ms(i));
        }
        let lines = keys.shown(t + ms(20));
        assert_eq!(lines.len(), KEYS_TIMED + 1);
        assert_eq!(lines[0], "key 79: shown after 20 ms");
        assert_eq!(lines[KEYS_TIMED], "keys: 3 more presses not timed");
        keys.pressed(79, t + ms(30));
        assert_eq!(keys.shown(t + ms(47)), ["key 79: shown after 17 ms"]);
    }

    #[test]
    fn nothing_waiting_yields_no_lines() {
        let t = Instant::now();
        let mut keys = KeyTimes::default();
        assert!(!keys.is_waiting());
        assert!(keys.shown(t).is_empty());
        assert!(keys.not_shown(t, "nothing to draw").is_empty());
    }

    #[test]
    fn foreground_counts_from_the_event_that_brought_the_shell_back() {
        let t = Instant::now();
        let mut fg = Foreground::default();
        assert_eq!(fg.shown(t), None, "the start-up is not a foreground");
        fg.came_back(t);
        assert_eq!(
            fg.shown(t + ms(48)).as_deref(),
            Some("foreground: first frame after 48 ms")
        );
        assert_eq!(fg.shown(t + ms(64)), None, "once per return");
    }

    #[test]
    fn hidden_again_before_a_frame_says_why() {
        let t = Instant::now();
        let mut fg = Foreground::default();
        fg.came_back(t);
        assert_eq!(
            fg.not_shown(t + ms(12), "the window lost focus").as_deref(),
            Some("foreground: not shown after 12 ms: the window lost focus")
        );
        assert_eq!(fg.shown(t + ms(30)), None);
        assert_eq!(fg.not_shown(t + ms(30), "the window was hidden"), None);
    }

    #[test]
    fn a_second_foreground_event_keeps_the_first_time() {
        let t = Instant::now();
        let mut fg = Foreground::default();
        fg.came_back(t);
        fg.came_back(t + ms(10));
        assert_eq!(
            fg.shown(t + ms(30)).as_deref(),
            Some("foreground: first frame after 30 ms")
        );
    }

    #[test]
    fn a_window_line_says_how_long_since_the_last_key() {
        let t = Instant::now();
        let mut since = SinceKey::default();
        since.key(t);
        for _ in 0..37 {
            since.other();
        }
        assert_eq!(
            since.line("window focus lost", t + ms(132_400)),
            "window focus lost: last key 132.4 s ago, 37 other events since"
        );
    }

    #[test]
    fn before_any_key_it_says_so() {
        let t = Instant::now();
        let mut since = SinceKey::default();
        since.other();
        assert_eq!(
            since.line("app background", t),
            "app background: no key yet, 1 other events since"
        );
    }

    #[test]
    fn other_events_count_since_the_last_key() {
        let t = Instant::now();
        let mut since = SinceKey::default();
        since.other();
        since.other();
        since.key(t);
        since.other();
        assert_eq!(
            since.line("window hidden", t + ms(60)),
            "window hidden: last key 0.1 s ago, 1 other events since"
        );
    }
}
