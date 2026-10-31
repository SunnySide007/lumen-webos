//! Frame-time statistics for the log: each burst's `drew` line and its
//! `slow frame` lines, and an `fps` line once a burst's frames add up to a
//! second (docs/specs/shell-mvp.md, "Repainting"). The loop tells it when a
//! frame showed and when a burst is over, and logs the lines it gives back.
//! The formats (docs/specs/look-and-feel.md on main, "Log lines") are pinned
//! by the tests below, since `tools/frame-stats.sh` (on main) reads them.

use std::time::{Duration, Instant};

use crate::timing::{self, Faults, FrameSplit, SLOW_FRAME};

/// How many of a burst's first timed frames its line names.
const FIRST_TIMED: usize = 3;
/// How many slow frames a burst logs a line for; the rest are counted.
const SLOW_LOGGED: u32 = 4;
/// The frame time an `fps` line covers, at least.
const FPS_WINDOW: Duration = Duration::from_secs(1);

/// Why a burst is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstEnd {
    /// The loop is about to wait: the screen settled, or the shell is hidden.
    Idle,
    /// A launch is about to block the loop.
    Launch,
    /// The shell is leaving.
    Exit,
}

/// What a frame or a burst's end gives the loop: lines to log, in order, and
/// whether to read GL's error flag.
#[derive(Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Said {
    pub lines: Vec<String>,
    pub read_gl_error: bool,
}

/// The burst going on, the `fps` window and the fault counters at the last
/// swap.
#[derive(Debug)]
pub struct FrameStats {
    burst: Burst,
    fps: FpsWindow,
    /// Read after the last swap, for the next frame's faults.
    faults_seen: Option<Faults>,
}

impl FrameStats {
    /// No burst yet, and no fault reading to count the first frame's from:
    /// that frame is never timed, so it needs none.
    pub fn new() -> Self {
        Self {
            burst: Burst::default(),
            fps: FpsWindow::new(FPS_WINDOW),
            faults_seen: None,
        }
    }

    /// A frame finished swapping at `at`, having spent and done what `split`
    /// says and drawn `quads`. `faults` is the fault counters read after the
    /// swap, `None` where they are not counted; the split's faults are set
    /// here, to the change since the last frame's reading. Gives the frame's
    /// `slow frame` line, if it took over `SLOW_FRAME`, then an `fps` line
    /// once the window is full, and GL's error flag is due a read with it.
    /// The first frame after an idle wait or a launch has nothing before it
    /// to be timed from, so it gives neither.
    pub fn frame(
        &mut self,
        at: Instant,
        mut split: FrameSplit,
        faults: Option<Faults>,
        quads: usize,
    ) -> Said {
        split.faults = faults
            .zip(self.faults_seen)
            .map(|(later, earlier)| later.since(earlier));
        self.faults_seen = faults;
        let mut said = Said::default();
        let Some(took) = self.burst.frame(at) else {
            return said;
        };
        if let Some(line) = self.burst.slow_line(took, &split) {
            said.lines.push(line);
        }
        if let Some(report) = self.fps.record(took) {
            said.lines.push(fps_line(&report, quads));
            said.read_gl_error = true;
        }
        said
    }

    /// The burst is over, for the reason `end`: its lines (slow frames not
    /// logged, then the burst itself), none if no frame was drawn, and a
    /// fresh start. `quads` is the last frame's. `skipped` is the pacer's
    /// count of vsyncs skipped in the burst, `None` without a pacer, which
    /// leaves it out of the line. The next `fps` window starts with the next
    /// burst, so it never spans an idle wait, a hide or a launch. GL's error
    /// flag holds until read and a burst shorter than a window has no `fps`
    /// line, so a burst that ends idle having drawn has it read too.
    pub fn burst_over(&mut self, end: BurstEnd, quads: usize, skipped: Option<u32>) -> Said {
        let lines = self.burst.finish(quads, skipped);
        self.fps.reset();
        Said {
            read_gl_error: end == BurstEnd::Idle && !lines.is_empty(),
            lines,
        }
    }
}

