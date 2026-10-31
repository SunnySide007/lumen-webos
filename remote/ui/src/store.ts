// SPDX-License-Identifier: GPL-3.0-or-later
//
// The TV's state as the UI knows it. It changes only through pushed events
// (and the answers to apps.list, settings.get and privacy.state), never from
// what this page asked for, so two clients show the same thing.
import type {
  AppInfo,
  Event,
  InputStatus,
  PowerState,
  PrivacyState,
  SettingValues,
} from "./protocol.ts";

export interface TvState {
  power: PowerState | null;
  volume: { level: number; muted: boolean } | null;
  foreground: string | null;
  inputs: InputStatus[] | null;
  apps: AppInfo[] | null;
  settings: Partial<SettingValues>;
  privacy: PrivacyState | null;
}

export const EMPTY_STATE: TvState = {
  power: null,
  volume: null,
  foreground: null,
  inputs: null,
  apps: null,
  settings: {},
  privacy: null,
};

/** A value with listeners: the one piece of reactivity the UI uses. */
export class Store<T> {
  private value: T;
  private readonly listeners = new Set<(value: T, before: T) => void>();

  constructor(initial: T) {
    this.value = initial;
  }

  get(): T {
    return this.value;
  }

  set(next: T): void {
    if (Object.is(next, this.value)) return;
    const before = this.value;
    this.value = next;
    for (const fn of this.listeners) fn(next, before);
  }

  update(fn: (value: T) => T): void {
    this.set(fn(this.value));
  }

  /** Calls `fn` now and on every change; returns the unsubscribe. */
  subscribe(fn: (value: T, before: T) => void): () => void {
    this.listeners.add(fn);
    fn(this.value, this.value);
    return () => this.listeners.delete(fn);
  }
}

function sameInputs(a: InputStatus[] | null, b: InputStatus[]): boolean {
  return (
    a !== null &&
    a.length === b.length &&
    a.every(
      (x, i) =>
        x.id === b[i]?.id &&
        x.label === b[i]?.label &&
        x.device === b[i]?.device &&
        x.connected === b[i]?.connected,
    )
  );
}

/** Folds a pushed event into the state; returns the same object when nothing
 * changed, so listeners aren't woken for an `alive` or a repeat. */
export function applyEvent(s: TvState, e: Event): TvState {
  switch (e.event) {
    case "power":
      return s.power === e.data.state ? s : { ...s, power: e.data.state };
    case "volume":
      return s.volume?.level === e.data.level && s.volume.muted === e.data.muted
        ? s
        : { ...s, volume: { level: e.data.level, muted: e.data.muted } };
    case "foreground":
      return s.foreground === e.data.id ? s : { ...s, foreground: e.data.id };
    case "inputs":
      return sameInputs(s.inputs, e.data.inputs) ? s : { ...s, inputs: e.data.inputs };
    case "settings": {
      const changed = Object.entries(e.data.values).some(
        ([k, v]) => s.settings[k as keyof SettingValues] !== v,
      );
      return changed ? { ...s, settings: { ...s.settings, ...e.data.values } } : s;
    }
    case "hello":
    case "alive":
      return s;
  }
}

/** The 8 colours the shell gives apps that bring none, by position in the
 * list (lumen-core's style::TILE_PALETTE). */
export const TILE_PALETTE = [
  "#c43a3a",
  "#d6782e",
  "#be982c",
  "#409c5c",
  "#2c8ca0",
  "#3a68c4",
  "#7052be",
  "#aa468c",
] as const;

/** The ambient colour when nothing in front has one. */
export const DEFAULT_AMBIENT = "#3a4a9c";

export interface Tile extends AppInfo {
  color: string;
  inFront: boolean;
  /** For inputs: whether something is plugged in; null if unknown. */
  connected: boolean | null;
  /** For inputs: what CEC says is plugged in. */
  device: string | null;
}

/** The Apps and inputs grid: apps.list's entries, with each input's
 * connection from the `inputs` event and a colour for every tile. */
export function tiles(s: TvState): Tile[] {
  const byId = new Map((s.inputs ?? []).map((i) => [i.id, i]));
  return (s.apps ?? []).map((app, i) => {
    const input = app.kind === "input" ? byId.get(app.id) : undefined;
    return {
      ...app,
      color: app.iconColor ?? TILE_PALETTE[i % TILE_PALETTE.length] ?? DEFAULT_AMBIENT,
      inFront: app.id === s.foreground,
      connected: input ? input.connected : null,
      device: input?.device ?? null,
    };
  });
}

/** The app or input in front, if apps.list knows it. */
export function frontApp(s: TvState): Tile | null {
  return tiles(s).find((t) => t.inFront) ?? null;
}

/** The page's ambient colour: the colour of whatever is in front. */
export function ambientColor(s: TvState): string {
  return frontApp(s)?.color ?? DEFAULT_AMBIENT;
}

/** A display name for the app in front, even when apps.list hasn't come. */
export function frontTitle(s: TvState): string | null {
  if (s.foreground === null) return null;
  return frontApp(s)?.title ?? s.foreground;
}
