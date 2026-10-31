// SPDX-License-Identifier: GPL-3.0-or-later
//
// A fake TV that answers the Remote protocol as lumen-ctl will: every request
// is checked by protocol.ts's parseRequest (types and ranges), changes the
// fake state, and the change goes out as an event to every client, whoever
// asked. Nothing here touches a TV, luna or root.
import {
  PROTOCOL_VERSION,
  SETTING_KEYS,
  VERBS,
  parseRequest,
} from "../../ui/src/protocol.ts";
import type {
  ErrorCode,
  Event,
  InputStatus,
  PowerState,
  PrivacyState,
  Reply,
  Request,
  SettingValues,
  Verb,
} from "../../ui/src/protocol.ts";
import { APPS, LUMEN_ID, initialInputs, initialSettings, privacyState } from "./fixtures.ts";

export interface TvOptions {
  /** false: the TV has no route for keys (remote.md, risk 1), so `key` is
   * left out of hello and refused as unavailable. */
  keys?: boolean;
  /** true: Lumen Guard exists and has counts. */
  guard?: boolean;
  /** Runs `fn` after `ms`, as LG's services answer a moment later. */
  schedule?: (ms: number, fn: () => void) => void;
  log?: (line: string) => void;
  now?: () => Date;
}

/** How long the fake TV takes to bring an app to the front. */
export const LAUNCH_MS = 300;

class Refusal extends Error {
  readonly code: ErrorCode;
  constructor(code: ErrorCode, message: string) {
    super(message);
    this.code = code;
  }
}

export class FakeTv {
  power: PowerState = "active";
  volume = 14;
  muted = false;
  foreground: string | null = "netflix";
  readonly inputs: InputStatus[] = initialInputs();
  readonly settings: SettingValues = initialSettings();
  /** What text.insert has typed into the TV's focused field. */
  text = "";
  /** Called after power.off has answered: the server drops every client. */
  onPowerOff: (() => void) | null = null;

  private readonly keys: boolean;
  private readonly privacy: PrivacyState;
  private readonly schedule: (ms: number, fn: () => void) => void;
  private readonly log: (line: string) => void;
  private readonly listeners = new Set<(e: Event) => void>();

  constructor(opts: TvOptions = {}) {
    this.keys = opts.keys ?? true;
    this.schedule = opts.schedule ?? ((ms, fn) => void setTimeout(fn, ms));
    this.log = opts.log ?? (() => {});
    const now = (opts.now ?? (() => new Date()))();
    this.privacy = privacyState(new Date(now.getTime() - 3 * 3600_000));
    if (opts.guard) {
      this.privacy.guard = {
        since: new Date(now.getTime() - 3 * 3600_000).toISOString(),
        lookups: 4_812,
        blocked: 0,
      };
    }
  }

  get verbs(): Verb[] {
    return VERBS.filter((v) => this.keys || v !== "key");
  }

