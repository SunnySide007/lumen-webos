// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { Glide, MoveQueue } from "../src/glide.ts";

const plain = { gain: 2, boost: 0, maxBoost: 1, slop: 6, tapMs: 320, holdMs: 600 };

describe("the touchpad as a pointer", () => {
  it("a short, still touch is a click and moves nothing", () => {
    const g = new Glide(plain);
    g.down(100, 100, 0);
    expect(g.move(102, 101)).toBeNull();
    expect(g.up(200)).toBe("click");
  });

  it("a drag moves the pointer by the gain and is no click", () => {
    const g = new Glide(plain);
    g.down(100, 100, 0);
    // The first 6 pixels are the slop and are dropped.
    expect(g.move(110, 100)).toBeNull();
    expect(g.move(120, 95)).toEqual({ dx: 20, dy: -10 });
    expect(g.move(110, 95)).toEqual({ dx: -20, dy: 0 });
    expect(g.up(250)).toBeNull();
  });

  it("keeps the fractions of a pixel for the next move", () => {
    const g = new Glide({ ...plain, gain: 0.4 });
    g.down(0, 0, 0);
    g.move(10, 0);
    expect(g.move(11, 0)).toBeNull();
    expect(g.move(12, 0)).toBeNull();
    expect(g.move(13, 0)).toEqual({ dx: 1, dy: 0 });
  });

  it("a fast move carries farther than a slow one, up to a limit", () => {
    const g = new Glide({ ...plain, boost: 0.05, maxBoost: 3 });
    g.down(0, 0, 0);
    g.move(10, 0);
    // 2 pixels: gain 2 × 1.1; 20 pixels: 2 × 2; 200 pixels: 2 × 3, the limit.
    expect(g.move(12, 0)).toEqual({ dx: 4, dy: 0 });
    expect(g.move(32, 0)).toEqual({ dx: 80, dy: 0 });
    expect(g.move(232, 0)).toEqual({ dx: 1200, dy: 0 });
  });

  it("a touch that stays put is Back, once, and is then spent", () => {
    const g = new Glide(plain);
    g.down(0, 0, 0);
    expect(g.hold(300)).toBeNull();
    expect(g.hold(620)).toBe("back");
    expect(g.hold(900)).toBeNull();
    expect(g.move(80, 0)).toBeNull();
    expect(g.up(950)).toBeNull();
  });

  it("a touch that moved is never Back, and a slow tap is no click", () => {
    const g = new Glide(plain);
    g.down(0, 0, 0);
    g.move(20, 0);
    expect(g.hold(700)).toBeNull();
    g.down(0, 0, 1000);
    expect(g.up(1400)).toBeNull();
  });

  it("a cancelled touch does nothing more", () => {
    const g = new Glide(plain);
    g.down(0, 0, 0);
    g.cancel();
    expect(g.move(50, 0)).toBeNull();
    expect(g.up(10)).toBeNull();
  });
});

describe("moves waiting for the link", () => {
  it("add up into one request", () => {
    const q = new MoveQueue();
    expect(q.take()).toBeNull();
    q.add({ dx: 5, dy: -2 });
    q.add({ dx: 7, dy: -1 });
    expect(q.take()).toEqual({ dx: 12, dy: -3 });
    expect(q.take()).toBeNull();
  });

  it("send at most the whole screen at once and keep the rest", () => {
    const q = new MoveQueue();
    q.add({ dx: 2500, dy: -1200 });
    expect(q.take()).toEqual({ dx: 1920, dy: -1080 });
    expect(q.take()).toEqual({ dx: 580, dy: -120 });
    q.add({ dx: 1, dy: 0 });
    q.clear();
    expect(q.take()).toBeNull();
  });
});
