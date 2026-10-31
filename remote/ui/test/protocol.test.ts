// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  KEY_NAMES,
  MAX_LINE_BYTES,
  SETTINGS,
  SETTING_KEYS,
  VERBS,
  checkArgs,
  checkResult,
  parseRequest,
  parseServerLine,
  splitLines,
} from "../src/protocol.ts";
import type { Verb } from "../src/protocol.ts";

/** One good set of arguments per verb. */
const GOOD: Record<Verb, unknown> = {
  "power.off": {},
  "screen.off": {},
  "screen.on": {},
  "volume.up": {},
  "volume.down": {},
  "volume.set": { level: 20 },
  "mute.set": { muted: true },
  "apps.list": {},
  launch: { id: "com.webos.app.hdmi1" },
  key: { name: "ok" },
  "text.insert": { text: "héllo 👋" },
  "text.delete": { count: 3 },
  "text.enter": {},
  "lumen.open": {},
  "ambient.start": {},
  "privacy.state": {},
  "settings.get": { keys: ["picture.mode"] },
  "settings.set": { key: "picture.oledLight", value: 80 },
  toast: { text: "Volume 20" },
};

/** Arguments each verb must refuse. */
const BAD: Partial<Record<Verb, unknown[]>> = {
  "power.off": [{ now: true }],
  "volume.set": [{}, { level: -1 }, { level: 101 }, { level: 2.5 }, { level: "20" }, { level: 20, extra: 1 }],
  "mute.set": [{}, { muted: "yes" }, { muted: 1 }],
  launch: [{}, { id: "" }, { id: "../etc/passwd" }, { id: "a b" }, { id: "x".repeat(129) }, { id: 5 }],
  key: [{ name: "power" }, { name: "OK" }, {}],
  "text.insert": [{ text: "" }, { text: "a\nb" }, { text: "\t" }, { text: "x".repeat(257) }, { text: "\ud800" }, { text: 1 }],
  "text.delete": [{ count: 0 }, { count: 257 }, {}, { count: 1.5 }],
  "settings.get": [{ keys: [] }, { keys: ["picture.mode", "nope"] }, { keys: "picture.mode" }],
  "settings.set": [
    { key: "picture.mode", value: "loud" },
    { key: "picture.oledLight", value: 101 },
    { key: "picture.oledLight", value: "80" },
    { key: "backlight", value: 1 },
    { key: "picture.mode" },
  ],
  toast: [{ text: "x".repeat(121) }, { text: "" }],
};

describe("verbs", () => {
  it("are exactly remote.md's table", () => {
    expect(VERBS.sort()).toEqual(
      [
        "power.off",
        "screen.off",
        "screen.on",
        "volume.up",
        "volume.down",
        "volume.set",
        "mute.set",
        "apps.list",
        "launch",
        "key",
        "text.insert",
        "text.enter",
        "text.delete",
        "lumen.open",
        "ambient.start",
        "privacy.state",
        "settings.get",
        "settings.set",
        "toast",
      ].sort(),
    );
  });

  it.each(VERBS)("%s takes its good arguments", (verb) => {
    expect(checkArgs(verb, GOOD[verb])).toBeNull();
  });

  it.each(Object.entries(BAD).flatMap(([verb, list]) => list.map((args) => [verb, args] as const)))(
    "%s refuses %j",
    (verb, args) => {
      expect(checkArgs(verb as Verb, args)).toEqual(expect.any(String));
    },
  );

  it("takes every key name and every value of every setting", () => {
    for (const name of KEY_NAMES) expect(checkArgs("key", { name })).toBeNull();
    for (const key of SETTING_KEYS) {
      const spec = SETTINGS[key];
      const values: unknown[] = "values" in spec ? [...spec.values] : [spec.min, spec.max];
      for (const value of values) expect(checkArgs("settings.set", { key, value })).toBeNull();
    }
  });
});

