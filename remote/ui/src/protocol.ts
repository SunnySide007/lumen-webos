// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Lumen Remote protocol: what lumen-ctl and its clients say to each other
// (docs/specs/remote.md, "Verbs"). This file is the source of truth: the web
// UI and the mock import it, and lumen-ctl (R2, in Rust) is written against
// it. Where remote.md leaves a shape open, this file picks the simplest one;
// remote/README.md lists those choices.
//
// Framing: one JSON object per line, in each direction. Over SSH the lines end
// in "\n". Over a WebSocket each text message carries one line; a trailing
// "\n" is allowed, and a receiver splits a message on "\n" and skips blank
// lines, so lumen-remote can relay lumen-ctl's lines as they come.
//
//   client  {"id": 7, "verb": "volume.set", "args": {"level": 20}}
//   server  {"id": 7, "ok": true, "result": {}}
//           {"id": 7, "ok": false, "error": {"code": "bad-args", "message": "…"}}
//           {"event": "volume", "data": {"level": 20, "muted": false}}
//
// The server speaks first: `hello`, with the protocol version and the verbs
// it accepts, then one event per watched state (power, volume, foreground,
// inputs, settings) with its current value, as a luna subscription's first
// reply does. After that it pushes an event whenever that state changes,
// whoever changed it, and `alive` every ALIVE_INTERVAL_MS, so a client can
// tell a quiet TV from a dead link. Actions reply with an empty result; the
// state they change arrives as events.
//
// This file has no imports and only erasable TypeScript, so Node runs it as
// is (the mock) and Vite bundles it (the UI).

/** Bumped only for a change old clients can't read; new verbs, events and
 * fields are added without a bump, and receivers ignore what they don't know. */
export const PROTOCOL_VERSION = 1;

/** The longest line either side accepts, in bytes of UTF-8. */
export const MAX_LINE_BYTES = 65_536;

/** How often the server sends `alive`. */
export const ALIVE_INTERVAL_MS = 10_000;

/** Request ids are chosen by the client, unique among its pending requests. */
export const MAX_REQUEST_ID = 2_147_483_647;

// ---------------------------------------------------------------- values

/** Keys `key` can press: the Magic Remote's, as far as the UI needs them. */
export const KEY_NAMES = [
  "up",
  "down",
  "left",
  "right",
  "ok",
  "back",
  "home",
  "menu",
  "play",
  "pause",
  "stop",
  "rewind",
  "fastforward",
] as const;
export type KeyName = (typeof KEY_NAMES)[number];

/** LG's power states, from tvpower's getPowerState, in Lumen's own words. */
export const POWER_STATES = ["active", "screen-off", "screensaver", "standby"] as const;
export type PowerState = (typeof POWER_STATES)[number];

interface EnumSetting {
  readonly kind: "enum";
  readonly values: readonly string[];
}
interface IntSetting {
  readonly kind: "int";
  readonly min: number;
  readonly max: number;
}

/**
 * The picture and sound settings Remote may read and write (D7), with every
 * value it may write. The names are Lumen's; lumen-ctl maps them to LG's
 * settingsservice keys once R0 has read those on the B9.
 */
export const SETTINGS = {
  "picture.mode": {
    kind: "enum",
    values: [
      "vivid",
      "standard",
      "eco",
      "cinema-home",
      "cinema",
      "sports",
      "game",
      "filmmaker",
      "expert-bright",
      "expert-dark",
    ],
  },
  "picture.oledLight": { kind: "int", min: 0, max: 100 },
  "picture.energySaving": {
    kind: "enum",
    values: ["auto", "off", "minimum", "medium", "maximum"],
  },
  "sound.mode": {
    kind: "enum",
    values: ["ai", "standard", "cinema", "clear-voice", "sports", "music", "game"],
  },
} as const satisfies Record<string, EnumSetting | IntSetting>;

export type SettingKey = keyof typeof SETTINGS;
export const SETTING_KEYS = Object.keys(SETTINGS) as SettingKey[];
export type SettingValue<K extends SettingKey> = (typeof SETTINGS)[K] extends {
  values: readonly (infer V)[];
}
  ? V
  : number;
