//! The home loop's clock (`Clock`) and the times of each pass (`Pass`): real
//! time, paced by vsync while frames are drawn, or with `--fixed-dt` a fixed
//! step per frame. Real time is the wall clock's (`WallClock`): the system's
//! on the TV, the fakes' in the home loop's tests, which reach this module
//! through the loop.

use std::time::{Duration, Instant};

use lumen_core::pace::Pacer;

use super::WallClock;

/// The loop's time. Real by default, paced by `lumen_core::pace` while frames
/// are drawn with vsync: each frame of a burst shows one period of motion
/// more than the one before, and a frame shows one period after its pass.
/// With `--fixed-dt` a virtual clock that advances exactly one step per frame
/// drawn and, while the screen is settled, jumps straight to the next
/// deadline instead of waiting for it, so the same keys always give the same
/// frames; it is never paced.
#[derive(Debug)]
pub(super) struct Clock<'w, W> {
    wall: &'w W,
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

impl<'w, W: WallClock> Clock<'w, W> {
    /// Real time counts from `start` on `wall`. `period` paces a real clock
    /// (vsync on); a fixed step is never paced.
    pub(super) fn new(
        wall: &'w W,
        start: Instant,
        step: Option<Duration>,
        period: Option<Duration>,
    ) -> Self {
        Self {
            wall,
            start,
            step,
            virtual_now: Duration::ZERO,
            pacer: period.filter(|_| step.is_none()).map(Pacer::new),
        }
    }

    /// The wall clock real time is read from.
    pub(super) fn wall(&self) -> &'w W {
        self.wall
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
        let real = self.real();
        Pass {
            now: self.pacer.map_or(real, |p| p.time(real)),
            real,
        }
    }

    /// The time a pass read now would have.
    pub(super) fn now(&self) -> Duration {
        self.pass().now
    }

    /// The time a pass would have had at `at`, before the first frame and
    /// unpaced: real time since the start, or a fixed clock's own.
    pub(super) fn at(&self, at: Instant) -> Duration {
        if self.step.is_some() {
            return self.virtual_now;
        }
        at.saturating_duration_since(self.start)
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
        self.wall.now().saturating_duration_since(self.start)
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