describe("parseRequest", () => {
  it("reads a request, with or without args", () => {
    expect(parseRequest('{"id":1,"verb":"volume.set","args":{"level":7}}')).toEqual({
      ok: true,
      request: { id: 1, verb: "volume.set", args: { level: 7 } },
    });
    expect(parseRequest('{"id":2,"verb":"volume.up"}')).toEqual({
      ok: true,
      request: { id: 2, verb: "volume.up", args: {} },
    });
  });

  it.each([
    ["not json", null, "bad-line"],
    ["[1,2]", null, "bad-line"],
    ['{"verb":"volume.up"}', null, "bad-line"],
    ['{"id":0,"verb":"volume.up"}', null, "bad-line"],
    ['{"id":1.5,"verb":"volume.up"}', null, "bad-line"],
    ['{"id":3,"verb":"volume.up","sudo":true}', 3, "bad-line"],
    ['{"id":4,"verb":"rm -rf"}', 4, "unknown-verb"],
    ['{"id":5,"verb":"volume.set","args":[50]}', 5, "bad-args"],
    ['{"id":6,"verb":"volume.set","args":{"level":500}}', 6, "bad-args"],
  ])("refuses %s", (line, id, code) => {
    expect(parseRequest(line)).toMatchObject({ ok: false, id, error: { code } });
  });

  it("refuses a line over the limit before parsing it", () => {
    const line = `{"id":1,"verb":"toast","args":{"text":"${"é".repeat(MAX_LINE_BYTES / 2)}"}}`;
    expect(parseRequest(line)).toMatchObject({ ok: false, id: null, error: { code: "bad-line" } });
  });

  it("doesn't echo a long verb back whole", () => {
    const r = parseRequest(JSON.stringify({ id: 1, verb: "v".repeat(1000) }));
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.message.length).toBeLessThan(80);
  });
});

describe("parseServerLine", () => {
  it("reads replies", () => {
    expect(parseServerLine('{"id":1,"ok":true,"result":{}}')).toEqual({ id: 1, ok: true, result: {} });
    expect(
      parseServerLine('{"id":null,"ok":false,"error":{"code":"bad-line","message":"not JSON"}}'),
    ).toEqual({ id: null, ok: false, error: { code: "bad-line", message: "not JSON" } });
  });

  it("maps an error code it doesn't know to failed", () => {
    expect(
      parseServerLine('{"id":2,"ok":false,"error":{"code":"melted","message":"hot"}}'),
    ).toMatchObject({ error: { code: "failed" } });
  });

  it("reads events and keeps hello's verbs to known ones", () => {
    expect(parseServerLine('{"event":"volume","data":{"level":12,"muted":false}}')).toEqual({
      event: "volume",
      data: { level: 12, muted: false },
    });
    expect(
      parseServerLine('{"event":"hello","data":{"protocol":1,"server":"x","verbs":["key","fly"]}}'),
    ).toEqual({ event: "hello", data: { protocol: 1, server: "x", verbs: ["key"] } });
  });

  it.each([
    "nope",
    "42",
    '{"event":"weather","data":{}}',
    '{"event":"volume","data":{"level":101,"muted":false}}',
    '{"event":"power","data":{"state":"exploded"}}',
    '{"event":"settings","data":{"values":{"picture.mode":"loud"}}}',
    '{"event":"inputs","data":{"inputs":[{"id":"x"}]}}',
    '{"id":1,"ok":"yes"}',
  ])("ignores %s", (line) => {
    expect(parseServerLine(line)).toBeNull();
  });
});

describe("checkResult", () => {
  it("checks the shapes of the query verbs' results", () => {
    const apps = { apps: [{ id: "netflix", title: "Netflix", kind: "app", iconColor: "#e50914" }] };
    expect(checkResult("apps.list", apps)).toEqual(apps);
    expect(checkResult("apps.list", { apps: [{ id: "netflix" }] })).toBeNull();
    expect(checkResult("apps.list", { apps: [{ ...apps.apps[0], iconColor: "red" }] })).toBeNull();
    expect(checkResult("settings.get", { values: { "picture.oledLight": 80 } })).not.toBeNull();
    expect(checkResult("settings.get", { values: { "picture.oledLight": 800 } })).toBeNull();
    expect(
      checkResult("privacy.state", {
        hook: { installed: true, log: ["a"] },
        port11111: { rejected: true },
        jobs: [{ name: "rdxd", running: false }],
        guard: null,
      }),
    ).not.toBeNull();
    expect(checkResult("privacy.state", { hook: {} })).toBeNull();
    expect(checkResult("volume.up", {})).toEqual({});
    expect(checkResult("volume.up", null)).toBeNull();
  });
});

describe("splitLines", () => {
  it("splits a message into its non-blank lines", () => {
    expect(splitLines('{"a":1}\n\n{"b":2}\n')).toEqual(['{"a":1}', '{"b":2}']);
  });
});
