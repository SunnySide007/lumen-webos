// SPDX-License-Identifier: GPL-3.0-or-later
//
// Words the UI shows for the protocol's values, as LG's menus name them.
import type { PowerState, SettingKey } from "./protocol.ts";

export const SETTING_LABELS: Record<SettingKey, string> = {
  "picture.mode": "Picture mode",
  "picture.oledLight": "OLED light",
  "picture.energySaving": "Energy saving",
  "sound.mode": "Sound mode",
};

export const VALUE_LABELS: Record<string, string> = {
  vivid: "Vivid",
  standard: "Standard",
  eco: "Eco",
  "cinema-home": "Cinema Home",
  cinema: "Cinema",
  sports: "Sports",
  game: "Game",
  filmmaker: "Filmmaker",
  "expert-bright": "Expert (bright)",
  "expert-dark": "Expert (dark)",
  auto: "Auto",
  off: "Off",
  minimum: "Minimum",
  medium: "Medium",
  maximum: "Maximum",
  ai: "AI Sound",
  "clear-voice": "Clear Voice",
  music: "Music",
};

export function valueLabel(v: string | number | undefined): string {
  if (v === undefined) return "—";
  return typeof v === "number" ? String(v) : (VALUE_LABELS[v] ?? v);
}

export const POWER_LABELS: Record<PowerState, string> = {
  active: "On",
  "screen-off": "Screen off",
  screensaver: "Screensaver",
  standby: "Off",
};
