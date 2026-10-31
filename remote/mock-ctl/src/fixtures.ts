// SPDX-License-Identifier: GPL-3.0-or-later
//
// The fake TV's contents: made up, shaped like a B9's. App ids are webOS's
// usual ones so the UI meets realistic strings; the colours are the apps'
// own brand colours, which is what LG's launch points carry as iconColor.
import type { AppInfo, InputStatus, PrivacyState, SettingValues } from "../../ui/src/protocol.ts";

export const LUMEN_ID = "io.github.sunnyside007.lumen.shell";

export const APPS: AppInfo[] = [
  { id: "com.webos.app.hdmi1", title: "HDMI 1", kind: "input", iconColor: null },
  { id: "com.webos.app.hdmi2", title: "HDMI 2", kind: "input", iconColor: null },
  { id: "com.webos.app.hdmi3", title: "HDMI 3", kind: "input", iconColor: null },
  { id: "com.webos.app.hdmi4", title: "HDMI 4", kind: "input", iconColor: null },
  { id: "netflix", title: "Netflix", kind: "app", iconColor: "#e50914" },
  { id: "youtube.leanback.v4", title: "YouTube", kind: "app", iconColor: "#ff0033" },
  { id: "amazon", title: "Prime Video", kind: "app", iconColor: "#1a98ff" },
  { id: "cdp-30", title: "Plex", kind: "app", iconColor: "#e5a00d" },
  { id: "com.disney.disneyplus-prod", title: "Disney+", kind: "app", iconColor: "#113ccf" },
  { id: "spotify-beehive", title: "Spotify", kind: "app", iconColor: "#1db954" },
  { id: LUMEN_ID, title: "Lumen", kind: "app", iconColor: "#7c8cff" },
  { id: "org.webosbrew.hbchannel", title: "Homebrew Channel", kind: "app", iconColor: null },
  { id: "com.webos.app.livetv", title: "Live TV", kind: "app", iconColor: null },
  { id: "com.webos.app.browser", title: "Web Browser", kind: "app", iconColor: null },
  { id: "com.webos.app.photovideo", title: "Photo & Video", kind: "app", iconColor: null },
  { id: "com.palm.app.settings", title: "Settings", kind: "app", iconColor: null },
];

export function initialInputs(): InputStatus[] {
  return [
    { id: "com.webos.app.hdmi1", label: "HDMI 1", device: "Google TV Streamer", connected: true },
    { id: "com.webos.app.hdmi2", label: "HDMI 2", device: null, connected: false },
    { id: "com.webos.app.hdmi3", label: "HDMI 3", device: null, connected: false },
    { id: "com.webos.app.hdmi4", label: "HDMI 4", device: null, connected: false },
  ];
}

export function initialSettings(): SettingValues {
  return {
    "picture.mode": "cinema",
    "picture.oledLight": 80,
    "picture.energySaving": "off",
    "sound.mode": "standard",
  };
}

/** What lumen-privacy logs at a boot (profiles/privacy/lumen-privacy). */
export function privacyState(bootedAt: Date): PrivacyState {
  const at = (s: number) => new Date(bootedAt.getTime() + s * 1000).toUTCString().replace("GMT", "UTC");
  return {
    hook: {
      installed: true,
      log: [
        `${at(34)} lumen-privacy: start`,
        "tcp 11111 rejected from the LAN",
        `${at(34)} stopped crashreportd`,
        `${at(34)} stopped rdxd`,
        `${at(44)} stopped uploadd`,
        `${at(44)} stopped remotelogger`,
        `${at(634)} lumen-privacy: done`,
      ],
    },
    port11111: { rejected: true },
    jobs: ["crashreportd", "rdxd", "uploadd", "remotelogger"].map((name) => ({ name, running: false })),
    guard: null,
  };
}
