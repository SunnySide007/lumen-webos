//! The home loop's clock (`Clock`) and the times of each pass (`Pass`): real
//! time, paced by vsync while frames are drawn, or with `--fixed-dt` a fixed
//! step per frame.

use std::time::{Duration, Instant};

use lumen_core::pace::Pacer;

/// The loop's time. Real by default, paced by `lumen_core::pace` while frames
/// are drawn with vsync: each frame of a burst shows one period of motion
/// more than the one before, and a frame shows one period after its pass.
/// With `--fixed-dt` a virtual clock that advances exactly one step per frame
/// drawn and, while the screen is settled, jumps straight to the next
/// deadline instead of waiting for it, so the same keys always give the same
/// frames; it is never paced.
#[derive(Debug)]
pub(super) struct Clock {
    pub(super) start: Instant,
    step: Option<Duration>,
    virtual_now: Duration,
    /// With vsync and the real clock.
    pacer: Option<Pacer>,
}

/// One pass of the loop: its time on the loop's clock, and the real time it
/// was read at (the same unless paced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Pass {
    pub(super) now: Duration,
    real: Duration,
}

impl Clock {
    /// `period` paces a real clock (vsync on); a fixed step is never paced.
    pub(super) fn new(step: Option<Duration>, period: Option<Duration>) -> Self {
        Self {
            start: Instant::now(),
            step,
            virtual_now: Duration::ZERO,
            pacer: period.filter(|_| step.is_none()).map(Pacer::new),
        }
    }

    /// Paces a real clock by `period` from the next burst on; a fixed step
    /// is never paced.
    pub(super) fn pace(&mut self, period: Duration) {
        if self.step.is_none() {
            self.pacer = Some(Pacer::new(period));
        }
    }

    /// A pass read now. Its `now` is what presses, repeats and deadlines go
    /// by; its `real` is what a paced frame is drawn for.
    pub(super) fn pass(&self) -> Pass {
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
    pub(super) fn now(&self) -> Duration {
        self.pass().now
    }

    /// The motion time the frame drawn in the pass at `now` shows: paced, one
    /// period on, the earliest vsync it can reach, so a press made in the
    /// pass already moves something in that frame; otherwise `now`.
    pub(super) fn shows(&self, now: Duration) -> Duration {
        self.pacer.map_or(now, |p| p.shows(now))
    }

    /// When key repeats fall due in `pass`: its real time when paced, never
    /// after its `now`, so motion running ahead of real time fires no repeat
    /// before the key-up that would stop it.
    pub(super) fn input_time(&self, pass: Pass) -> Duration {
        match self.pacer {
            Some(_) => pass.real.min(pass.now),
            None => pass.now,
        }
    }

    /// Real time since the start, whatever the clock: `--exit-after` and the
    /// idle waits use it.
    pub(super) fn real(&self) -> Duration {
        self.start.elapsed()
    }

    pub(super) fn is_fixed(&self) -> bool {
        self.step.is_some()
    }

    /// A frame was drawn in `pass`.
    pub(super) fn frame_drawn(&mut self, pass: Pass) {
        if let Some(step) = self.step {
            self.virtual_now += step;
        }
        if let Some(pacer) = &mut self.pacer {
            pacer.drawn(pass.now, pass.real);
        }
    }

    /// The screen settled, or a launch cut the burst off: the next frame
    /// starts a burst.
    pub(super) fn idle(&mut self) {
        if let Some(pacer) = &mut self.pacer {
            pacer.idle();
        }
    }

    /// The vsyncs skipped since the last call, for the burst line; `None`
    /// when not paced.
    pub(super) fn take_skipped(&mut self) -> Option<u32> {
        self.pacer.as_mut().map(Pacer::take_skipped)
    }

    /// With a fixed step, moves the clock on to `t` (never back).
    pub(super) fn jump_to(&mut self, t: Duration) {
        if self.is_fixed() {
            self.virtual_now = self.virtual_now.max(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
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
}
