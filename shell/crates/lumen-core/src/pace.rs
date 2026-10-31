//! Motion time for frames shown at a steady refresh (docs/specs/shell-mvp.md,
//! "Motion", "Paced by vsync").
//!
//! With vsync the panel shows one frame each period, but the loop does not run
//! at that pace. After an idle wait the first frames go into the driver's
//! queue at once and a later one waits for a free buffer: on the B9, frames 2
//! and 3 of a burst take about 9 and 30 ms, two vsyncs together, so they are
//! probably shown 16.7 ms apart (the slow-motion video is not read yet).
//! Stepping the springs by that real time moves them 9 ms, then 30 ms: an
//! uneven start to every tap. A `Pacer` gives each frame of a burst exactly one
//! period of motion and lets real time in only in whole periods.
//!
//! Each pass of the loop has two times. The pass's own (`time`) is where the
//! last frame left the motion: presses, repeats and deadlines use it. The
//! frame drawn in the pass shows one period later (`shows`), the earliest
//! vsync it can reach. So a press moves something in the very next frame,
//! instead of that frame repeating the one before it.

use std::time::Duration;

/// Frames 1 to 3 of a burst fill the driver's queue: on the B9 frames 2 and
/// 3 take about 9 and 30 ms (frame 1 is not timed), and frame 4 is the first
/// at the panel's pace. Lag is sampled from frame 4 and used from frame 5.
const QUEUE_FILLED: u32 = 3;
/// Motion time may run this many periods ahead of real time: the driver's
/// queue lets frames in early, but no faster than the panel on average.
const MAX_LEAD: u32 = 2;

/// The rate taken when SDL gives none the shell believes: what the B9 shows
/// (hello-gles held 60.0 fps).
pub const DEFAULT_HZ: u32 = 60;
/// One period at `DEFAULT_HZ`.
pub const DEFAULT_PERIOD: Duration = Duration::from_nanos(16_666_666);

/// The period of a panel refreshing `hz` times a second, if `hz` is a rate
/// the shell believes: 24 to 500 Hz. Anything else is a misread.
pub fn period_of(hz: i64) -> Option<Duration> {
    let hz = u32::try_from(hz)
        .ok()
        .filter(|hz| (24..=500).contains(hz))?;
    Some(Duration::from_secs(1) / hz)
}

/// Turns the loop's real time into motion time while frames are drawn with
/// vsync, one period of motion a frame; see the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pacer {
    period: Duration,
    /// The time of the last pass that drew a frame; `None` before the first.
    /// It never goes back.
    last: Option<Duration>,
    /// Frames drawn since the screen last settled.
    frames: u32,
    /// How far real time ran past the pass's time, in ns, at its least, over
    /// the burst's frames since the queue filled: where a frame drawn on time
    /// sits against the vsync. Negative while motion runs ahead.
    lag: i64,
    /// Periods skipped since `take_skipped`: vsyncs at which the panel showed
    /// the frame before again.
    skipped: u32,
}

impl Pacer {
    /// Frames `period` apart on the panel. A zero period counts as 1 ns.
    pub fn new(period: Duration) -> Self {
        Self {
            period: period.max(Duration::from_nanos(1)),
            last: None,
            frames: 0,
            lag: i64::MAX,
            skipped: 0,
        }
    }

    /// The panel's refresh period this pacer steps by.
    pub const fn period(&self) -> Duration {
        self.period
    }