export type SettingValues = { [K in SettingKey]: SettingValue<K> };
export type SettingChange = { [K in SettingKey]: { key: K; value: SettingValue<K> } }[SettingKey];

/** One entry of `apps.list`: an app, or an HDMI input (launched the same way). */
export interface AppInfo {
  id: string;
  title: string;
  kind: "app" | "input";
  /** "#rrggbb" from LG's launch point, or null; the UI's ambient colour. */
  iconColor: string | null;
}

/** One input in the `inputs` event. `id` is what `launch` takes. */
export interface InputStatus {
  id: string;
  /** "HDMI 1". */
  label: string;
  /** What CEC says is plugged in, if anything. */
  device: string | null;
  connected: boolean;
}

/** `privacy.state`: what Lumen's privacy work did on this boot (privacy.md). */
export interface PrivacyState {
  /** The boot hook, /var/lib/webosbrew/init.d/lumen-privacy, and its log. */
  hook: { installed: boolean; log: string[] };
  /** The INPUT rule rejecting tcp 11111 from the LAN. */
  port11111: { rejected: boolean };
  /** LG's diagnostics jobs the hook stops. */
  jobs: { name: string; running: boolean }[];
  /** Lumen Guard's counts since `since` (ISO 8601), or null before Guard exists. */
  guard: { since: string; lookups: number; blocked: number } | null;
}

// ---------------------------------------------------------------- verbs

type NoArgs = Record<string, never>;
type Done = Record<string, never>;

/** Every verb, its arguments and its result. */
export interface Verbs {
  "power.off": { args: NoArgs; result: Done };
  "screen.off": { args: NoArgs; result: Done };
  "screen.on": { args: NoArgs; result: Done };
  "volume.up": { args: NoArgs; result: Done };
  "volume.down": { args: NoArgs; result: Done };
  "volume.set": { args: { level: number }; result: Done };
  "mute.set": { args: { muted: boolean }; result: Done };
  "apps.list": { args: NoArgs; result: { apps: AppInfo[] } };
  launch: { args: { id: string }; result: Done };
  key: { args: { name: KeyName }; result: Done };
  "text.insert": { args: { text: string }; result: Done };
  "text.delete": { args: { count: number }; result: Done };
  "text.enter": { args: NoArgs; result: Done };
  "lumen.open": { args: NoArgs; result: Done };
  "ambient.start": { args: NoArgs; result: Done };
  "privacy.state": { args: NoArgs; result: PrivacyState };
  "settings.get": { args: { keys?: SettingKey[] }; result: { values: Partial<SettingValues> } };
  "settings.set": { args: SettingChange; result: Done };
  toast: { args: { text: string }; result: Done };
}
export type Verb = keyof Verbs;
export type ArgsOf<V extends Verb> = Verbs[V]["args"];
export type ResultOf<V extends Verb> = Verbs[V]["result"];

export type Request = { [V in Verb]: { id: number; verb: V; args: ArgsOf<V> } }[Verb];

/** The longest text `text.insert` takes at once, in code points. */
export const MAX_INSERT = 256;
/** The most characters `text.delete` removes at once. */
export const MAX_DELETE = 256;
/** The longest `toast`, in code points. */
export const MAX_TOAST = 120;
/** App and input ids: webOS's reverse-DNS ids and short ones like "netflix". */
const APP_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const COLOR = /^#[0-9a-fA-F]{6}$/;
// Control characters, C0 and C1: text.insert can't carry a newline or a tab.
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

type Obj = Record<string, unknown>;
/** Checks a verb's arguments; returns what is wrong, or null. */
type ArgCheck = (args: Obj) => string | null;

