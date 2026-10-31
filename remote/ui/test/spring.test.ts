// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { SPRINGS, settleMs, springAt, springEasing } from "../src/spring.ts";

describe("springs", () => {
  it("start at 0 and settle at 1", () => {
    for (const s of Object.values(SPRINGS)) {
      expect(springAt(s, 0)).toBeCloseTo(0, 9);
      expect(springAt(s, 5)).toBeCloseTo(1, 6);
    }
  });

  it("overshoot only when underdamped", () => {
    const peak = (zeta: number) =>
      Math.max(...Array.from({ length: 2000 }, (_, i) => springAt({ omega: 20, zeta }, i / 1000)));
    expect(peak(0.5)).toBeGreaterThan(1.05);
    expect(peak(1)).toBeLessThanOrEqual(1);
  });

  it("settle sooner when stiffer", () => {
    expect(settleMs({ omega: 30, zeta: 1 })).toBeLessThan(settleMs({ omega: 15, zeta: 1 }));
  });

  it("become a CSS linear() that runs from 0 to 1", () => {
    const { easing, ms } = springEasing(SPRINGS.spring, 8);
    expect(easing).toMatch(/^linear\(0, .*, 1\)$/);
    expect(easing.split(",")).toHaveLength(9);
    expect(ms).toBeGreaterThan(100);
    expect(ms).toBeLessThan(1000);
  });
});
