// SPDX-License-Identifier: GPL-3.0-or-later
//
// The remote's parts, shared by the Remote screen and the PC's thin remote
// at the side of the desktop: the keys, the direction pad, the touchpad, the
// volume and channel rockers, the shortcuts, and the number and colour keys.
// Each part adds its undo to `off`.
import { accepts, enableWhen } from "../context.ts";
import type { Ctx } from "../context.ts";
import { flash, h, holdRepeat, setText, setVars } from "../dom.ts";
import { icon, muteIcon } from "../icons.ts";
import type { IconName } from "../icons.ts";
import { tilt } from "../motion.ts";
import type { KeyName } from "../protocol.ts";
import { shortcuts } from "../store.ts";
import type { Tile } from "../store.ts";
import { Swipe } from "../swipe.ts";

type Off = (() => void)[];

const ARROWS: readonly KeyName[] = ["up", "down", "left", "right"];

/** Keyboard keys that press the TV's keys while a remote is up and no text
 * field has the focus. */
const KEYBOARD: Record<string, KeyName> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "ok",
  Backspace: "back",
  Escape: "back",
  h: "home",
  m: "menu",
  i: "info",
  " ": "play",
  k: "pause",
  PageUp: "channelup",
  PageDown: "channeldown",
  "0": "0",
  "1": "1",
  "2": "2",
  "3": "3",
  "4": "4",
  "5": "5",
  "6": "6",
  "7": "7",
  "8": "8",
  "9": "9",
};

/** What a key's button shows: one of the icons, or a few letters. */
export type KeyFace = IconName | { text: string };

/** The remote's keys: what presses them, and the buttons that show a press. */
export interface Keys {
  press(name: KeyName): void;
  /** A button for one key; `repeat` presses again while it is held. */
  button(name: KeyName, label: string, face: KeyFace, cls: string, repeat?: boolean): HTMLButtonElement;
  /** Called at every press, however it was made. */
  onPress(fn: (name: KeyName) => void): void;
  /** Every element that shows only while the TV takes keys. */
  groups: HTMLElement[];
}

/** The keys, enabled while the link takes `key`. `groups` hide while it
 * doesn't: a pad that can never work is noise. */
export function makeKeys(ctx: Ctx, off: Off): Keys {
  const buttons = new Map<KeyName, HTMLButtonElement[]>();
  const listeners: ((name: KeyName) => void)[] = [];
  const groups: HTMLElement[] = [];
  const press = (name: KeyName) => {
    for (const b of buttons.get(name) ?? []) flash(b, "pressed", 220);
    for (const fn of listeners) fn(name);
    void ctx.send("key", { name });
  };
  const paint = () => {
    const l = ctx.link.get();
    const on = accepts(l, "key");
    for (const list of buttons.values()) for (const b of list) b.disabled = !on;
    const offered = ctx.client.lastVerbs?.has("key") === true;
    for (const g of groups) g.hidden = !offered;
  };
  off.push(ctx.link.subscribe(paint));

  const onKey = (e: KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const t = e.target as HTMLElement;
    if (t.closest("input, textarea, select, [contenteditable]")) return;
    // Enter and Space on a focused button press that button instead.
    if ((e.key === "Enter" || e.key === " ") && t.closest("button, a")) return;
    const name = KEYBOARD[e.key];
    if (!name || !ctx.client.supports("key")) return;
    e.preventDefault();
    if (e.repeat && !ARROWS.includes(name)) return;
    press(name);
  };
  document.addEventListener("keydown", onKey);
  off.push(() => document.removeEventListener("keydown", onKey));

  return {
    press,
    button(name, label, face, cls, repeat = false) {
      const inner = typeof face === "string" ? icon(face) : h("span", { class: "key-text", text: face.text });
      const b = h("button", { class: cls, type: "button", "aria-label": label, title: label, "data-key": name }, inner);
      if (repeat) holdRepeat(b, () => press(name));
      else b.addEventListener("click", () => press(name));
      buttons.set(name, [...(buttons.get(name) ?? []), b]);
      paint();
      return b;
    },
    onPress: (fn) => listeners.push(fn),
    groups,
  };
}

/** The pad: four arcs and OK, as one glass disc that leans toward a press. */
export function padPart(keys: Keys): HTMLElement {
  const ok = keys.button("ok", "OK", { text: "OK" }, "pad-ok");
  const pad = h(
    "div",
    { class: "pad tilt", role: "group", "aria-label": "Direction pad: arrow keys move, Enter is OK, Backspace is Back" },
    h("div", { class: "pad-disc glass refract" }),
    keys.button("up", "Up", "up", "pad-arrow pad-up", true),
    keys.button("right", "Right", "right", "pad-arrow pad-right", true),
    keys.button("down", "Down", "down", "pad-arrow pad-down", true),
    keys.button("left", "Left", "left", "pad-arrow pad-left", true),
    ok,
  );
  tilt(pad, 6);
  keys.onPress((name) => {
    if (ARROWS.includes(name) || name === "ok") flash(pad, `lean-${name}`, 260);
  });
  keys.groups.push(pad);
  return pad;
}