function isObject(v: unknown): v is Obj {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function isInt(v: unknown, min: number, max: number): v is number {
  return typeof v === "number" && Number.isInteger(v) && v >= min && v <= max;
}

function onlyKeys(args: Obj, allowed: readonly string[]): string | null {
  for (const k of Object.keys(args)) {
    if (!allowed.includes(k)) return `unexpected argument ${quote(k)}`;
  }
  return null;
}

function codePoints(s: string): number {
  return [...s].length;
}

/** Checks a line of text a person typed; returns what is wrong, or null. */
function checkText(name: string, v: unknown, max: number): string | null {
  if (typeof v !== "string") return `${name} must be a string`;
  const n = codePoints(v);
  if (n < 1 || n > max) return `${name} must be 1 to ${max} characters`;
  if (CONTROL.test(v)) return `${name} must not contain control characters`;
  if (!v.isWellFormed()) return `${name} must be well-formed Unicode`;
  return null;
}

const none: ArgCheck = (a) => onlyKeys(a, []);

/** Checks a setting and its value; returns what is wrong, or null. */
export function checkSetting(key: unknown, value: unknown): string | null {
  if (typeof key !== "string" || !Object.hasOwn(SETTINGS, key)) {
    return `unknown setting ${quote(key)}`;
  }
  const spec: EnumSetting | IntSetting = SETTINGS[key as SettingKey];
  if (spec.kind === "int") {
    return isInt(value, spec.min, spec.max)
      ? null
      : `${key} must be an integer from ${spec.min} to ${spec.max}`;
  }
  return typeof value === "string" && spec.values.includes(value)
    ? null
    : `${key} must be one of ${spec.values.join(", ")}`;
}

/** Argument checks, one per verb; its keys are the verb list. lumen-ctl
 * checks the same types and ranges. */
const ARG_CHECKS: { [V in Verb]: ArgCheck } = {
  "power.off": none,
  "screen.off": none,
  "screen.on": none,
  "volume.up": none,
  "volume.down": none,
  "volume.set": (a) =>
    onlyKeys(a, ["level"]) ??
    (isInt(a.level, 0, 100) ? null : "level must be an integer from 0 to 100"),
  "mute.set": (a) =>
    onlyKeys(a, ["muted"]) ?? (typeof a.muted === "boolean" ? null : "muted must be true or false"),
  "apps.list": none,
  launch: (a) =>
    onlyKeys(a, ["id"]) ??
    (typeof a.id === "string" && APP_ID.test(a.id) ? null : "id must be an app or input id"),
  key: (a) =>
    onlyKeys(a, ["name"]) ??
    (typeof a.name === "string" && (KEY_NAMES as readonly string[]).includes(a.name)
      ? null
      : `name must be one of ${KEY_NAMES.join(", ")}`),
  "text.insert": (a) => onlyKeys(a, ["text"]) ?? checkText("text", a.text, MAX_INSERT),
  "text.delete": (a) =>
    onlyKeys(a, ["count"]) ??
    (isInt(a.count, 1, MAX_DELETE) ? null : `count must be an integer from 1 to ${MAX_DELETE}`),
  "text.enter": none,
  "lumen.open": none,
  "ambient.start": none,
  "privacy.state": none,
  "settings.get": (a) => {
    const extra = onlyKeys(a, ["keys"]);
    if (extra) return extra;
    if (a.keys === undefined) return null;
    if (!Array.isArray(a.keys) || a.keys.length < 1 || a.keys.length > SETTING_KEYS.length) {
      return `keys must be a list of 1 to ${SETTING_KEYS.length} settings`;
    }
    for (const k of a.keys as unknown[]) {
      if (typeof k !== "string" || !Object.hasOwn(SETTINGS, k)) return `unknown setting ${quote(k)}`;
    }
    return null;
  },
  "settings.set": (a) => onlyKeys(a, ["key", "value"]) ?? checkSetting(a.key, a.value),
  toast: (a) => onlyKeys(a, ["text"]) ?? checkText("text", a.text, MAX_TOAST),
};

export const VERBS = Object.keys(ARG_CHECKS) as Verb[];

export function isVerb(v: unknown): v is Verb {
  return typeof v === "string" && Object.hasOwn(ARG_CHECKS, v);
}

/** Checks a verb's arguments as lumen-ctl will; returns what is wrong, or null. */
export function checkArgs(verb: Verb, args: unknown): string | null {
  if (!isObject(args)) return "args must be an object";
  return ARG_CHECKS[verb](args);
}

// ---------------------------------------------------------------- errors

/**
 * - `bad-line`: not JSON, not an object, too long, or no usable id (the
 *   reply's id is then null)
 * - `unknown-verb`: not a verb this server accepts
 * - `bad-args`: an argument of the wrong type or out of range
 * - `not-found`: no app, input or setting by that name on the TV
 * - `unavailable`: a known verb the TV can't do now (no route for keys, off)
 * - `failed`: LG's service refused or didn't answer
 */
export const ERROR_CODES = [
  "bad-line",
  "unknown-verb",
  "bad-args",
  "not-found",
  "unavailable",
  "failed",
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];
export interface ErrorInfo {
  code: ErrorCode;
  message: string;
}

export type ReplyOk = { id: number; ok: true; result: unknown };
export type ReplyErr = { id: number | null; ok: false; error: ErrorInfo };
export type Reply = ReplyOk | ReplyErr;

// ---------------------------------------------------------------- events

export interface Events {
  /** The first line on every connection. */
  hello: { protocol: number; server: string; verbs: Verb[] };
  power: { state: PowerState };
  volume: { level: number; muted: boolean };
  /** The app or input in front; null when LG reports none. */
  foreground: { id: string | null };
  inputs: { inputs: InputStatus[] };
  /** The settings that changed, or all of them on connecting. */
  settings: { values: Partial<SettingValues> };
  alive: NoArgs;
}
export type EventName = keyof Events;
export type Event = { [E in EventName]: { event: E; data: Events[E] } }[EventName];

export type ServerMessage = Reply | Event;

// ---------------------------------------------------------------- reading lines

export type ParsedRequest =
  | { ok: true; request: Request }
  | { ok: false; id: number | null; error: ErrorInfo };

export function utf8Length(s: string): number {
  return new TextEncoder().encode(s).length;
}

function quote(v: unknown): string {
  const s = typeof v === "string" ? v : String(JSON.stringify(v));
  return JSON.stringify(s.length > 40 ? `${s.slice(0, 40)}…` : s);
}

function refuse(id: number | null, code: ErrorCode, message: string): ParsedRequest {
  return { ok: false, id, error: { code, message } };
}

/** Reads one request line as lumen-ctl will: every field typed and in range. */
export function parseRequest(line: string): ParsedRequest {
  if (utf8Length(line) > MAX_LINE_BYTES) {
    return refuse(null, "bad-line", `line is longer than ${MAX_LINE_BYTES} bytes`);
  }
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    return refuse(null, "bad-line", "not JSON");
  }
  if (!isObject(value)) return refuse(null, "bad-line", "not a JSON object");
  if (!isInt(value.id, 1, MAX_REQUEST_ID)) {
    return refuse(null, "bad-line", `id must be an integer from 1 to ${MAX_REQUEST_ID}`);
  }
  const id = value.id;
  const extra = onlyKeys(value, ["id", "verb", "args"]);
  if (extra) return refuse(id, "bad-line", extra.replace("argument", "field"));
  if (!isVerb(value.verb)) return refuse(id, "unknown-verb", `unknown verb ${quote(value.verb)}`);
  const args = value.args ?? {};
  const problem = checkArgs(value.verb, args);
  if (problem) return refuse(id, "bad-args", problem);
  return { ok: true, request: { id, verb: value.verb, args } as Request };
}

