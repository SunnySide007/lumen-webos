// SPDX-License-Identifier: GPL-3.0-or-later
//
// Springs as CSS easing. A damped spring from 0 to 1 is sampled into a CSS
// `linear()` function, so the browser runs the motion on its compositor and
// no script runs per frame. The shell moves on critically damped springs
// (look-and-feel.md, "Motion"); Remote's bolder controls get a little
// overshoot on top.

export interface Spring {
  /** Natural frequency, per second: higher is faster. */
  omega: number;
  /** Damping ratio: 1 settles without overshoot, below 1 overshoots. */
  zeta: number;
}

/** Position at time `t` (seconds) of a spring released at 0 toward 1. */
export function springAt({ omega, zeta }: Spring, t: number): number {
  if (zeta >= 1) return 1 - Math.exp(-omega * t) * (1 + omega * t);
  const wd = omega * Math.sqrt(1 - zeta * zeta);
  const decay = Math.exp(-zeta * omega * t);
  return 1 - decay * (Math.cos(wd * t) + ((zeta * omega) / wd) * Math.sin(wd * t));
}

/** How long the spring takes to stay within `tolerance` of 1, in ms. */
export function settleMs(spring: Spring, tolerance = 0.001): number {
  let last = 0;
  for (let ms = 0; ms <= 5_000; ms++) {
    if (Math.abs(1 - springAt(spring, ms / 1000)) > tolerance) last = ms;
  }
  return last + 1;
}

/** The spring as `linear(…)` with `points` samples, and its duration. */
export function springEasing(spring: Spring, points = 32): { easing: string; ms: number } {
  const ms = settleMs(spring);
  const stops: string[] = [];
  for (let i = 0; i <= points; i++) {
    const p = i / points;
    const x = i === points ? 1 : springAt(spring, (p * ms) / 1000);
    stops.push(i === 0 || i === points ? `${x}` : `${round(x)} ${round(p * 100, 2)}%`);
  }
  return { easing: `linear(${stops.join(", ")})`, ms };
}

function round(x: number, places = 4): number {
  const f = 10 ** places;
  return Math.round(x * f) / f;
}

/** The two springs the CSS uses: `--spring` for controls that morph, and
 * `--spring-soft` for things that settle back, such as a tile's tilt. */
export const SPRINGS = {
  spring: { omega: 22, zeta: 0.62 },
  "spring-soft": { omega: 16, zeta: 0.9 },
} as const satisfies Record<string, Spring>;
