// SPDX-License-Identifier: GPL-3.0-or-later
//
// Apps and inputs: apps.list as a grid, launched with `launch <id>`. The tile
// in front is lit and the page takes its colour; inputs show whether
// something is plugged in, from the `inputs` event.
import { accepts } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { flash, h, setText, setVars } from "../dom.ts";
import { icon } from "../icons.ts";
import { tilt } from "../motion.ts";
import { tiles } from "../store.ts";
import type { Tile } from "../store.ts";

export function appsScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const inputsGrid = h("div", { class: "grid inputs", role: "list" });
  const appsGrid = h("div", { class: "grid", role: "list" });
  const empty = h("div", { class: "empty glass" });
  const sections = h(
    "div",
    { class: "app-sections" },
    h("h2", { class: "section-title", text: "Inputs" }),
    inputsGrid,
    h("h2", { class: "section-title", text: "Apps" }),
    appsGrid,
  );
  const els = new Map<string, HTMLButtonElement>();
  let ids = "";

  const launch = async (t: Tile, el: HTMLButtonElement) => {
    flash(el, "launch", 520);
    const done = await ctx.send("launch", { id: t.id });
    if (done) ctx.say(`Opening ${t.title}`);
  };

  const makeTile = (t: Tile): HTMLButtonElement => {
    const el = h(
      "button",
      { class: `tile tilt kind-${t.kind}`, type: "button", role: "listitem", "data-id": t.id },
      h(
        "span",
        { class: "tile-face" },
        h("span", { class: "tile-glint", "aria-hidden": "true" }),
        t.kind === "input"
          ? h("span", { class: "tile-input" }, icon("input"), h("span", { text: t.title.replace(/^HDMI\s*/, "") }))
          : h("span", { class: "tile-letter", text: [...t.title][0] ?? "?" }),
      ),
      h("span", { class: "tile-title", text: t.title }),
      h("span", { class: "tile-sub" }),
    );
    el.addEventListener("click", () => void launch(t, el));
    el.disabled = !accepts(ctx.link.get(), "launch");
    tilt(el, 10);
    return el;
  };

  const update = (el: HTMLButtonElement, t: Tile) => {
    setVars(el, { tile: t.color });
    el.classList.toggle("in-front", t.inFront);
    el.classList.toggle("connected", t.connected === true);
    el.classList.toggle("unplugged", t.connected === false);
    const plug = t.connected === true ? (t.device ?? "Connected") : t.connected === false ? "Nothing plugged in" : null;
    const sub = [t.inFront ? "In front" : null, plug].filter((x) => x !== null).join(" · ");
    setText(el.querySelector(".tile-sub")!, sub);
    el.setAttribute("aria-label", sub ? `${t.title}, ${sub}` : t.title);
    if (t.inFront) el.setAttribute("aria-current", "true");
    else el.removeAttribute("aria-current");
  };

  off.push(
    ctx.tv.subscribe((s) => {
      const list = tiles(s);
      const nextIds = list.map((t) => t.id).join("\n");
      if (nextIds !== ids) {
        ids = nextIds;
        els.clear();
        inputsGrid.replaceChildren();
        appsGrid.replaceChildren();
        for (const t of list) {
          const el = makeTile(t);
          els.set(t.id, el);
          (t.kind === "input" ? inputsGrid : appsGrid).append(el);
        }
      }
      for (const t of list) {
        const el = els.get(t.id);
        if (el) update(el, t);
      }
      sections.hidden = list.length === 0;
      empty.hidden = list.length > 0;
    }),
  );

  off.push(
    ctx.link.subscribe((l) => {
      const can = accepts(l, "launch");
      for (const el of els.values()) el.disabled = !can;
      empty.replaceChildren(
        icon(l.kind === "open" ? "apps" : "warn"),
        h("span", { text: l.kind === "open" ? "Asking the TV for its apps…" : "Not connected to the TV" }),
      );
      if (l.kind === "open" && ctx.tv.get().apps === null) void refresh();
    }),
  );

  async function refresh(): Promise<void> {
    const r = await ctx.send("apps.list");
    if (r) ctx.tv.update((s) => ({ ...s, apps: r.apps }));
  }

  const el = h("section", { class: "apps" }, empty, sections);
  return { el, destroy: () => off.forEach((f) => f()) };
}
