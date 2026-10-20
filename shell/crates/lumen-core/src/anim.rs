//! Motion: a critically damped spring, stepped with its exact solution.
//!
//! The closed form means a frame of any length lands on the same curve: a slow
//! frame shows as a jump, never as slow motion, and many small steps add up to
//! one big one. Retargeting keeps the velocity, so quick key presses chain.

use std::f32::consts::E;

/// A value pulled toward a target by a critically damped spring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    /// Angular frequency, per second: higher is faster.
    omega: f32,
    /// Distance from the target that counts as there.
    epsilon: f32,
    /// Speed that counts as stopped.
    rest_speed: f32,
}

/// How fast a spring moves and when it counts as settled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub omega: f32,
    pub epsilon: f32,
    pub rest_speed: f32,
}

impl Spring {
    /// A spring at rest on `value`.
    pub const fn new(value: f32, tuning: Tuning) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            omega: tuning.omega,
            epsilon: tuning.epsilon,
            rest_speed: tuning.rest_speed,
        }
    }

    pub const fn value(&self) -> f32 {
        self.value
    }

    pub const fn target(&self) -> f32 {
        self.target
    }

    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Pulls toward a new target from wherever the spring is, keeping its speed.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Adds `dv` to the velocity: a nudge that springs back.
    pub fn kick(&mut self, dv: f32) {
        self.velocity += dv;
    }

    /// Replaces the velocity, keeping the position: a fresh nudge from
    /// wherever the spring is, which does not stack on one in progress.
    pub fn set_velocity(&mut self, v: f32) {
        self.velocity = v;
    }

    /// Jumps onto the target and stops.
    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }

    /// At the target and not moving.
    pub fn is_settled(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    /// Advances by `dt` seconds. Within `epsilon` of the target and slower than
    /// `rest_speed`, the spring snaps onto the target and stops.
    pub fn step(&mut self, dt: f32) {
        if self.is_settled() || dt.is_nan() || dt <= 0.0 {
            return;
        }
        // Past a minute the spring has long since arrived, and an infinite
        // step would turn the arithmetic below into NaN.
        let dt = dt.min(60.0);
        let w = self.omega;
        let x0 = self.value - self.target;
        let v0 = self.velocity;
        let decay = (-w * dt).exp();
        let c = v0 + w * x0;
        let x = (x0 + c * dt) * decay;
        let v = (v0 - w * c * dt) * decay;
        self.value = self.target + x;
        self.velocity = v;
        if x.abs() < self.epsilon && v.abs() < self.rest_speed {
            self.snap();
        }
    }
}

/// The velocity kick that moves a spring at rest out to `peak` and back. A
/// critically damped spring kicked with `v` peaks at `v / (omega * e)`, after
/// `1 / omega` seconds.
pub fn kick_for_peak(peak: f32, omega: f32) -> f32 {
    peak * omega * E
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;
    const PX: Tuning = Tuning {
        omega: 20.0,
        epsilon: 0.25,
        rest_speed: 2.0,
    };
    const UNIT: Tuning = Tuning {
        omega: 30.0,
        epsilon: 0.001,
        rest_speed: 0.01,
    };

    /// Steps at 60 fps until settled; returns the time it took.
    fn settle(spring: &mut Spring) -> f32 {
        let mut t = 0.0;
        while !spring.is_settled() {
            spring.step(FRAME);
            t += FRAME;
            assert!(t < 5.0, "spring never settled: {spring:?}");
        }
        t
    }

    #[test]
    fn a_new_spring_is_at_rest() {
        let mut s = Spring::new(3.0, PX);
        assert!(s.is_settled());
        s.step(FRAME);
        assert_eq!(s.value(), 3.0);
    }

    #[test]
    fn settles_exactly_on_the_target() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(640.0);
        assert!(!s.is_settled());
        let t = settle(&mut s);
        assert_eq!(s.value(), 640.0);
        assert_eq!(s.velocity(), 0.0);
        // The spec: a 640 px scroll settles in about 0.55 s.
        assert!((0.45..0.65).contains(&t), "settled after {t} s");
    }

    #[test]
    fn lift_settles_in_about_350_ms() {
        let mut s = Spring::new(0.0, UNIT);
        s.set_target(1.0);
        let t = settle(&mut s);
        assert!((0.3..0.4).contains(&t), "settled after {t} s");
    }

    #[test]
    fn critically_damped_never_overshoots() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(100.0);
        let mut last = 0.0;
        while !s.is_settled() {
            s.step(FRAME);
            assert!(s.value() <= 100.0, "overshot to {}", s.value());
            assert!(s.value() >= last, "moved backwards");
            last = s.value();
        }
    }

    #[test]
    fn one_long_step_matches_many_short_ones() {
        let mut short = Spring::new(0.0, PX);
        let mut long = short;
        short.set_target(280.0);
        long.set_target(280.0);
        for _ in 0..6 {
            short.step(0.05);
        }
        long.step(0.3);
        assert!((short.value() - long.value()).abs() < 0.01);
        assert!((short.velocity() - long.velocity()).abs() < 0.1);
    }

    #[test]
    fn a_hitch_jumps_ahead_instead_of_slowing_down() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(280.0);
        s.step(10.0);
        assert!(s.is_settled());
        assert_eq!(s.value(), 280.0);
    }

    #[test]
    fn zero_negative_or_nan_time_changes_nothing() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(50.0);
        s.step(0.0);
        s.step(-1.0);
        s.step(f32::NAN);
        assert_eq!(s.value(), 0.0);
    }

    #[test]
    fn an_infinite_step_arrives_instead_of_breaking() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(50.0);
        s.kick(100.0);
        s.step(f32::INFINITY);
        assert!(s.is_settled());
        assert_eq!(s.value(), 50.0);
    }

    #[test]
    fn retargeting_keeps_the_velocity() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(280.0);
        s.step(0.05);
        let v = s.velocity();
        assert!(v > 0.0);
        s.set_target(560.0);
        assert_eq!(s.velocity(), v);
    }

    #[test]
    fn a_kick_peaks_where_asked_and_returns() {
        let mut s = Spring::new(0.0, PX);
        s.kick(kick_for_peak(14.0, PX.omega));
        let (mut peak, mut peak_t, mut t) = (0.0f32, 0.0, 0.0);
        let dt = 0.001;
        while !s.is_settled() {
            s.step(dt);
            t += dt;
            if s.value() > peak {
                peak = s.value();
                peak_t = t;
            }
        }
        assert!((peak - 14.0).abs() < 0.05, "peak {peak}");
        assert!((peak_t - 0.05).abs() < 0.002, "peak at {peak_t} s");
        assert_eq!(s.value(), 0.0);
        // The spec: settled in about 0.45 s.
        assert!((0.35..0.5).contains(&t), "settled after {t} s");
    }

    #[test]
    fn snap_jumps_to_the_target() {
        let mut s = Spring::new(0.0, PX);
        s.set_target(10.0);
        s.kick(100.0);
        s.snap();
        assert!(s.is_settled());
        assert_eq!(s.value(), 10.0);
    }
}