function isAppInfo(v: unknown): v is AppInfo {
  return (
    isObject(v) &&
    typeof v.id === "string" &&
    typeof v.title === "string" &&
    (v.kind === "app" || v.kind === "input") &&
    (v.iconColor === null || (typeof v.iconColor === "string" && COLOR.test(v.iconColor)))
  );
}

function isInputStatus(v: unknown): v is InputStatus {
  return (
    isObject(v) &&
    typeof v.id === "string" &&
    typeof v.label === "string" &&
    (v.device === null || typeof v.device === "string") &&
    typeof v.connected === "boolean"
  );
}

function isSettingValues(v: unknown): v is Partial<SettingValues> {
  return isObject(v) && Object.entries(v).every(([k, value]) => checkSetting(k, value) === null);
}

function isCount(v: unknown): v is number {
  return isInt(v, 0, Number.MAX_SAFE_INTEGER);
}

function isPrivacyState(v: unknown): v is PrivacyState {
  if (!isObject(v)) return false;
  const { hook, port11111, jobs, guard } = v;
  return (
    isObject(hook) &&
    typeof hook.installed === "boolean" &&
    Array.isArray(hook.log) &&
    hook.log.every((l) => typeof l === "string") &&
    isObject(port11111) &&
    typeof port11111.rejected === "boolean" &&
    Array.isArray(jobs) &&
    jobs.every((j) => isObject(j) && typeof j.name === "string" && typeof j.running === "boolean") &&
    (guard === null ||
      (isObject(guard) &&
        typeof guard.since === "string" &&
        isCount(guard.lookups) &&
        isCount(guard.blocked)))
  );
}

