// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  DEFAULT_AMBIENT,
  EMPTY_STATE,
  Store,
  TILE_PALETTE,
  ambientColor,
  applyEvent,
  frontTitle,
  isLight,
  shortcuts,
  tileFace,
  tiles,
} from "../src/store.ts";
import type { TvState } from "../src/store.ts";

const apps: TvState["apps"] = [
  { id: "com.webos.app.hdmi1", title: "HDMI 1", kind: "input", iconColor: null },
  { id: "netflix", title: "Netflix", kind: "app", iconColor: "#e50914" },
  { id: "com.webos.app.browser", title: "Web Browser", kind: "app", iconColor: null },
];

describe("applyEvent", () => {
  it("folds each watched state in", () => {
    let s = EMPTY_STATE;
    s = applyEvent(s, { event: "power", data: { state: "active" } });
    s = applyEvent(s, { event: "volume", data: { level: 9, muted: false } });
    s = applyEvent(s, { event: "foreground", data: { id: "netflix" } });
    s = applyEvent(s, {
      event: "inputs",
      data: { inputs: [{ id: "com.webos.app.hdmi1", label: "HDMI 1", device: "Streamer", connected: true }] },
    });
    s = applyEvent(s, { event: "settings", data: { values: { "picture.mode": "cinema" } } });
    s = applyEvent(s, { event: "settings", data: { values: { "picture.oledLight": 70 } } });
    expect(s).toMatchObject({
      power: "active",
      volume: { level: 9, muted: false },
      foreground: "netflix",
      inputs: [{ connected: true }],
      settings: { "picture.mode": "cinema", "picture.oledLight": 70 },
    });
  });

  it("returns the same state when nothing changed", () => {
    const s = applyEvent(EMPTY_STATE, { event: "volume", data: { level: 9, muted: false } });
    expect(applyEvent(s, { event: "volume", data: { level: 9, muted: false } })).toBe(s);
    expect(applyEvent(s, { event: "alive", data: {} })).toBe(s);
    expect(applyEvent(s, { event: "hello", data: { protocol: 1, server: "x", verbs: [] } })).toBe(s);
    const t = applyEvent(s, { event: "settings", data: { values: { "sound.mode": "music" } } });
    expect(applyEvent(t, { event: "settings", data: { values: { "sound.mode": "music" } } })).toBe(t);
    const inputs = [{ id: "a", label: "HDMI 1", device: null, connected: false }];
    const u = applyEvent(t, { event: "inputs", data: { inputs } });
    expect(applyEvent(u, { event: "inputs", data: { inputs: [{ ...inputs[0]! }] } })).toBe(u);
  });
});

describe("tiles and the ambient colour", () => {
  const s: TvState = {
    ...EMPTY_STATE,
    apps,
    foreground: "netflix",
    inputs: [{ id: "com.webos.app.hdmi1", label: "HDMI 1", device: "Google TV Streamer", connected: true }],
  };

  it("colours tiles by icon colour, else by position in the shell's palette", () => {
    expect(tiles(s).map((t) => t.color)).toEqual([TILE_PALETTE[0], "#e50914", TILE_PALETTE[2]]);
  });

  it("marks the app in front and each input's connection", () => {
    const t = tiles(s);
    expect(t.map((x) => x.inFront)).toEqual([false, true, false]);
    expect(t[0]).toMatchObject({ connected: true, device: "Google TV Streamer" });
    expect(t[1]).toMatchObject({ connected: null, device: null });
  });

  it("takes the ambient colour from the app in front", () => {
    expect(ambientColor(s)).toBe("#e50914");
    expect(ambientColor({ ...s, foreground: "com.webos.app.browser" })).toBe(TILE_PALETTE[2]);
    expect(ambientColor({ ...s, foreground: "unknown.app" })).toBe(DEFAULT_AMBIENT);
    expect(ambientColor(EMPTY_STATE)).toBe(DEFAULT_AMBIENT);
  });

  it("names the app in front even before apps.list", () => {
    expect(frontTitle(s)).toBe("Netflix");
    expect(frontTitle({ ...EMPTY_STATE, foreground: "x.y" })).toBe("x.y");
    expect(frontTitle(EMPTY_STATE)).toBeNull();
  });
});

