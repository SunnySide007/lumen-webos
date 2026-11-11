// SPDX-License-Identifier: GPL-3.0-or-later
//
// The touchpad's arithmetic as a pointer: a finger's path turned into moves
// of the TV's pointer, in pixels of a 1920 by 1080 screen. A slow drag moves
// it finely and a fast one carries it across, as a laptop's touchpad does; a
// touch that ends where it began, soon enough, is a click; one that stays put
// is Back. No DOM here, so the rules are tested as numbers.
import { POINTER_SPAN } from "./protocol.ts";

export interface GlideOptions {
  /** TV pixels for one pixel of slow travel. */
  gain: number;
  /** How much each pixel of one move's travel adds to the gain. */
  boost: number;
  /** The most the gain grows to, as a multiple. */
  maxBoost: number;
  /** Travel under this many pixels is still a tap. */
  slop: number;
  /** A tap ends within this many milliseconds. */
  tapMs: number;
  /** A touch that stays put this long is Back. */
  holdMs: number;
}

export const GLIDE_DEFAULTS: GlideOptions = { gain: 2.2, boost: 0.05, maxBoost: 3, slop: 6, tapMs: 320, holdMs: 600 };

export interface Move {
  dx: number;
  dy: number;
}

/** One finger's touch, from down to up. */
export class Glide {
  private readonly o: GlideOptions;
  private x = 0;
  private y = 0;
  private at = 0;
  private travel = 0;
  /** Fractions of a TV pixel not yet sent. */
  private rx = 0;
  private ry = 0;
  private moved = false;
  private held = false;
  private active = false;

  constructor(options: Partial<GlideOptions> = {}) {
    this.o = { ...GLIDE_DEFAULTS, ...options };
  }

  down(x: number, y: number, at: number): void {
    this.x = x;
    this.y = y;
    this.at = at;
    this.travel = this.rx = this.ry = 0;
    this.moved = this.held = false;
    this.active = true;
  }

  /** How far this move carries the TV's pointer, in whole pixels, or null
   * when it carries it nowhere. The travel inside the slop is dropped, so a
   * tap never nudges the pointer; a touch held for Back is spent. */
  move(x: number, y: number): Move | null {
    if (!this.active || this.held) return null;
    const dx = x - this.x;
    const dy = y - this.y;
    this.x = x;
    this.y = y;
    const length = Math.hypot(dx, dy);
    if (!this.moved) {
      this.travel += length;
      if (this.travel < this.o.slop) return null;
      this.moved = true;
      return null;
    }
    const gain = this.o.gain * Math.min(this.o.maxBoost, 1 + this.o.boost * length);
    this.rx += dx * gain;
    this.ry += dy * gain;
    const move = { dx: Math.trunc(this.rx), dy: Math.trunc(this.ry) };
    this.rx -= move.dx;
    this.ry -= move.dy;
    return move.dx === 0 && move.dy === 0 ? null : move;
  }

  /** Called while the finger rests: Back, once, after `holdMs` without
   * moving. */
  hold(at: number): "back" | null {
    if (!this.active || this.moved || this.held || at - this.at < this.o.holdMs) return null;
    this.held = true;
    return "back";
  }

  /** The finger lifts: a click for a tap, nothing after a drag or a hold. */
  up(at: number): "click" | null {
    if (!this.active) return null;
    this.active = false;
    return !this.moved && !this.held && at - this.at <= this.o.tapMs ? "click" : null;
  }

  cancel(): void {
    this.active = false;
  }
}

/** Moves waiting for the link, added up: the pointer's requests go one at a
 * time, so however fast the finger is the TV gets the newest place in as few
 * requests as the link allows. */
export class MoveQueue {
  private dx = 0;
  private dy = 0;

  add(move: Move): void {
    this.dx += move.dx;
    this.dy += move.dy;
  }

  /** The next request's move, at most the whole screen, or null when
   * nothing waits. What is over stays for the request after. */
  take(): Move | null {
    if (this.dx === 0 && this.dy === 0) return null;
    const [w, h] = POINTER_SPAN;
    const move = { dx: Math.max(-w, Math.min(w, this.dx)), dy: Math.max(-h, Math.min(h, this.dy)) };
    this.dx -= move.dx;
    this.dy -= move.dy;
    return move;
  }

  clear(): void {
    this.dx = this.dy = 0;
  }
}
