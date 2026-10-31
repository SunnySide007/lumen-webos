// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lumen's own line icons, drawn for this UI on a 24-unit grid. No icon font
// and nothing fetched (look-and-feel.md, principle 6: no Apple artwork).

const PATHS = {
  remote: "M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M12 9.25a2.75 2.75 0 1 0 0 5.5a2.75 2.75 0 1 0 0-5.5z",
  sound: "M4 9.5h3.2L12 5.5v13l-4.8-4H4z M15.5 9.2a4 4 0 0 1 0 5.6 M18.2 6.6a7.6 7.6 0 0 1 0 10.8",
  apps: "M5 4.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5H5a.5.5 0 0 1-.5-.5V5a.5.5 0 0 1 .5-.5z M15 4.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5h-4a.5.5 0 0 1-.5-.5V5a.5.5 0 0 1 .5-.5z M5 14.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5H5a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z M15 14.5h4a.5.5 0 0 1 .5.5v4a.5.5 0 0 1-.5.5h-4a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z",
  picture: "M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M12 3.5v17 M12 7.5h4.5 M12 11h7.5 M12 14.5h7 M12 18h4",
  lumen: "M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z M18.5 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z",
  pair: "M8 3h8a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z M10.5 17.5h3",
  up: "M6.5 14.5L12 9l5.5 5.5",
  down: "M6.5 9.5L12 15l5.5-5.5",
  left: "M14.5 6.5L9 12l5.5 5.5",
  right: "M9.5 6.5L15 12l-5.5 5.5",
  back: "M9 14.5L4.5 10 9 5.5 M4.5 10h10a5 5 0 0 1 0 10H12",
  home: "M4 11l8-6.5 8 6.5 M6 9.5V20h4.5v-5.5h3V20H18V9.5",
  menu: "M5 7h14 M5 12h14 M5 17h14",
  play: "M8 5.5v13l10.5-6.5z",
  pause: "M8.5 5.5v13 M15.5 5.5v13",
  stop: "M7 7h10v10H7z",
  rewind: "M11 6.5v11L3.5 12z M20.5 6.5v11L13 12z",
  fastforward: "M13 6.5v11l7.5-5.5z M3.5 6.5v11L11 12z",
  power: "M12 3.5v8 M6.6 6.8a7.5 7.5 0 1 0 10.8 0",
  screen: "M3.5 5h17v11h-17z M8.5 20h7 M12 16v4",
  screenOff: "M3.5 5h17v11h-17z M8.5 20h7 M12 16v4 M3 3l18 18",
  plus: "M12 5.5v13 M5.5 12h13",
  minus: "M5.5 12h13",
  refresh: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3 M19.5 4.5v4.8h-4.8",
  check: "M5.5 12.5l4 4L18.5 7.5",
  warn: "M12 4l9 16H3z M12 10v4.5 M12 17.6v.1",
  enter: "M19.5 5v6.5a3 3 0 0 1-3 3H5.5 M9.5 10.5l-4 4 4 4",
  shield: "M12 3.5l7.5 2.8v5.4c0 4.6-3.2 7.6-7.5 8.8-4.3-1.2-7.5-4.2-7.5-8.8V6.3z M8.8 12l2.3 2.3 4.3-4.6",
  moon: "M19.5 14.6A8 8 0 0 1 9.4 4.5a8 8 0 1 0 10.1 10.1z",
  message: "M4.5 5.5h15v10.5H9.5l-5 4z",
  input: "M4 7.5h16v9H4z M7.5 11h1.5 M11 11h1.5 M14.5 11h2 M7.5 13.5h9",
  tv: "M3.5 5h17v11h-17z M8.5 20h7",
  link: "M10 14a4 4 0 0 0 5.7 0l2.8-2.8a4 4 0 0 0-5.7-5.7L11.5 6.8 M14 10a4 4 0 0 0-5.7 0l-2.8 2.8a4 4 0 0 0 5.7 5.7l1.3-1.3",
  tilt: "M12 3.5l7.5 4.3v8.4L12 20.5l-7.5-4.3V7.8z M12 12l7.5-4.2 M12 12v8.5 M12 12L4.5 7.8",
  speaker: "M4 9.5h3.2L12 5.5v13l-4.8-4H4z",
  waves: "M15.5 9.2a4 4 0 0 1 0 5.6 M18.2 6.6a7.6 7.6 0 0 1 0 10.8",
  slash: "M15.5 9l5 6 M20.5 9l-5 6",
} as const;

export type IconName = keyof typeof PATHS;

/** Icons drawn filled rather than stroked. */
const FILLED: ReadonlySet<IconName> = new Set(["play", "rewind", "fastforward", "stop"]);

const NS = "http://www.w3.org/2000/svg";

/** An icon as inline SVG, sized by CSS (1em by default), hidden from screen
 * readers: the control around it carries the label. */
export function icon(name: IconName, cls = ""): SVGSVGElement {
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("class", `icon ${FILLED.has(name) ? "icon-fill" : ""} ${cls}`.trim());
  svg.append(path(PATHS[name]));
  return svg;
}

/** The mute control's icon: a speaker whose waves give way to a cross, so
 * the change can be animated part by part. */
export function muteIcon(): SVGSVGElement {
  const svg = icon("speaker", "mute-icon");
  svg.append(path(PATHS.waves, "waves"), path(PATHS.slash, "slash"));
  return svg;
}

function path(d: string, cls?: string): SVGPathElement {
  const p = document.createElementNS(NS, "path");
  p.setAttribute("d", d);
  if (cls) p.setAttribute("class", cls);
  return p;
}