/** The touchpad: a swipe moves, a tap is OK, a touch held still is Back, and
 * the wheel moves up and down. A light follows the finger and each arrow
 * sent leaves a streak the way it went. */
export function touchpadPart(ctx: Ctx, keys: Keys, off: Off): HTMLElement {
  const dot = h("span", { class: "touch-dot", "aria-hidden": "true" });
  const streak = h("span", { class: "touch-streak", "aria-hidden": "true" });
  const el = h(
    "div",
    {
      class: "touchpad glass",
      role: "group",
      tabindex: "0",
      "aria-label": "Touchpad: swipe to move, tap for OK, hold for Back",
    },
    dot,
    streak,
    h(
      "p",
      { class: "touch-hint" },
      h("span", { text: "Swipe to move" }),
      h("span", { text: "Tap for OK · hold for Back" }),
    ),
  );
  const swipe = new Swipe();
  let pointer: number | null = null;
  let holdTimer: ReturnType<typeof setTimeout> | null = null;
  const clearHold = () => {
    if (holdTimer) clearTimeout(holdTimer);
    holdTimer = null;
  };
  const place = (e: PointerEvent) => {
    const r = el.getBoundingClientRect();
    setVars(el, { "touch-x": `${(e.clientX - r.left).toFixed(0)}px`, "touch-y": `${(e.clientY - r.top).toFixed(0)}px` });
  };
  const send = (name: KeyName) => {
    if (!ctx.client.supports("key")) return;
    keys.press(name);
    if (ARROWS.includes(name)) {
      streak.dataset.way = name;
      flash(streak, "go", 320);
    } else {
      flash(el, name === "ok" ? "tapped" : "held", 380);
    }
  };
  el.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || pointer !== null) return;
    pointer = e.pointerId;
    el.setPointerCapture(e.pointerId);
    el.classList.add("touching");
    place(e);
    swipe.down(e.clientX, e.clientY, e.timeStamp);
    clearHold();
    holdTimer = setTimeout(() => {
      const key = swipe.hold(performance.now());
      if (key) send(key);
    }, 620);
  });
  el.addEventListener("pointermove", (e) => {
    if (e.pointerId !== pointer) return;
    place(e);
    // A fast flick is capped: three arrows from one move are plenty.
    for (const key of swipe.move(e.clientX, e.clientY).slice(0, 3)) send(key);
  });
  const end = (e: PointerEvent, cancelled: boolean) => {
    if (e.pointerId !== pointer) return;
    pointer = null;
    clearHold();
    el.classList.remove("touching");
    if (cancelled) swipe.cancel();
    else {
      const key = swipe.up(e.timeStamp);
      if (key) send(key);
    }
  };
  el.addEventListener("pointerup", (e) => end(e, false));
  el.addEventListener("pointercancel", (e) => end(e, true));
  el.addEventListener("contextmenu", (e) => e.preventDefault());
  let wheel = 0;
  el.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      wheel += e.deltaY;
      while (Math.abs(wheel) >= 80) {
        send(wheel > 0 ? "down" : "up");
        wheel -= Math.sign(wheel) * 80;
      }
    },
    { passive: false },
  );
  off.push(clearHold);
  keys.groups.push(el);
  return el;
}

/** Volume: up, the level, down, in one capsule, with mute under it. */
export function volumeRocker(ctx: Ctx, off: Off): HTMLElement {
  const up = h("button", { class: "rocker-key", type: "button", "aria-label": "Volume up", title: "Volume up" }, icon("plus"));
  const down = h(
    "button",
    { class: "rocker-key", type: "button", "aria-label": "Volume down", title: "Volume down" },
    icon("minus"),
  );
  const level = h("span", { class: "rocker-mid vol-level", "aria-hidden": "true" });
  holdRepeat(up, () => void ctx.send("volume.up"));
  holdRepeat(down, () => void ctx.send("volume.down"));
  const mute = h(
    "button",
    { class: "round-button small mute-key", type: "button", "aria-label": "Mute", title: "Mute", "aria-pressed": "false" },
    muteIcon(),
  );
  mute.addEventListener("click", () => {
    void ctx.send("mute.set", { muted: !(ctx.tv.get().volume?.muted ?? false) });
  });
  off.push(
    ctx.tv.subscribe((s, before) => {
      setText(level, s.volume ? String(s.volume.level) : "–");
      mute.classList.toggle("muted", s.volume?.muted === true);
      mute.setAttribute("aria-pressed", String(s.volume?.muted === true));
      if (s.volume && before.volume && s.volume.level !== before.volume.level) flash(level, "bump", 400);
    }),
    enableWhen(ctx.link, [up, down], "volume.up", "volume.down"),
    enableWhen(ctx.link, [mute], "mute.set"),
  );
  return h(
    "div",
    { class: "rocker-col" },
    h("div", { class: "rocker glass", role: "group", "aria-label": "Volume" }, up, level, down),
    mute,
    h("span", { class: "rocker-label", text: "Vol" }),
  );
}

