// SPDX-License-Identifier: GPL-3.0-or-later
import { beforeEach, describe, expect, it } from "vitest";
import { VERBS } from "../../ui/src/protocol.ts";
import type { Event } from "../../ui/src/protocol.ts";
import { startDemo } from "../src/demo.ts";
import { LUMEN_ID } from "../src/fixtures.ts";
import { FakeTv } from "../src/tv.ts";

let tv: FakeTv;
let events: Event[];
let later: (() => void)[];

/** Runs what the TV scheduled, as LG's services answering. */
function settle(): void {
  while (later.length) later.shift()?.();
}

function ask(verb: string, args?: unknown, id = 1) {
  return tv.handle(JSON.stringify(args === undefined ? { id, verb } : { id, verb, args }));
}

beforeEach(() => {
  later = [];
  tv = new FakeTv({ schedule: (_ms, fn) => later.push(fn), now: () => new Date("2026-10-30T12:00:00Z") });
  events = [];
  tv.onEvent((e) => events.push(e));
});

describe("FakeTv", () => {
  it("says hello with every verb, then its state", () => {
    expect(tv.hello("t")).toEqual({ event: "hello", data: { protocol: 1, server: "t", verbs: VERBS } });
    expect(tv.snapshot().map((e) => e.event)).toEqual(["power", "volume", "foreground", "inputs", "settings"]);
  });

  it("answers every verb", () => {
    const args: Record<string, unknown> = {
      "volume.set": { level: 3 },
      "mute.set": { muted: true },
      launch: { id: "netflix" },
      key: { name: "ok" },
      "text.insert": { text: "a" },
      "text.delete": { count: 1 },
      "settings.set": { key: "sound.mode", value: "music" },
      toast: { text: "hi" },
    };
    VERBS.forEach((verb, i) => {
      expect(ask(verb, args[verb], i + 1), verb).toMatchObject({ id: i + 1, ok: true });
    });
  });

  it("changes the volume and pushes it", () => {
    ask("volume.set", { level: 30 });
    ask("volume.up");
    ask("mute.set", { muted: true });
    ask("volume.down");
    expect(events).toEqual([
      { event: "volume", data: { level: 30, muted: false } },
      { event: "volume", data: { level: 31, muted: false } },
      { event: "volume", data: { level: 31, muted: true } },
      { event: "volume", data: { level: 30, muted: false } },
    ]);
  });

  it("stops the volume at its ends without an event", () => {
    ask("volume.set", { level: 100 });
    events = [];
    ask("volume.up");
    expect(events).toEqual([]);
  });

  it("refuses bad requests as lumen-ctl will", () => {
    expect(ask("volume.set", { level: 101 })).toMatchObject({ ok: false, error: { code: "bad-args" } });
    expect(ask("reboot")).toMatchObject({ ok: false, error: { code: "unknown-verb" } });
    expect(tv.handle("{")).toEqual({ id: null, ok: false, error: { code: "bad-line", message: "not JSON" } });
    expect(ask("launch", { id: "no.such.app" })).toMatchObject({ ok: false, error: { code: "not-found" } });
    expect(events).toEqual([]);
  });

  it("brings a launched app to the front a moment later", () => {
    ask("launch", { id: "com.webos.app.hdmi1" });
    expect(events).toEqual([]);
    settle();
    expect(events).toEqual([{ event: "foreground", data: { id: "com.webos.app.hdmi1" } }]);
    ask("lumen.open");
    settle();
    expect(tv.foreground).toBe(LUMEN_ID);
  });

  it("keeps a text field", () => {
    ask("text.insert", { text: "héllo 👋" });
    ask("text.delete", { count: 2 });
    expect(tv.text).toBe("héllo");
    ask("text.enter");
    expect(tv.text).toBe("");
  });

  it("pushes a setting only when it changes", () => {
    ask("settings.set", { key: "picture.mode", value: "cinema" });
    expect(events).toEqual([]);
    ask("settings.set", { key: "picture.mode", value: "vivid" });
    expect(events).toEqual([{ event: "settings", data: { values: { "picture.mode": "vivid" } } }]);
    expect(ask("settings.get", { keys: ["picture.mode"] })).toMatchObject({
      result: { values: { "picture.mode": "vivid" } },
    });
  });

  it("turns off: standby, then the server's hook", () => {
    let off = false;
    tv.onPowerOff = () => (off = true);
    ask("power.off");
    expect(events).toEqual([{ event: "power", data: { state: "standby" } }]);
    expect(off).toBe(false);
    settle();
    expect(off).toBe(true);
  });

  it("starts the screensaver, and a key wakes it", () => {
    ask("ambient.start");
    settle();
    expect(tv.power).toBe("screensaver");
    ask("key", { name: "up" });
    expect(tv.power).toBe("active");
  });

  it("reports the privacy hook's work, and Guard's counts when asked to", () => {
    const r = ask("privacy.state");
    expect(r).toMatchObject({
      ok: true,
      result: { hook: { installed: true }, port11111: { rejected: true }, guard: null },
    });
    tv = new FakeTv({ guard: true });
    expect(ask("privacy.state")).toMatchObject({ result: { guard: { blocked: 0 } } });
  });

  it("without a route for keys, leaves key out of hello and refuses it", () => {
    tv = new FakeTv({ keys: false });
    const hello = tv.hello("t");
    expect(hello.event === "hello" && hello.data.verbs.includes("key")).toBe(false);
    expect(ask("key", { name: "ok" })).toMatchObject({ ok: false, error: { code: "unavailable" } });
  });
});

describe("startDemo", () => {
  it("walks the volume and cycles the apps", () => {
    const timers: (() => void)[] = [];
    const real = globalThis.setInterval;
    globalThis.setInterval = ((fn: () => void) => {
      timers.push(fn);
      return 0;
    }) as unknown as typeof setInterval;
    try {
      const stop = startDemo(tv, { random: () => 0.9 });
      timers[0]?.();
      timers[1]?.();
      stop();
    } finally {
      globalThis.setInterval = real;
    }
    expect(events).toEqual([
      { event: "volume", data: { level: 18, muted: false } },
      { event: "foreground", data: { id: "youtube.leanback.v4" } },
    ]);
  });
});