    /// The pass's time when the real clock reads `real`.
    ///
    /// * The first pass of a burst is at `real`, or at the time the last
    ///   frame showed if that is later, so time never runs back and the
    ///   first frame always moves. That one pass may lead real time by up to
    ///   three periods, when a press comes just after the screen settled.
    /// * Every later pass is one period on from the one before: where the
    ///   last frame left the motion.
    /// * Real time over half a period past where passes usually sit (the
    ///   burst's least lag, sampled from frame 4 and used from frame 5, held
    ///   between one period ahead and none; before that, none) means the
    ///   panel missed a vsync: the periods missed are skipped, whole.
    /// * Over two periods ahead, frames come faster than the panel (a faster
    ///   panel than the period says): motion follows real time instead of
    ///   running fast. Only here, and where time would run back, is a step
    ///   not a whole number of periods.
    pub fn time(&self, real: Duration) -> Duration {
        let Some(last) = self.last else {
            return real;
        };
        let p = self.period;
        let mut t = last.saturating_add(p);
        if self.frames == 0 {
            return real.max(t);
        }
        let usual = if self.frames > QUEUE_FILLED {
            self.lag.clamp(-nanos(p), 0)
        } else {
            0
        };
        let late = nanos(real).saturating_sub(nanos(t)).saturating_sub(usual);
        if late > nanos(p) / 2 {
            let periods = late.saturating_add(nanos(p) / 2) / nanos(p);
            t = t.saturating_add(p.saturating_mul(u32::try_from(periods).unwrap_or(u32::MAX)));
        }
        t.min(real.saturating_add(p.saturating_mul(MAX_LEAD)))
            .max(last)
    }

    /// The motion time a frame drawn in the pass at `pass` shows: one period
    /// on, the earliest vsync it can reach.
    pub fn shows(&self, pass: Duration) -> Duration {
        pass.saturating_add(self.period)
    }

    /// A frame was drawn in the pass at `pass`, which `time` gave for `real`.
    pub fn drawn(&mut self, pass: Duration, real: Duration) {
        if let Some(last) = self.last
            && self.frames > 0
        {
            let periods = pass.saturating_sub(last).as_nanos() / self.period.as_nanos();
            let missed = u32::try_from(periods.saturating_sub(1)).unwrap_or(u32::MAX);
            self.skipped = self.skipped.saturating_add(missed);
        }
        self.last = Some(self.last.map_or(pass, |last| last.max(pass)));
        self.frames = self.frames.saturating_add(1);
        if self.frames > QUEUE_FILLED {
            self.lag = self.lag.min(nanos(real).saturating_sub(nanos(pass)));
        }
    }

    /// Periods skipped since the last call: each is a vsync at which the panel
    /// showed the frame before again. A burst's line takes them as it ends.
    pub fn take_skipped(&mut self) -> u32 {
        std::mem::take(&mut self.skipped)
    }

    /// The screen settled, or the burst was cut off (a launch blocking the
    /// loop): the next frame starts a burst, and the wait until then skips
    /// nothing.
    pub fn idle(&mut self) {
        self.frames = 0;
        self.lag = i64::MAX;
    }
}