  onEvent(fn: (e: Event) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  hello(server: string): Event {
    return { event: "hello", data: { protocol: PROTOCOL_VERSION, server, verbs: this.verbs } };
  }

  /** The current value of every watched state, as a new client gets it. */
  snapshot(): Event[] {
    return [
      { event: "power", data: { state: this.power } },
      { event: "volume", data: { level: this.volume, muted: this.muted } },
      { event: "foreground", data: { id: this.foreground } },
      { event: "inputs", data: { inputs: this.inputs.map((i) => ({ ...i })) } },
      { event: "settings", data: { values: { ...this.settings } } },
    ];
  }

  /** Answers one request line. */
  handle(line: string): Reply {
    const parsed = parseRequest(line);
    if (!parsed.ok) {
      this.log(`refused: ${parsed.error.code}: ${parsed.error.message}`);
      return { id: parsed.id, ok: false, error: parsed.error };
    }
    const { request } = parsed;
    try {
      const result = this.run(request);
      this.log(`${request.verb} ${JSON.stringify(request.args)}: ok`);
      return { id: request.id, ok: true, result };
    } catch (err) {
      if (!(err instanceof Refusal)) throw err;
      this.log(`${request.verb} ${JSON.stringify(request.args)}: ${err.code}: ${err.message}`);
      return { id: request.id, ok: false, error: { code: err.code, message: err.message } };
    }
  }

  private run(r: Request): unknown {
    switch (r.verb) {
      case "power.off":
        this.setPower("standby");
        this.schedule(0, () => this.onPowerOff?.());
        return {};
      case "screen.off":
        this.setPower("screen-off");
        return {};
      case "screen.on":
        this.setPower("active");
        return {};
      case "volume.up":
        this.setVolume(Math.min(100, this.volume + 1), false);
        return {};
      case "volume.down":
        this.setVolume(Math.max(0, this.volume - 1), false);
        return {};
      case "volume.set":
        this.setVolume(r.args.level, this.muted);
        return {};
      case "mute.set":
        this.setVolume(this.volume, r.args.muted);
        return {};
      case "apps.list":
        return { apps: APPS.map((a) => ({ ...a })) };
      case "launch": {
        const id = r.args.id;
        if (!APPS.some((a) => a.id === id)) throw new Refusal("not-found", `no app or input ${id}`);
        this.schedule(LAUNCH_MS, () => this.setForeground(id));
        return {};
      }
      case "key":
        if (!this.keys) throw new Refusal("unavailable", "this TV has no route for keys");
        this.log(`key ${r.args.name}`);
        if (this.power === "screensaver") this.setPower("active");
        return {};
      case "text.insert":
        this.text += r.args.text;
        this.log(`text field: ${JSON.stringify(this.text)}`);
        return {};
      case "text.delete":
        this.text = [...this.text].slice(0, -r.args.count).join("");
        this.log(`text field: ${JSON.stringify(this.text)}`);
        return {};
      case "text.enter":
        this.log(`text entered: ${JSON.stringify(this.text)}`);
        this.text = "";
        return {};
      case "lumen.open":
        this.schedule(LAUNCH_MS, () => this.setForeground(LUMEN_ID));
        return {};
      case "ambient.start":
        this.schedule(LAUNCH_MS, () => this.setPower("screensaver"));
        return {};
      case "privacy.state":
        return structuredClone(this.privacy);
      case "settings.get": {
        const keys = r.args.keys ?? SETTING_KEYS;
        return { values: Object.fromEntries(keys.map((k) => [k, this.settings[k]])) };
      }
      case "settings.set": {
        const { key, value } = r.args;
        if (this.settings[key] !== value) {
          (this.settings as Record<string, unknown>)[key] = value;
          this.emit({ event: "settings", data: { values: { [key]: value } } });
        }
        return {};
      }
      case "toast":
        this.log(`toast on the TV: ${JSON.stringify(r.args.text)}`);
        return {};
    }
  }

  // The changes below also come from outside: the demo script and the
  // console stand in for the Magic Remote and LG's own menus.

  setPower(state: PowerState): void {
    if (this.power === state) return;
    this.power = state;
    this.emit({ event: "power", data: { state } });
  }

  setVolume(level: number, muted: boolean): void {
    if (this.volume === level && this.muted === muted) return;
    this.volume = level;
    this.muted = muted;
    this.emit({ event: "volume", data: { level, muted } });
  }

  setForeground(id: string | null): void {
    if (this.foreground === id) return;
    this.foreground = id;
    this.emit({ event: "foreground", data: { id } });
  }

  setInput(index: number, connected: boolean, device: string | null): void {
    const input = this.inputs[index];
    if (!input) return;
    input.connected = connected;
    input.device = connected ? device : null;
    this.emit({ event: "inputs", data: { inputs: this.inputs.map((i) => ({ ...i })) } });
  }

  private emit(e: Event): void {
    for (const fn of this.listeners) fn(e);
  }
}
