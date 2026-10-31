// SPDX-License-Identifier: GPL-3.0-or-later
//
// The demo script: changes made on the TV's side, as if someone were holding
// the Magic Remote, so the UI can be seen following the TV.
import type { FakeTv } from "./tv.ts";

export interface DemoOptions {
  volumeEveryMs?: number;
  appEveryMs?: number;
  random?: () => number;
}

/** The apps the demo cycles through, an input among them. */
export const DEMO_APPS = [
  "netflix",
  "youtube.leanback.v4",
  "cdp-30",
  "com.webos.app.hdmi1",
  "spotify-beehive",
  "amazon",
];

/** Starts the script; returns its stop. */
export function startDemo(tv: FakeTv, opts: DemoOptions = {}): () => void {
  const random = opts.random ?? Math.random;
  const volume = setInterval(() => {
    // A walk of 1 to 4 steps, kept between 6 and 40: the range a living room uses.
    const step = (1 + Math.floor(random() * 4)) * (random() < 0.5 ? -1 : 1);
    const level = Math.max(6, Math.min(40, tv.volume + step));
    tv.setVolume(level === tv.volume ? level - step : level, tv.muted);
  }, opts.volumeEveryMs ?? 5_000);
  const app = setInterval(() => {
    const next = (DEMO_APPS.indexOf(tv.foreground ?? "") + 1) % DEMO_APPS.length;
    tv.setForeground(DEMO_APPS[next] ?? null);
  }, opts.appEveryMs ?? 15_000);
  return () => {
    clearInterval(volume);
    clearInterval(app);
  };
}
