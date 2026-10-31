// SPDX-License-Identifier: GPL-3.0-or-later
//
// Sound: the volume as a liquid slider, a step either way, and mute. The
// number and the slider follow the TV's volume events, whoever changed it,
// except while the person here is dragging.
import { enableWhen } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { flash, h, holdRepeat, setText, setVars } from "../dom.ts";
import { icon, muteIcon } from "../icons.ts";
import { rimLight } from "../motion.ts";

/** How often a drag sends volume.set, at most. */
const SEND_EVERY_MS = 90;
/** How long after a drag the slider ignores events, which may still carry
 * the values the drag sent on the way. */
const SETTLE_MS = 600;

export function soundScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];

  const num = h("span", { class: "vol-big", "aria-hidden": "true", text: "–" });
  const state = h("span", { class: "vol-state", text: "Volume" });
  const range = h("input", {
    class: "vol-range",
    type: "range",
    min: "0",
    max: "100",
    step: "1",
    value: "0",
    "aria-label": "Volume",
  });
  const track = h("div", { class: "vol-track" }, h("div", { class: "vol-fill", "aria-hidden": "true" }), range);
  const down = h("button", { class: "round-button", type: "button", "aria-label": "Volume down", title: "Volume down" }, icon("minus"));
  const up = h("button", { class: "round-button", type: "button", "aria-label": "Volume up", title: "Volume up" }, icon("plus"));
  const mute = h(
    "button",
    { class: "mute-button", type: "button", "aria-pressed": "false" },
    muteIcon(),
    h("span", { class: "mute-text", text: "Mute" }),
  );
  holdRepeat(down, () => void ctx.send("volume.down"));
  holdRepeat(up, () => void ctx.send("volume.up"));
  mute.addEventListener("click", () => {
    void ctx.send("mute.set", { muted: !(ctx.tv.get().volume?.muted ?? false) });
  });

  // Local value while dragging; null when the slider shows the TV's.
  let local: number | null = null;
  let lastSent = -1;
  let sendTimer: ReturnType<typeof setTimeout> | null = null;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  const show = (level: number, muted: boolean) => {
    setText(num, String(level));
    range.value = String(level);
    range.setAttribute("aria-valuetext", `${level}${muted ? ", muted" : ""}`);
    setVars(card, { "fill-n": String(level / 100) });
  };
  const flush = () => {
    sendTimer = null;
    if (local !== null && local !== lastSent) {
      lastSent = local;
      void ctx.send("volume.set", { level: local });
    }
  };
  range.addEventListener("input", () => {
    local = Number(range.value);
    if (settleTimer) clearTimeout(settleTimer);
    show(local, ctx.tv.get().volume?.muted ?? false);
    sendTimer ??= setTimeout(flush, SEND_EVERY_MS);
  });
  range.addEventListener("change", () => {
    if (sendTimer) clearTimeout(sendTimer);
    flush();
    settleTimer = setTimeout(() => {
      local = null;
      const v = ctx.tv.get().volume;
      if (v) show(v.level, v.muted);
    }, SETTLE_MS);
  });

  const card = h(
    "div",
    { class: "vol-card glass" },
    h("div", { class: "vol-head" }, icon("sound"), state),
    h("div", { class: "vol-number" }, num),
    track,
    h("div", { class: "vol-buttons" }, down, mute, up),
  );
  off.push(rimLight(card));

  off.push(
    ctx.tv.subscribe((s, before) => {
      const v = s.volume;
      card.classList.toggle("unknown", v === null);
      if (!v) return;
      card.classList.toggle("muted", v.muted);
      mute.setAttribute("aria-pressed", String(v.muted));
      setText(mute.querySelector(".mute-text")!, v.muted ? "Muted" : "Mute");
      setText(state, v.muted ? "Muted" : "Volume");
      if (local !== null) return;
      show(v.level, v.muted);
      if (before.volume && before.volume.level !== v.level) flash(num, "tick", 420);
    }),
  );
  off.push(enableWhen(ctx.link, [range, down, up], "volume.set", "volume.up", "volume.down"));
  off.push(enableWhen(ctx.link, [mute], "mute.set"));
  off.push(() => {
    if (sendTimer) clearTimeout(sendTimer);
    if (settleTimer) clearTimeout(settleTimer);
  });

  const el = h(
    "section",
    { class: "sound" },
    card,
    h("p", { class: "hint", text: "Changes made on the TV's own remote show here as they happen." }),
  );
  return { el, destroy: () => off.forEach((f) => f()) };
}
