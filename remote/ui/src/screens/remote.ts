// SPDX-License-Identifier: GPL-3.0-or-later
//
// Remote: the direction pad, OK, Back, Home and Menu, playback, power, and a
// field whose typing goes to the TV's focused text field.
import { accepts, enableWhen } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { flash, h, holdRepeat } from "../dom.ts";
import { icon } from "../icons.ts";
import type { IconName } from "../icons.ts";
import { rimLight, tilt } from "../motion.ts";
import type { KeyName } from "../protocol.ts";
import { diffText, editCommands } from "../textdiff.ts";

/** Keyboard keys that press the TV's keys while the Remote screen is up and
 * no text field has the focus. */
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
  " ": "play",
  k: "pause",
};

export function remoteScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const keyButtons = new Map<KeyName, HTMLButtonElement>();

  const press = (name: KeyName) => {
    const b = keyButtons.get(name);
    if (b) flash(b, "pressed", 220);
    if (["up", "down", "left", "right", "ok"].includes(name)) flash(pad, `lean-${name}`, 260);
    void ctx.send("key", { name });
  };

  const keyButton = (name: KeyName, label: string, iconName: IconName, cls: string, repeat = false) => {
    const b = h("button", { class: cls, type: "button", "aria-label": label, title: label }, icon(iconName));
    if (repeat) holdRepeat(b, () => press(name));
    else b.addEventListener("click", () => press(name));
    keyButtons.set(name, b);
    return b;
  };

  // The pad: four arcs and OK, as one glass disc that leans toward a press.
  const ok = h("button", { class: "pad-ok", type: "button", "aria-label": "OK", title: "OK" }, h("span", { text: "OK" }));
  ok.addEventListener("click", () => press("ok"));
  keyButtons.set("ok", ok);
  const pad = h(
    "div",
    { class: "pad tilt", role: "group", "aria-label": "Direction pad: arrow keys move, Enter is OK, Backspace is Back" },
    h("div", { class: "pad-disc glass refract" }),
    keyButton("up", "Up", "up", "pad-arrow pad-up", true),
    keyButton("right", "Right", "right", "pad-arrow pad-right", true),
    keyButton("down", "Down", "down", "pad-arrow pad-down", true),
    keyButton("left", "Left", "left", "pad-arrow pad-left", true),
    ok,
  );
  tilt(pad, 6);

  const nav = h(
    "div",
    { class: "key-row" },
    labelled(keyButton("back", "Back", "back", "pill-button"), "Back"),
    labelled(keyButton("home", "Home", "home", "pill-button"), "Home"),
    labelled(keyButton("menu", "Menu", "menu", "pill-button"), "Menu"),
  );

  const playback = h(
    "div",
    { class: "play-row glass", role: "group", "aria-label": "Playback" },
    keyButton("rewind", "Rewind", "rewind", "round-button"),
    keyButton("play", "Play", "play", "round-button big"),
    keyButton("pause", "Pause", "pause", "round-button big"),
    keyButton("stop", "Stop", "stop", "round-button"),
    keyButton("fastforward", "Fast-forward", "fastforward", "round-button"),
  );
  off.push(enableWhen(ctx.link, [...keyButtons.values()], "key"));

  // Power. On is Wake-on-LAN from the PC app or Google Home (remote.md), so
  // this page can only turn things off, and asks once more before it does.
  const screenButton = h("button", { class: "pill-button wide", type: "button" }, icon("screenOff"), h("span"));
  const screenLabel = screenButton.querySelector("span")!;
  screenButton.addEventListener("click", () => {
    const p = ctx.tv.get().power;
    void ctx.send(p === "screen-off" ? "screen.on" : "screen.off");
  });
  off.push(
    ctx.tv.subscribe((s) => {
      const isOff = s.power === "screen-off";
      screenLabel.textContent = isOff ? "Screen on" : "Screen off";
      screenButton.replaceChild(icon(isOff ? "screen" : "screenOff"), screenButton.firstChild!);
    }),
  );
  off.push(
    ctx.link.subscribe((l) => {
      screenButton.disabled = !accepts(l, "screen.off") || !accepts(l, "screen.on");
    }),
  );

  const powerButton = h("button", { class: "pill-button wide danger", type: "button" }, icon("power"), h("span", { text: "Turn off" }));
  const powerLabel = powerButton.querySelector("span")!;
  let armed: ReturnType<typeof setTimeout> | null = null;
  const disarm = () => {
    if (armed) clearTimeout(armed);
    armed = null;
    powerButton.classList.remove("armed");
    powerLabel.textContent = "Turn off";
  };
  powerButton.addEventListener("click", () => {
    if (!armed) {
      powerButton.classList.add("armed");
      powerLabel.textContent = "Turn off the TV?";
      armed = setTimeout(disarm, 3000);
      return;
    }
    disarm();
    ctx.say("Turning the TV off. The PC app or Google Home turns it back on.");
    void ctx.send("power.off");
  });
  off.push(enableWhen(ctx.link, [powerButton], "power.off"), disarm);
  const power = h("div", { class: "power-row" }, screenButton, powerButton);

  // Typing. What the field holds is compared with what was sent after every
  // change; Backspace in an empty field still deletes on the TV.
  let sent = "";
  const field = h("input", {
    class: "type-field",
    id: "type-field",
    type: "text",
    autocomplete: "off",
    autocapitalize: "off",
    spellcheck: "false",
    enterkeyhint: "send",
    placeholder: "Type into the TV's text field",
    "aria-describedby": "type-hint",
  });
  const enter = h("button", { class: "round-button", type: "submit", "aria-label": "Enter", title: "Enter" }, icon("enter"));
  const sendAll = (cmds: ReturnType<typeof editCommands>) => {
    for (const c of cmds) {
      void ctx.send(c.verb, c.args).then((r) => {
        if (r === null) {
          field.value = "";
          sent = "";
        }
      });
    }
  };
  field.addEventListener("input", () => {
    const cmds = editCommands(diffText(sent, field.value));
    sent = field.value;
    sendAll(cmds);
  });
  field.addEventListener("keydown", (e) => {
    if (e.key === "Backspace" && field.value === "" && !e.repeat) sendAll([{ verb: "text.delete", args: { count: 1 } }]);
  });
  const form = h(
    "form",
    { class: "type glass", "aria-label": "Type on the TV" },
    h("label", { for: "type-field", class: "type-label" }, icon("input"), h("span", { text: "Type on the TV" })),
    h("div", { class: "type-row" }, field, enter),
    h("p", { class: "hint", id: "type-hint", text: "Each letter goes to the TV as you type; Enter sends it." }),
  );
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    void ctx.send("text.enter");
    field.value = "";
    sent = "";
  });
  off.push(enableWhen(ctx.link, [field, enter], "text.insert", "text.delete", "text.enter"));
  off.push(rimLight(form), rimLight(playback));

  const noKeys = h("p", { class: "notice", hidden: true }, icon("warn"), h("span", {
    text: "This TV has no route for keys yet (remote.md, risk 1): apps, inputs, typing and playback still work.",
  }));
  off.push(
    ctx.link.subscribe((l) => {
      noKeys.hidden = !(l.kind === "open" && !l.verbs.has("key"));
    }),
  );

  const onKey = (e: KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const t = e.target as HTMLElement;
    if (t.closest("input, textarea, select, [contenteditable]")) return;
    // Enter and Space on a focused button press that button instead.
    if ((e.key === "Enter" || e.key === " ") && t.closest("button, a")) return;
    const name = KEYBOARD[e.key];
    if (!name || !ctx.client.supports("key")) return;
    e.preventDefault();
    if (e.repeat && !["up", "down", "left", "right"].includes(name)) return;
    press(name);
  };
  document.addEventListener("keydown", onKey);
  off.push(() => document.removeEventListener("keydown", onKey));

  const el = h(
    "section",
    { class: "remote" },
    h("div", { class: "remote-main" }, noKeys, pad, nav),
    h("div", { class: "remote-side" }, power, playback, form),
  );
  return {
    el,
    destroy: () => off.forEach((f) => f()),
  };
}

function labelled(button: HTMLButtonElement, text: string): HTMLButtonElement {
  button.append(h("span", { text }));
  return button;
}
