//! Frame-time statistics, reported once per window (one second by default) so
//! the on-screen overlay and the log quote the same numbers.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    pub fps: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
    pub frames: u32,
}

pub struct FrameStats {
    window: Duration,
    elapsed: Duration,
    frames: u32,
    worst: Duration,
}

impl FrameStats {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            elapsed: Duration::ZERO,
            frames: 0,
            worst: Duration::ZERO,
        }
    }

    /// Records one frame's duration. Returns a report, and starts a new window,
    /// once the frames recorded add up to at least the window.
    pub fn record(&mut self, frame: Duration) -> Option<Report> {
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
    pub fn reset(&mut self) {
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
        let mut stats = FrameStats::new(Duration::from_secs(1));
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
        let mut stats = FrameStats::new(Duration::from_millis(100));
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
        let mut stats = FrameStats::new(Duration::from_millis(100));
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
}
