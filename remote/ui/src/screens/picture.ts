// SPDX-License-Identifier: GPL-3.0-or-later
//
// Picture and sound: a fixed list of LG's settings (D7). The screen reads
// them when it opens and shows each one's value before anything changes;
// it writes one only when asked, and every write offers an undo
// (remote.md, risk 9).
import { accepts } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { h, setText, setVars } from "../dom.ts";
import { icon } from "../icons.ts";
import type { IconName } from "../icons.ts";
import { SETTING_LABELS, valueLabel } from "../labels.ts";
import { rimLight } from "../motion.ts";
import { SETTINGS } from "../protocol.ts";
import type { SettingChange, SettingKey } from "../protocol.ts";

const GROUPS: { key: SettingKey; icon: IconName; note: string }[] = [
  { key: "picture.mode", icon: "picture", note: "LG's preset for the input in front." },
  { key: "picture.oledLight", icon: "lumen", note: "How bright the panel glows." },
  { key: "picture.energySaving", icon: "moon", note: "Dims the picture to save power." },
  { key: "sound.mode", icon: "sound", note: "LG's sound preset." },
];

export function pictureScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const pending = new Map<SettingKey, ReturnType<typeof setTimeout>>();
  const groups = new Map<SettingKey, (value: string | number | undefined) => void>();
  const all: (HTMLButtonElement | HTMLInputElement)[] = [];

  const write = async (change: SettingChange, undo = true) => {
    const before = ctx.tv.get().settings[change.key];
    if (before === change.value) return;
    markPending(change.key);
    const done = await ctx.send("settings.set", change);
    if (!done || !undo || before === undefined) return;
    const label = SETTING_LABELS[change.key];
    ctx.say(`${label}: ${valueLabel(before)} → ${valueLabel(change.value)}`, {
      label: "Undo",
      run: () => void write({ key: change.key, value: before } as SettingChange, false),
    });
  };

  const markPending = (key: SettingKey) => {
    clearTimeout(pending.get(key));
    cards.get(key)?.classList.add("pending");
    pending.set(
      key,
      setTimeout(() => cards.get(key)?.classList.remove("pending"), 3000),
    );
  };

  const cards = new Map<SettingKey, HTMLElement>();
  for (const g of GROUPS) {
    const spec = SETTINGS[g.key];
    const current = h("span", { class: "setting-value" });
    let control: HTMLElement;
    if (spec.kind === "enum") {
      const glow = h("span", { class: "chip-glow", "aria-hidden": "true" });
      const chips = h("div", { class: "chips", role: "group", "aria-label": SETTING_LABELS[g.key] }, glow);
      const buttons = new Map<string, HTMLButtonElement>();
      for (const value of spec.values) {
        const b = h("button", { class: "chip-option", type: "button", "aria-pressed": "false", text: valueLabel(value) });
        b.addEventListener("click", () => void write({ key: g.key, value } as SettingChange));
        buttons.set(value, b);
        all.push(b);
        chips.append(b);
      }
      const place = () => {
        const sel = [...buttons.values()].find((b) => b.getAttribute("aria-pressed") === "true");
        chips.classList.toggle("has-value", sel !== undefined);
        if (!sel) return;
        setVars(chips, {
          gx: `${sel.offsetLeft}px`,
          gy: `${sel.offsetTop}px`,
          gw: `${sel.offsetWidth}px`,
          gh: `${sel.offsetHeight}px`,
        });
      };
      const ro = new ResizeObserver(place);
      ro.observe(chips);
      off.push(() => ro.disconnect());
      groups.set(g.key, (v) => {
        for (const [value, b] of buttons) b.setAttribute("aria-pressed", String(value === v));
        place();
      });
      control = chips;
    } else {
      const range = h("input", {
        class: "setting-range",
        type: "range",
        min: String(spec.min),
        max: String(spec.max),
        step: "1",
        "aria-label": SETTING_LABELS[g.key],
      });
      let dragging = false;
      range.addEventListener("input", () => {
        dragging = true;
        setText(current, range.value);
        setVars(range, { fill: `${((Number(range.value) - spec.min) / (spec.max - spec.min)) * 100}%` });
      });
      range.addEventListener("change", () => {
        dragging = false;
        void write({ key: g.key, value: Number(range.value) } as SettingChange);
      });
      all.push(range);
      groups.set(g.key, (v) => {
        if (dragging || typeof v !== "number") return;
        range.value = String(v);
        setVars(range, { fill: `${((v - spec.min) / (spec.max - spec.min)) * 100}%` });
      });
      control = range;
    }
    const card = h(
      "div",
      { class: `setting glass ${spec.kind}` },
      h(
        "div",
        { class: "setting-head" },
        icon(g.icon),
        h("div", null, h("h2", { class: "setting-title", text: SETTING_LABELS[g.key] }), h("p", { class: "setting-note", text: g.note })),
        current,
      ),
      control,
    );
    groups.set(
      g.key,
      ((inner) => (v: string | number | undefined) => {
        setText(current, valueLabel(v));
        inner(v);
      })(groups.get(g.key)!),
    );
    cards.set(g.key, card);
    off.push(rimLight(card));
  }

  off.push(
    ctx.tv.subscribe((s, before) => {
      for (const [key, show] of groups) {
        const v = s.settings[key];
        show(v);
        if (v !== before.settings[key]) {
          clearTimeout(pending.get(key));
          cards.get(key)?.classList.remove("pending");
        }
      }
    }),
  );
  let wasOpen = false;
  off.push(
    ctx.link.subscribe((l) => {
      for (const c of all) c.disabled = !accepts(l, "settings.set");
      // Read on opening the screen and after each reconnect.
      if (l.kind === "open" && !wasOpen) void read();
      wasOpen = l.kind === "open";
    }),
  );
  off.push(() => pending.forEach((t) => clearTimeout(t)));

  async function read(): Promise<void> {
    const r = await ctx.send("settings.get");
    if (r) ctx.tv.update((s) => ({ ...s, settings: { ...s.settings, ...r.values } }));
  }

  const el = h(
    "section",
    { class: "picture" },
    h("div", { class: "settings" }, ...cards.values()),
    h("p", { class: "hint" }, icon("check"), h("span", {
      text: "Nothing changes on its own: each setting is written only when you pick it, and can be undone.",
    })),
  );
  return {
    el,
    shown: () => {
      for (const [key, show] of groups) show(ctx.tv.get().settings[key]);
    },
    destroy: () => off.forEach((f) => f()),
  };
}