/// `fps 60.0 avg 16.67 ms max 17.20 ms (60 frames, 45 quads)`.
fn fps_line(report: &Report, quads: usize) -> String {
    format!(
        "fps {:.1} avg {:.2} ms max {:.2} ms ({} frames, {quads} quads)",
        report.fps, report.avg_ms, report.max_ms, report.frames
    )
}

/// The frames drawn since the burst began, for its `drew` line and its
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
    /// a burst's first frame (after an idle wait or a launch), which has
    /// nothing to measure from.
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

/// One `fps` window's numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Report {
    fps: f64,
    avg_ms: f64,
    max_ms: f64,
    frames: u32,
}

/// The timed frames of a burst since its last `fps` line.
#[derive(Debug)]
struct FpsWindow {
    window: Duration,
    elapsed: Duration,
    frames: u32,
    worst: Duration,
}

impl FpsWindow {
    fn new(window: Duration) -> Self {
        Self {
            window,
            elapsed: Duration::ZERO,
            frames: 0,
            worst: Duration::ZERO,
        }
    }

    /// Records one frame's duration. Returns a report, and starts a new window,
    /// once the frames recorded add up to at least the window.
    fn record(&mut self, frame: Duration) -> Option<Report> {
        self.elapsed += frame;
        self.frames += 1;
        self.worst = self.worst.max(frame);
        if self.elapsed < self.window {
            return None;
        }
        let secs = self.elapsed.as_secs_f64();
        let report = Report {
            fps: f64::from(self.frames) / secs,
            avg_ms: secs * 1000.0 / f64::from(self.frames),
            max_ms: self.worst.as_secs_f64() * 1000.0,
            frames: self.frames,
        };
        self.reset();
        Some(report)
    }

    /// Starts a new window, dropping what the current one holds without a
    /// report: drawing stopped, so the next frame does not follow these.
    fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.frames = 0;
        self.worst = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_60HZ: Duration = Duration::from_micros(16_667);

    #[test]
    fn reports_only_once_the_window_is_full() {
        let mut stats = FpsWindow::new(Duration::from_secs(1));
        for _ in 0..59 {
            assert_eq!(stats.record(FRAME_60HZ), None);
        }
        let report = stats
            .record(FRAME_60HZ)
            .expect("60 frames of 16.667 ms fill one second");
        assert_eq!(report.frames, 60);
        assert!((report.fps - 60.0).abs() < 0.01, "fps {}", report.fps);
        assert!(
            (report.avg_ms - 16.667).abs() < 0.001,
            "avg {}",
            report.avg_ms
        );
    }

    #[test]
    fn max_is_the_worst_frame_and_resets_each_window() {
        let mut stats = FpsWindow::new(Duration::from_millis(100));
        stats.record(Duration::from_millis(10));
        stats.record(Duration::from_millis(40));
        let first = stats
            .record(Duration::from_millis(50))
            .expect("100 ms recorded");
        assert!((first.max_ms - 50.0).abs() < 1e-9);

        stats.record(Duration::from_millis(20));
        let second = stats
            .record(Duration::from_millis(80))
            .expect("100 ms recorded");
        assert!((second.max_ms - 80.0).abs() < 1e-9);
        assert_eq!(second.frames, 2);
    }