const EVENT_CHECKS: { [E in EventName]: (d: Obj) => boolean } = {
  hello: (d) =>
    isInt(d.protocol, 0, MAX_REQUEST_ID) &&
    typeof d.server === "string" &&
    Array.isArray(d.verbs) &&
    d.verbs.every((v) => typeof v === "string"),
  power: (d) => (POWER_STATES as readonly unknown[]).includes(d.state),
  volume: (d) => isInt(d.level, 0, 100) && typeof d.muted === "boolean",
  foreground: (d) => d.id === null || typeof d.id === "string",
  inputs: (d) => Array.isArray(d.inputs) && d.inputs.every(isInputStatus),
  settings: (d) => isSettingValues(d.values),
  alive: () => true,
};

/**
 * Reads one line from the server. Returns null for a line that isn't a reply
 * or a known event with the right shape; a client ignores those, so a newer
 * server can add events. `hello`'s verbs are kept to the ones this file knows.
 */
export function parseServerLine(line: string): ServerMessage | null {
  let v: unknown;
  try {
    v = JSON.parse(line);
  } catch {
    return null;
  }
  if (!isObject(v)) return null;
  if (typeof v.event === "string") {
    const name = v.event;
    if (!Object.hasOwn(EVENT_CHECKS, name) || !isObject(v.data)) return null;
    if (!EVENT_CHECKS[name as EventName](v.data)) return null;
    if (name === "hello") {
      const data = v.data as Events["hello"];
      return { event: "hello", data: { ...data, verbs: data.verbs.filter(isVerb) } };
    }
    return { event: name, data: v.data } as Event;
  }
  if (v.ok === true && isInt(v.id, 1, MAX_REQUEST_ID)) {
    return { id: v.id, ok: true, result: v.result };
  }
  if (
    v.ok === false &&
    (v.id === null || isInt(v.id, 1, MAX_REQUEST_ID)) &&
    isObject(v.error) &&
    typeof v.error.code === "string" &&
    typeof v.error.message === "string"
  ) {
    const code = (ERROR_CODES as readonly string[]).includes(v.error.code)
      ? (v.error.code as ErrorCode)
      : "failed";
    return { id: v.id, ok: false, error: { code, message: v.error.message } };
  }
  return null;
}

const RESULT_CHECKS: Partial<{ [V in Verb]: (r: Obj) => boolean }> = {
  "apps.list": (r) => Array.isArray(r.apps) && r.apps.every(isAppInfo),
  "privacy.state": isPrivacyState,
  "settings.get": (r) => isSettingValues(r.values),
};

/** Checks a reply's result against its verb; null if it has the wrong shape. */
export function checkResult<V extends Verb>(verb: V, result: unknown): ResultOf<V> | null {
  if (!isObject(result)) return null;
  const check = RESULT_CHECKS[verb];
  if (check && !check(result)) return null;
  return result as ResultOf<V>;
}

/** Splits a WebSocket message into its lines. */
export function splitLines(message: string): string[] {
  return message.split("\n").filter((l) => l.trim() !== "");
}

export function encode(message: Request | Reply | Event): string {
  return JSON.stringify(message);
}
