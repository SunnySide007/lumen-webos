// SPDX-License-Identifier: GPL-3.0-or-later
//
// The PC's thin remote: one narrow column that sits at the side of the
// desktop, above other windows, like a remote left on the desk. From the
// top: a grip to drag it by with the link's light, power, the pad, Back,
// Home and Menu, the volume and channel rockers, playback, and shortcuts.
// The full window is one click away for everything else.
import { accepts, enableWhen } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { h } from "../dom.ts";
import { icon } from "../icons.ts";
import { channelRocker, makeKeys, padPart, shortcutsPart, volumeRocker } from "./parts.ts";

export function sideRemote(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const keys = makeKeys(ctx, off);
  const pc = ctx.host.pc;

  // The grip: Tauri drags the window by any element with this attribute.
  const light = h("button", {
    class: "side-light",
    type: "button",
    onclick: () => ctx.client.retryNow(),
  });
  off.push(
    ctx.link.subscribe((l) => {
      const tone = l.kind === "open" ? (l.stale ? "stale" : "ok") : l.kind === "connecting" || l.kind === "paused" ? "busy" : "bad";
      light.dataset.tone = tone;
      const text =
        l.kind === "open" ? "Connected" : l.kind === "connecting" ? "Connecting" : l.kind === "paused" ? "Paused" : "Not connected: click to retry";
      light.title = text;
      light.setAttribute("aria-label", `Link to the TV: ${text}`);
    }),
  );
  const open = h(
    "button",
    { class: "side-tool", type: "button", "aria-label": "Open the full window", title: "Open the full window" },
    icon("expand"),
  );
  open.addEventListener("click", () => void pc?.openMain().catch((e: unknown) => ctx.say(String(e))));
  const hide = h("button", { class: "side-tool", type: "button", "aria-label": "Hide the remote", title: "Hide (the tray icon shows it again)" }, icon("exit"));
  hide.addEventListener("click", () => void pc?.hide().catch((e: unknown) => ctx.say(String(e))));
  const grip = h(
    "div",
    { class: "side-grip", "data-tauri-drag-region": true, title: "Drag to move" },
    light,
    h("span", { class: "side-grip-dots", "data-tauri-drag-region": true, "aria-hidden": "true" }, icon("grip")),
    open,
    hide,
  );

  // Power: the screen off and on, and off for good after a second click.
  const screen = h("button", { class: "round-button small", type: "button" }, icon("screenOff"));
  screen.addEventListener("click", () => {
    void ctx.send(ctx.tv.get().power === "screen-off" ? "screen.on" : "screen.off");
  });
  off.push(
    ctx.tv.subscribe((s) => {
      const isOff = s.power === "screen-off";
      screen.replaceChildren(icon(isOff ? "screen" : "screenOff"));
      screen.title = isOff ? "Screen on" : "Screen off";
      screen.setAttribute("aria-label", screen.title);
    }),
    ctx.link.subscribe((l) => {
      screen.disabled = !accepts(l, "screen.off") || !accepts(l, "screen.on");
    }),
  );
  const power = h("button", { class: "round-button small danger", type: "button" }, icon("power"));
  let armed: ReturnType<typeof setTimeout> | null = null;
  const disarm = () => {
    if (armed) clearTimeout(armed);
    armed = null;
    power.classList.remove("armed");
    power.title = "Turn off";
    power.setAttribute("aria-label", "Turn off");
  };
  disarm();
  power.addEventListener("click", () => {
    if (!armed) {
      power.classList.add("armed");
      power.title = "Click again to turn the TV off";
      power.setAttribute("aria-label", power.title);
      armed = setTimeout(disarm, 3000);
      return;
    }
    disarm();
    ctx.say("Turning the TV off");
    void ctx.send("power.off");
  });
  off.push(enableWhen(ctx.link, [power], "power.off"), disarm);
  const wake = h("button", { class: "round-button small accent", type: "button", hidden: true, title: "Turn on", "aria-label": "Turn on" }, icon("power"));
  if (pc) {
    wake.addEventListener("click", () => {
      wake.disabled = true;
      pc.wake().then(
        () => {
          ctx.say("Wake-on-LAN sent");
          ctx.client.retryNow();
        },
        (e: unknown) => ctx.say(String(e)),
      ).finally(() => (wake.disabled = false));
    });
    const paint = () => {
      const standby = ctx.tv.get().power === "standby" || ctx.link.get().kind !== "open";
      wake.hidden = !standby;
      power.hidden = standby;
    };
    off.push(ctx.link.subscribe(paint), ctx.tv.subscribe(paint));
  }
  const powerRow = h("div", { class: "side-row" }, screen, wake, power);

  const nav = h(
    "div",
    { class: "side-row" },
    keys.button("back", "Back", "back", "round-button small"),
    keys.button("home", "Home", "home", "round-button small"),
    keys.button("menu", "Menu", "menu", "round-button small"),
  );
  const playback = h(
    "div",
    { class: "side-play", role: "group", "aria-label": "Playback" },
    keys.button("rewind", "Rewind", "rewind", "round-button small"),
    keys.button("play", "Play", "play", "round-button small"),
    keys.button("pause", "Pause", "pause", "round-button small"),
    keys.button("fastforward", "Fast-forward", "fastforward", "round-button small"),
  );
  keys.groups.push(nav, playback);

  const el = h(
    "section",
    { class: "side-remote glass", "aria-label": "Remote" },
    grip,
    powerRow,
    padPart(keys),
    nav,
    h("div", { class: "side-rockers" }, volumeRocker(ctx, off), channelRocker(keys)),
    playback,
    shortcutsPart(ctx, off, 6),
  );
  return { el, destroy: () => off.forEach((f) => f()) };
}