    #[test]
    fn a_reset_starts_a_fresh_window() {
        let mut stats = FpsWindow::new(Duration::from_millis(100));
        stats.record(Duration::from_millis(60));
        stats.record(Duration::from_millis(30));
        stats.reset();
        assert_eq!(
            stats.record(Duration::from_millis(40)),
            None,
            "40 ms of 100"
        );
        let report = stats
            .record(Duration::from_millis(60))
            .expect("100 ms recorded");
        assert_eq!(report.frames, 2);
        assert!((report.max_ms - 60.0).abs() < 1e-9);
        assert!((report.avg_ms - 50.0).abs() < 1e-9);
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

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_frame_gives_its_slow_line_then_its_fps_line_and_a_gl_read() {
        let mut stats = FrameStats::new();
        let t0 = Instant::now();
        assert_eq!(
            stats.frame(t0, FrameSplit::default(), None, 10),
            Said::default(),
            "the first frame is not timed"
        );
        // 39 frames of 25 ms, which is not over the limit, then one of 35 ms:
        // 1.01 s in all, the first full window.
        let mut at = t0;
        for _ in 0..39 {
            at += ms(25);
            assert_eq!(stats.frame(at, swap_only(25), None, 10), Said::default());
        }
        let said = stats.frame(at + ms(35), swap_only(35), None, 12);
        assert_eq!(
            said.lines,
            [
                "slow frame 41: 35.00 ms = loop 0.00 + batch 0.00 + upload 0.00 + submit 0.00 + swap 35.00 ms; \
                 0 uploads, 0 KB, 0 glyphs; faults -",
                "fps 39.6 avg 25.25 ms max 35.00 ms (40 frames, 12 quads)",
            ]
        );
        assert!(said.read_gl_error, "with each fps line");
    }

    #[test]
    fn a_slow_frame_counts_the_faults_since_the_frame_before() {
        let mut stats = FrameStats::new();
        let t0 = Instant::now();
        let reading = |minor, major| Some(Faults { minor, major });
        let _ = stats.frame(t0, FrameSplit::default(), reading(100, 3), 10);
        let said = stats.frame(t0 + ms(30), swap_only(30), reading(140, 4), 10);
        assert!(
            said.lines[0].ends_with("; faults 40 minor, 1 major"),
            "{said:?}"
        );
        assert!(!said.read_gl_error, "a slow line alone reads no GL");
        let said = stats.frame(t0 + ms(60), swap_only(30), None, 10);
        assert!(said.lines[0].ends_with("; faults -"), "not counted");
        let said = stats.frame(t0 + ms(90), swap_only(30), reading(150, 4), 10);
        assert!(
            said.lines[0].ends_with("; faults -"),
            "no reading before it"
        );
    }

    #[test]
    fn only_a_burst_that_ends_idle_reads_gl_and_only_if_it_drew() {
        for (end, read) in [
            (BurstEnd::Idle, true),
            (BurstEnd::Launch, false),
            (BurstEnd::Exit, false),
        ] {
            let mut stats = FrameStats::new();
            let _ = stats.frame(Instant::now(), FrameSplit::default(), None, 10);
            let said = stats.burst_over(end, 10, None);
            assert_eq!(
                said.lines,
                ["drew 1 frames in 0 ms (avg 0.00 ms, worst 0.00 ms, 10 quads); idle"],
                "{end:?}"
            );
            assert_eq!(said.read_gl_error, read, "{end:?}");
            assert_eq!(
                stats.burst_over(end, 10, None),
                Said::default(),
                "{end:?}: nothing drawn since"
            );
        }
    }

    #[test]
    fn the_fps_window_starts_again_when_a_burst_ends() {
        for end in [BurstEnd::Idle, BurstEnd::Launch] {
            let mut stats = FrameStats::new();
            let t0 = Instant::now();
            let _ = stats.frame(t0, FrameSplit::default(), None, 10);
            let _ = stats.frame(t0 + ms(900), swap_only(900), None, 10);
            let _ = stats.burst_over(end, 10, None);
            // The next burst's first frame is not timed, and its second puts
            // 200 ms in a fresh window, not on top of the 900 before.
            let _ = stats.frame(t0 + ms(5_000), FrameSplit::default(), None, 10);
            let said = stats.frame(t0 + ms(5_200), swap_only(200), None, 10);
            assert!(
                !said.read_gl_error && !said.lines.iter().any(|l| l.starts_with("fps ")),
                "{end:?}: {said:?}"
            );
        }
    }
}