/// `d` in whole nanoseconds; past 292 years it stops counting.
fn nanos(d: Duration) -> i64 {
    i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{Launcher, MockLauncher};
    use crate::focus::Dir;
    use crate::home::{BackAtTop, Home};
    use crate::input::{Action, Press};

    /// 60 Hz.
    const P: Duration = Duration::from_nanos(16_666_667);

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    /// Real loop times for a burst starting at 1 s whose frames take `took`
    /// ms each, then 16.667 ms, until `frames` are drawn. Frame 1 follows an
    /// idle wait; the loop comes round 3 ms after it.
    fn loop_times(took: &[f64], frames: usize) -> Vec<Duration> {
        let mut at = vec![ms(1000.0), ms(1003.0)];
        let mut i = 0;
        while at.len() < frames {
            let step = took.get(i).copied().unwrap_or(16.667);
            at.push(*at.last().unwrap() + ms(step));
            i += 1;
        }
        at.truncate(frames);
        at
    }

    /// The pass times a pacer gives for `real`, each pass drawing a frame.
    fn paced(pacer: &mut Pacer, real: &[Duration]) -> Vec<Duration> {
        real.iter()
            .map(|&r| {
                let t = pacer.time(r);
                pacer.drawn(t, r);
                t
            })
            .collect()
    }

    /// One tap of Right, drawn at the real times given, through a `Pacer` or
    /// (with `None`) stepped by real time as before. Returns each frame's lift
    /// of the tile focus leaves and the tile it reaches.
    fn tap(real: &[Duration], pacer: Option<Pacer>) -> Vec<(f32, f32)> {
        let apps = MockLauncher::default().list().unwrap();
        let mut home = Home::new(apps, BackAtTop::Exit);
        home.frame_drawn();
        let mut pacer = pacer;
        let mut lifts = Vec::new();
        for (k, &r) in real.iter().enumerate() {
            let now = pacer.map_or(r, |p| p.time(r));
            if k == 0 {
                let press = Press {
                    action: Action::Move(Dir::Right),
                    repeat: false,
                };
                assert_eq!(home.handle(press, now), None);
            }
            home.advance_to(pacer.map_or(now, |p| p.shows(now)));
            let view = home.view();
            let lift = |i: usize| view.tiles.iter().find(|t| t.index == i).unwrap().lift;
            lifts.push((lift(0), lift(1)));
            home.frame_drawn();
            if let Some(p) = pacer.as_mut() {
                p.drawn(now, r);
            }
        }
        lifts
    }

    /// The B9's burst start: frames 2 and 3 take 9 and 31 ms.
    const B9: [f64; 2] = [9.0, 31.0];
    /// The same two vsyncs, evenly: about 17 and 17 ms.
    const EVEN: [f64; 2] = [17.0, 16.333];

    #[test]
    fn plausible_rates_give_their_period_and_misreads_none() {
        assert_eq!(period_of(240), Some(Duration::from_nanos(4_166_666)));
        assert_eq!(period_of(60), Some(DEFAULT_PERIOD));
        assert_eq!(period_of(24), Some(Duration::from_nanos(41_666_666)));
        assert_eq!(period_of(500), Some(Duration::from_millis(2)));
        for odd in [0, 1, 23, 501, 10_000, -60, i64::MAX] {
            assert_eq!(period_of(odd), None, "{odd} Hz");
        }
        assert_eq!(Duration::from_secs(1) / DEFAULT_HZ, DEFAULT_PERIOD);
    }

    #[test]
    fn a_tap_moves_the_same_whether_frames_2_and_3_take_9_and_31_or_17_and_17_ms() {
        let b9 = tap(&loop_times(&B9, 24), Some(Pacer::new(P)));
        let even = tap(&loop_times(&EVEN, 24), Some(Pacer::new(P)));
        assert_eq!(b9, even, "frame for frame, bit for bit");
        assert_eq!(b9.last(), Some(&(0.0, 1.0)), "and it settles");
    }

    #[test]
    fn stepped_by_real_time_the_same_tap_moves_unevenly() {
        let b9 = tap(&loop_times(&B9, 24), None);
        let even = tap(&loop_times(&EVEN, 24), None);
        // Frame 3 is 9 ms of motion past frame 2 instead of 17.
        let gap = (b9[2].1 - even[2].1).abs();
        assert!(gap > 0.05, "frame 3's lift differs by {gap}");
    }

    /// Audit finding 2: a press applied at the time its frame is drawn at
    /// moves nothing in that frame.
    #[test]
    fn a_taps_first_frame_already_moves() {
        let unpaced = tap(&loop_times(&B9, 1), None);
        assert_eq!(
            unpaced[0],
            (1.0, 0.0),
            "stepped by real time: the frame before"
        );
        let paced = tap(&loop_times(&B9, 1), Some(Pacer::new(P)));
        assert!(paced[0].0 < 1.0 && paced[0].1 > 0.0, "{:?}", paced[0]);
    }

    #[test]
    fn every_frame_of_a_burst_is_one_period_on() {
        let mut pacer = Pacer::new(P);
        let real = loop_times(&B9, 30);
        let shown = paced(&mut pacer, &real);
        assert_eq!(shown[0], real[0], "the first frame starts from real time");
        for pair in shown.windows(2) {
            assert_eq!(pair[1] - pair[0], P);
        }
        assert_eq!(pacer.take_skipped(), 0, "the queue filling is not a miss");
    }

    #[test]
    fn a_missed_vsync_skips_whole_periods_and_keeps_up() {
        // Steady, then one frame of 33.3 ms (a vsync missed), then steady.
        let mut took = vec![16.667; 20];
        took.push(33.333);
        took.extend([16.667; 10]);
        let real = loop_times(&took, 34);
        let mut pacer = Pacer::new(P);
        let shown = paced(&mut pacer, &real);
        let steps: Vec<Duration> = shown.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(steps.iter().filter(|&&s| s == P * 2).count(), 1, "one skip");
        assert!(steps.iter().all(|&s| s == P || s == P * 2), "{steps:?}");
        assert_eq!(pacer.take_skipped(), 1);
        let behind = real[33].saturating_sub(shown[33]);
        assert!(behind <= P / 2, "{behind:?} behind real time");
    }

    #[test]
    fn a_long_stall_jumps_ahead_in_whole_periods() {
        // A launch blocks the loop for 573 ms mid-burst.
        let real = loop_times(&[16.667, 573.0, 16.667], 6);
        let mut pacer = Pacer::new(P);
        let shown = paced(&mut pacer, &real);
        let mut skips = 0;
        for pair in shown.windows(2) {
            let step = (pair[1] - pair[0]).as_nanos();
            assert_eq!(step % P.as_nanos(), 0, "{step} ns");
            skips += step / P.as_nanos() - 1;
        }
        assert!(skips > 30, "{skips}");
        assert_eq!(u128::from(pacer.take_skipped()), skips);
        assert_eq!(pacer.take_skipped(), 0, "taken once");
        let behind = real[5].saturating_sub(shown[5]);
        assert!(behind <= P / 2, "{behind:?} behind real time");
    }

    #[test]
    fn quick_frames_mid_burst_are_no_skip_and_leave_none_behind() {
        // Steady, then two frames 9 ms apart (the queue taking frames in
        // early), then steady again.
        let mut took = vec![16.667; 10];
        took.extend([9.0, 9.0]);
        took.extend([16.667; 10]);
        let real = loop_times(&took, 24);
        let mut pacer = Pacer::new(P);
        let shown = paced(&mut pacer, &real);
        assert!(shown.windows(2).all(|w| w[1] - w[0] == P));
        assert_eq!(pacer.take_skipped(), 0);
    }

    #[test]
    fn a_burst_cut_off_counts_no_skip_for_the_wait() {
        // A launch ends the burst, blocks for 573 ms, and the next frame
        // starts a burst of its own.
        let mut pacer = Pacer::new(P);
        let before = paced(&mut pacer, &loop_times(&[], 5));
        pacer.idle();
        let after = pacer.time(before[4] + ms(573.0));
        pacer.drawn(after, before[4] + ms(573.0));
        assert_eq!(after, before[4] + ms(573.0), "from real time");
        assert_eq!(pacer.take_skipped(), 0);
    }

    #[test]
    fn frames_faster_than_the_period_follow_real_time() {
        // A panel faster than the period says: a frame every millisecond.
        let mut pacer = Pacer::new(P);
        let mut t = Duration::ZERO;
        for n in 0..1000u32 {
            let real = ms(f64::from(n));
            t = pacer.time(real);
            pacer.drawn(t, real);
            assert!(t <= real + P * MAX_LEAD, "{t:?} at {real:?}");
        }
        assert!(t >= ms(999.0), "motion keeps real speed: {t:?}");
    }

    #[test]
    fn a_clock_that_moves_one_period_a_frame_is_left_as_it_is() {
        let mut pacer = Pacer::new(P);
        for n in 0..100u32 {
            let real = P * n;
            let t = pacer.time(real);
            assert_eq!(t, real);
            pacer.drawn(t, real);
        }
        assert_eq!(pacer.take_skipped(), 0);
    }

    #[test]
    fn a_new_burst_starts_from_what_the_screen_shows_and_never_runs_back() {
        let mut pacer = Pacer::new(P);
        let t = pacer.time(ms(1000.0));
        pacer.drawn(t, ms(1000.0));
        let second = pacer.time(ms(1003.0));
        pacer.drawn(second, ms(1003.0));
        pacer.idle();
        assert_eq!(pacer.time(ms(1010.0)), pacer.shows(second));
        assert_eq!(pacer.time(ms(5000.0)), ms(5000.0));
    }

    #[test]
    fn a_zero_period_still_moves() {
        let mut pacer = Pacer::new(Duration::ZERO);
        assert_eq!(pacer.period(), Duration::from_nanos(1));
        let t = pacer.time(ms(5.0));
        pacer.drawn(t, ms(5.0));
        assert!(pacer.time(ms(6.0)) > t);
    }
}