/** Channel: up and down in one capsule. */
export function channelRocker(keys: Keys): HTMLElement {
  const el = h(
    "div",
    { class: "rocker-col" },
    h(
      "div",
      { class: "rocker glass", role: "group", "aria-label": "Channel" },
      keys.button("channelup", "Channel up", "up", "rocker-key", true),
      h("span", { class: "rocker-mid", "aria-hidden": "true", text: "CH" }),
      keys.button("channeldown", "Channel down", "down", "rocker-key", true),
    ),
    keys.button("info", "Info", "info", "round-button small"),
    h("span", { class: "rocker-label", text: "Ch" }),
  );
  keys.groups.push(el);
  return el;
}

/** One-tap tiles for the inputs with something plugged in and the family's
 * apps; the one in front is lit. Hidden while the TV has named none. */
export function shortcutsPart(ctx: Ctx, off: Off, max: number): HTMLElement {
  const el = h("div", { class: "shortcuts", role: "list", "aria-label": "Shortcuts" });
  const els = new Map<string, HTMLButtonElement>();
  let ids = "";
  const make = (t: Tile): HTMLButtonElement => {
    const short = t.kind === "input" ? t.title.replace(/^HDMI\s*/, "") : ([...t.title][0] ?? "?");
    const b = h(
      "button",
      { class: `shortcut kind-${t.kind}`, type: "button", role: "listitem", "data-id": t.id },
      h("span", { class: "shortcut-face" }, t.kind === "input" ? icon("input") : null, h("span", { text: short })),
      h("span", { class: "shortcut-title", text: t.title }),
    );
    b.addEventListener("click", () => {
      flash(b, "launch", 520);
      void ctx.send("launch", { id: t.id }).then((done) => {
        if (done) ctx.say(`Opening ${t.title}`);
      });
    });
    return b;
  };
  off.push(
    ctx.tv.subscribe((s) => {
      const list = shortcuts(s, max);
      const next = list.map((t) => t.id).join("\n");
      if (next !== ids) {
        ids = next;
        els.clear();
        el.replaceChildren();
        for (const t of list) {
          const b = make(t);
          els.set(t.id, b);
          el.append(b);
        }
      }
      const can = accepts(ctx.link.get(), "launch");
      for (const t of list) {
        const b = els.get(t.id);
        if (!b) continue;
        setVars(b, { tile: t.color });
        b.classList.toggle("in-front", t.inFront);
        b.disabled = !can;
        const label = t.device ? `${t.title}, ${t.device}` : t.title;
        b.title = label;
        b.setAttribute("aria-label", t.inFront ? `${label}, in front` : label);
      }
      el.hidden = list.length === 0;
    }),
    ctx.link.subscribe((l) => {
      const can = accepts(l, "launch");
      for (const b of els.values()) b.disabled = !can;
    }),
  );
  return el;
}

/** The number keys, as on a phone's dial. */
export function numbersPart(keys: Keys): HTMLElement {
  const key = (n: KeyName) => keys.button(n, n, { text: n }, "num-key");
  const el = h(
    "div",
    { class: "numbers", role: "group", "aria-label": "Number keys" },
    ...(["1", "2", "3", "4", "5", "6", "7", "8", "9"] as const).map(key),
    keys.button("exit", "Exit", "exit", "num-key side"),
    key("0"),
    keys.button("info", "Info", "info", "num-key side"),
  );
  keys.groups.push(el);
  return el;
}

/** The four colour keys. */
export function coloursPart(keys: Keys): HTMLElement {
  const el = h(
    "div",
    { class: "colours", role: "group", "aria-label": "Colour keys" },
    ...(["red", "green", "yellow", "blue"] as const).map((c) =>
      keys.button(c, c[0]!.toUpperCase() + c.slice(1), { text: "" }, `colour-key colour-${c}`),
    ),
  );
  keys.groups.push(el);
  return el;
}