describe("Store", () => {
  it("calls listeners now and on change, not on a repeat", () => {
    const store = new Store(1);
    const seen: [number, number][] = [];
    const off = store.subscribe((v, before) => seen.push([v, before]));
    store.set(1);
    store.set(2);
    store.update((v) => v + 1);
    off();
    store.set(9);
    expect(seen).toEqual([
      [1, 1],
      [2, 1],
      [3, 2],
    ]);
  });
});

describe("a small tile's face", () => {
  it("is an HDMI input's number, whatever the owner named it", () => {
    expect(tileFace({ id: "com.webos.app.hdmi2", title: "Home Theater", kind: "input" })).toBe("2");
    expect(tileFace({ id: "com.webos.app.hdmi3", title: "Google Streamer", kind: "input" })).toBe("3");
    expect(tileFace({ id: "x", title: "HDMI 4", kind: "input" })).toBe("4");
  });

  it("is another input's initials and an app's first letter", () => {
    expect(tileFace({ id: "com.webos.app.externalinput.av1", title: "AV", kind: "input" })).toBe("A");
    expect(tileFace({ id: "x", title: "Live TV", kind: "input" })).toBe("LT");
    expect(tileFace({ id: "netflix", title: "netflix", kind: "app" })).toBe("N");
    expect(tileFace({ id: "x", title: "", kind: "app" })).toBe("?");
  });

  it("knows a light tile from a dark one", () => {
    expect(isLight("#ffffff")).toBe(true);
    expect(isLight("#e3c043")).toBe(true);
    expect(isLight("#e50914")).toBe(false);
    expect(isLight("#000000")).toBe(false);
    expect(isLight("var(--ambient)")).toBe(false);
  });
});

describe("shortcuts", () => {
  const list: TvState["apps"] = [
    { id: "com.webos.app.hdmi1", title: "HDMI 1", kind: "input", iconColor: null },
    { id: "com.webos.app.hdmi3", title: "HDMI 3", kind: "input", iconColor: null },
    { id: "com.webos.app.browser", title: "Web Browser", kind: "app", iconColor: null },
    { id: "youtube.leanback.v4", title: "YouTube", kind: "app", iconColor: null },
    { id: "netflix", title: "Netflix", kind: "app", iconColor: "#e50914" },
  ];
  const inputs: TvState["inputs"] = [
    { id: "com.webos.app.hdmi1", label: "HDMI 1", device: null, connected: false },
    { id: "com.webos.app.hdmi3", label: "HDMI 3", device: "Streamer", connected: true },
  ];

  it("puts plugged-in inputs first, then the family's apps", () => {
    const s = { ...EMPTY_STATE, apps: list, inputs };
    expect(shortcuts(s, 8).map((t) => t.id)).toEqual([
      "com.webos.app.hdmi3",
      "netflix",
      "youtube.leanback.v4",
      "com.webos.app.browser",
    ]);
  });

  it("shows every input until the TV has said what is plugged in", () => {
    const s = { ...EMPTY_STATE, apps: list };
    expect(shortcuts(s, 2).map((t) => t.id)).toEqual(["com.webos.app.hdmi1", "com.webos.app.hdmi3"]);
  });

  it("keeps whatever is in front among them", () => {
    const s = { ...EMPTY_STATE, apps: list, inputs, foreground: "com.webos.app.browser" };
    expect(shortcuts(s, 2).map((t) => t.id)).toEqual(["com.webos.app.hdmi3", "com.webos.app.browser"]);
    expect(shortcuts(s, 0)).toEqual([]);
    expect(shortcuts(EMPTY_STATE, 8)).toEqual([]);
  });
});
