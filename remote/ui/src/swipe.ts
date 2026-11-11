// SPDX-License-Identifier: GPL-3.0-or-later
//
// The touchpad's arithmetic: a finger's path turned into the TV's keys. A
// drag presses one arrow for every `step` pixels it covers, in whichever
// direction leads; a touch that ends where it began, soon enough, is OK; one
// that stays put is Back. No DOM here, so the rules are tested as numbers.
import type { KeyName } from "./protocol.ts";

export interface SwipeOptions {
  /** Pixels of travel for one arrow. */
  step: number;
  /** Travel under this many pixels is still a tap. */
  slop: number;
  /** A tap ends within this many milliseconds. */
  tapMs: number;
  /** A touch that stays put this long is Back. */
  holdMs: number;
}

export const SWIPE_DEFAULTS: SwipeOptions = { step: 44, slop: 9, tapMs: 320, holdMs: 600 };

/** One finger's touch, from down to up. */
export class Swipe {
  private readonly o: SwipeOptions;
  private x = 0;
  private y = 0;
  private at = 0;
  /** Travel not yet spent on an arrow. */
  private dx = 0;
  private dy = 0;
  private moved = false;
  private held = false;
  private active = false;

  constructor(options: Partial<SwipeOptions> = {}) {
    this.o = { ...SWIPE_DEFAULTS, ...options };
  }

  down(x: number, y: number, at: number): void {
    this.x = x;
    this.y = y;
    this.at = at;
    this.dx = this.dy = 0;
    this.moved = this.held = false;
    this.active = true;
  }

  /** The arrows this move earns, oldest first. A touch that was held for
   * Back is spent: dragging it afterwards moves nothing. */
  move(x: number, y: number): KeyName[] {
    if (!this.active || this.held) return [];
    this.dx += x - this.x;
    this.dy += y - this.y;
    this.x = x;
    this.y = y;
    if (!this.moved && Math.hypot(this.dx, this.dy) < this.o.slop) return [];
    this.moved = true;
    const keys: KeyName[] = [];
    // The leading axis takes the step and the other axis's drift is dropped,
    // so a slightly slanted swipe stays one direction.
    for (;;) {
      const horizontal = Math.abs(this.dx) >= Math.abs(this.dy);
      const lead = horizontal ? this.dx : this.dy;
      if (Math.abs(lead) < this.o.step) break;
      const sign = Math.sign(lead);
      keys.push(horizontal ? (sign > 0 ? "right" : "left") : sign > 0 ? "down" : "up");
      if (horizontal) {
        this.dx -= sign * this.o.step;
        this.dy = 0;
      } else {
        this.dy -= sign * this.o.step;
        this.dx = 0;
      }
    }
    return keys;
  }

  /** Called while the finger rests: Back, once, after `holdMs` without
   * moving. */
  hold(at: number): KeyName | null {
    if (!this.active || this.moved || this.held || at - this.at < this.o.holdMs) return null;
    this.held = true;
    return "back";
  }

  /** The finger lifts: OK for a tap, nothing after a drag or a hold. */
  up(at: number): KeyName | null {
    if (!this.active) return null;
    this.active = false;
    return !this.moved && !this.held && at - this.at <= this.o.tapMs ? "ok" : null;
  }

  cancel(): void {
    this.active = false;
  }
}
