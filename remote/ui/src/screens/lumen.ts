// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lumen: open Lumen's home and its ambient screensaver on the TV, put a
// message on the TV, and the privacy state: the boot hook's log, the 11111
// rule, LG's stopped diagnostics jobs, and Guard's counts once Guard exists
// (privacy.md).
import { enableWhen } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { h, setText } from "../dom.ts";
import { icon } from "../icons.ts";
import type { IconName } from "../icons.ts";
import { rimLight } from "../motion.ts";
import { MAX_TOAST } from "../protocol.ts";
import type { PrivacyState } from "../protocol.ts";

export function lumenScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];

  const action = (iconName: IconName, title: string, note: string, run: () => void) => {
    const b = h(
      "button",
      { class: "action tilt", type: "button" },
      h("span", { class: "action-icon" }, icon(iconName)),
      h("span", { class: "action-title", text: title }),
      h("span", { class: "action-note", text: note }),
    );
    b.addEventListener("click", run);
    return b;
  };
  const openLumen = action("lumen", "Open Lumen", "Lumen's home, in front on the TV", () => {
    void ctx.send("lumen.open").then((r) => r && ctx.say("Opening Lumen on the TV"));
  });
  const ambient = action("moon", "Start ambient", "Lumen's screensaver, now", () => {
    void ctx.send("ambient.start").then((r) => r && ctx.say("Ambient starting on the TV"));
  });
  off.push(enableWhen(ctx.link, [openLumen], "lumen.open"), enableWhen(ctx.link, [ambient], "ambient.start"));

  const message = h("input", {
    class: "text-field",
    id: "toast-field",
    type: "text",
    maxlength: String(MAX_TOAST),
    autocomplete: "off",
    placeholder: "Dinner's ready",
  });
  const sendMessage = h("button", { class: "pill-button", type: "submit" }, icon("message"), h("span", { text: "Show" }));
  const toastForm = h(
    "form",
    { class: "toast-form" },
    h("label", { for: "toast-field", class: "field-label", text: "A message on the TV" }),
    h("div", { class: "type-row" }, message, sendMessage),
  );
  toastForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const text = message.value.trim();
    if (!text) return;
    void ctx.send("toast", { text }).then((r) => {
      if (r) {
        ctx.say("Shown on the TV");
        message.value = "";
      }
    });
  });
  off.push(enableWhen(ctx.link, [message, sendMessage], "toast"));

  const hero = h(
    "div",
    { class: "lumen-hero glass" },
    h("div", { class: "actions" }, openLumen, ambient),
    toastForm,
  );

  // Privacy.
  const checks = h("ul", { class: "checks" });
  const log = h("pre", { class: "log", tabindex: "0", "aria-label": "The boot hook's log" });
  const readAt = h("span", { class: "read-at" });
  const refresh = h("button", { class: "round-button small", type: "button", "aria-label": "Read again", title: "Read again" }, icon("refresh"));
  refresh.addEventListener("click", () => void read());
  off.push(enableWhen(ctx.link, [refresh], "privacy.state"));
  const privacy = h(
    "div",
    { class: "privacy glass" },
    h("div", { class: "card-head" }, icon("shield"), h("h2", { text: "Privacy" }), readAt, refresh),
    checks,
    h("details", { class: "log-box" }, h("summary", { text: "The boot hook's log" }), log),
  );
  privacy.classList.add("loading");

  const show = (p: PrivacyState) => {
    const item = (ok: boolean | null, text: string, note?: string) =>
      h(
        "li",
        { class: ok === null ? "na" : ok ? "ok" : "bad" },
        icon(ok === false ? "warn" : "check"),
        h("span", null, text, note ? h("small", { text: note }) : null),
      );
    const g = p.guard;
    checks.replaceChildren(
      item(p.hook.installed, p.hook.installed ? "Boot hook installed" : "Boot hook not installed", "lumen-privacy, stage S1"),
      item(p.port11111.rejected, p.port11111.rejected ? "Port 11111 closed to the LAN" : "Port 11111 open to the LAN", "crashreportd's port"),
      ...p.jobs.map((j) => item(!j.running, `${j.name} ${j.running ? "running" : "stopped"}`, "LG diagnostics job")),
      g
        ? item(true, `Guard: ${g.lookups.toLocaleString()} lookups, ${g.blocked.toLocaleString()} blocked`, `since ${new Date(g.since).toLocaleString()}`)
        : item(null, "Lumen Guard isn't installed yet", "G1 adds its counts here"),
    );
    setText(log, p.hook.log.length ? p.hook.log.join("\n") : "Nothing logged on this boot.");
    setText(readAt, `Read ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
    privacy.classList.remove("loading");
  };

  async function read(): Promise<void> {
    privacy.classList.add("reading");
    const p = await ctx.send("privacy.state");
    privacy.classList.remove("reading");
    if (p) {
      ctx.tv.update((s) => ({ ...s, privacy: p }));
      show(p);
    }
  }
  const known = ctx.tv.get().privacy;
  if (known) show(known);
  let wasOpen = false;
  off.push(
    ctx.link.subscribe((l) => {
      if (l.kind === "open" && !wasOpen) void read();
      wasOpen = l.kind === "open";
    }),
  );
  off.push(rimLight(hero), rimLight(privacy));

  const el = h("section", { class: "lumen" }, hero, privacy);
  return { el, destroy: () => off.forEach((f) => f()) };
}
