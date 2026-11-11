// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { Swipe } from "../src/swipe.ts";

const opts = { step: 40, slop: 8, tapMs: 300, holdMs: 600 };

describe("the touchpad's swipes", () => {
  it("a touch that ends where it began, soon, is OK", () => {
    const s = new Swipe(opts);
    s.down(100, 100, 0);
    expect(s.move(103, 101)).toEqual([]);
    expect(s.up(120)).toBe("ok");
  });

  it("a slow touch is not a tap", () => {
    const s = new Swipe(opts);
    s.down(100, 100, 0);
    expect(s.up(400)).toBeNull();
  });

  it("a drag presses one arrow for each step it covers", () => {
    const s = new Swipe(opts);
    s.down(0, 0, 0);
    expect(s.move(39, 0)).toEqual([]);
    expect(s.move(41, 0)).toEqual(["right"]);
    expect(s.move(125, 0)).toEqual(["right", "right"]);
    expect(s.up(200)).toBeNull();
  });

  it("goes each of the four ways", () => {
    for (const [x, y, key] of [
      [-50, 0, "left"],
      [50, 0, "right"],
      [0, -50, "up"],
      [0, 50, "down"],
    ] as const) {
      const s = new Swipe(opts);
      s.down(0, 0, 0);
      expect(s.move(x, y)).toEqual([key]);
    }
  });

  it("a slanted swipe stays one direction", () => {
    const s = new Swipe(opts);
    s.down(0, 0, 0);
    expect(s.move(45, 20)).toEqual(["right"]);
    expect(s.move(90, 40)).toEqual(["right"]);
  });

  it("turning back presses the other way after a full step", () => {
    const s = new Swipe(opts);
    s.down(0, 0, 0);
    expect(s.move(45, 0)).toEqual(["right"]);
    expect(s.move(20, 0)).toEqual([]);
    expect(s.move(0, 0)).toEqual(["left"]);
  });

  it("a touch that stays put is Back, once, and then no OK", () => {
    const s = new Swipe(opts);
    s.down(0, 0, 0);
    expect(s.hold(500)).toBeNull();
    expect(s.hold(650)).toBe("back");
    expect(s.hold(900)).toBeNull();
    expect(s.move(120, 0)).toEqual([]);
    expect(s.up(950)).toBeNull();
  });

  it("a drag is never a hold", () => {
    const s = new Swipe(opts);
    s.down(0, 0, 0);
    s.move(50, 0);
    expect(s.hold(700)).toBeNull();
  });

  it("does nothing before a touch or after a cancel", () => {
    const s = new Swipe(opts);
    expect(s.move(50, 0)).toEqual([]);
    expect(s.up(10)).toBeNull();
    s.down(0, 0, 0);
    s.cancel();
    expect(s.move(50, 0)).toEqual([]);
    expect(s.up(10)).toBeNull();
  });
});
