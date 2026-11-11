// SPDX-License-Identifier: GPL-3.0-or-later
//
// Remote: what is on the TV, the direction pad or the touchpad between the
// volume and channel rockers, Back, Home and Menu, shortcuts to the inputs
// and the family's apps, power, playback, the number and colour keys, and a
// field whose typing goes to the TV's focused text field. The keys show only
// once a hello has offered `key`: lumen-ctl leaves it out while the TV has no
// key route (remote.md, risk 1), and a pad that can never work is noise.
import { accepts, enableWhen } from "../context.ts";
import type { Ctx, Screen } from "../context.ts";
import { h, setText, setVars } from "../dom.ts";
import { icon } from "../icons.ts";
import { POWER_LABELS } from "../labels.ts";
import { rimLight } from "../motion.ts";
import { frontApp, frontTitle } from "../store.ts";
import { diffText, editCommands } from "../textdiff.ts";
import {
  channelRocker,
  coloursPart,
  makeKeys,
  numbersPart,
  padPart,
  shortcutsPart,
  touchpadPart,
  volumeRocker,
} from "./parts.ts";

/** Where the choice of pad or touchpad is kept, on this device only. */
const SURFACE_KEY = "lumen.remote.surface";
type Surface = "pad" | "touchpad";

function savedSurface(): Surface {
  try {
    return localStorage.getItem(SURFACE_KEY) === "touchpad" ? "touchpad" : "pad";
  } catch {
    return "pad";
  }
}

export function remoteScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const keys = makeKeys(ctx, off);

  // What is on the TV: the app or input in front, in its colour.
  const nowLetter = h("span", { class: "now-letter" });
  const nowTitle = h("strong", { class: "now-title" });
  const nowSub = h("span", { class: "now-sub" });
  const now = h(
    "div",
    { class: "now glass", role: "status", "aria-label": "On the TV" },
    h("span", { class: "now-face", "aria-hidden": "true" }, nowLetter),
    h("span", { class: "now-text" }, h("span", { class: "now-label", text: "On the TV" }), nowTitle, nowSub),
  );
  off.push(
    ctx.tv.subscribe((s) => {
      const title = frontTitle(s);
      const app = frontApp(s);
      now.hidden = title === null;
      setText(nowTitle, title ?? "");
      setText(nowLetter, app?.kind === "input" ? app.title.replace(/^HDMI\s*/, "") : ([...(title ?? "?")][0] ?? "?"));
      setVars(now, { tile: app?.color ?? "var(--ambient)" });
      const power = s.power && s.power !== "active" ? POWER_LABELS[s.power] : null;
      const sound = s.volume ? (s.volume.muted ? "Muted" : `Volume ${s.volume.level}`) : null;
      setText(nowSub, [power, app?.device, sound].filter((x) => x).join(" · "));
    }),
    rimLight(now),
  );

  // The pad or the touchpad, whichever this device last chose.
  const pad = padPart(keys);
  const touchpad = touchpadPart(ctx, keys, off);
  const surface = h("div", { class: "surface" }, pad, touchpad);
  const choose = (which: Surface, label: string, iconName: "remote" | "touchpad") =>
    h(
      "button",
      { class: "chip-option", type: "button", "data-surface": which, "aria-pressed": "false" },
      icon(iconName),
      h("span", { text: label }),
    );
  const padChoice = choose("pad", "Pad", "remote");
  const touchChoice = choose("touchpad", "Touchpad", "touchpad");
  const switcher = h("div", { class: "surface-switch", role: "group", "aria-label": "Pad or touchpad" }, padChoice, touchChoice);
  keys.groups.push(switcher);
  const show = (which: Surface) => {
    surface.dataset.surface = which;
    padChoice.setAttribute("aria-pressed", String(which === "pad"));
    touchChoice.setAttribute("aria-pressed", String(which === "touchpad"));
    try {
      localStorage.setItem(SURFACE_KEY, which);
    } catch {
      // Private browsing: the choice lasts as long as the page.
    }
  };
  padChoice.addEventListener("click", () => show("pad"));
  touchChoice.addEventListener("click", () => show("touchpad"));
  show(savedSurface());

  const deck = h("div", { class: "deck" }, volumeRocker(ctx, off), surface, channelRocker(keys));

  const nav = h(
    "div",
    { class: "key-row" },
    labelled(keys.button("back", "Back", "back", "pill-button"), "Back"),
    labelled(keys.button("home", "Home", "home", "pill-button"), "Home"),
    labelled(keys.button("menu", "Menu", "menu", "pill-button"), "Menu"),
  );
  keys.groups.push(nav);

  const playback = h(
    "div",
    { class: "play-row glass", role: "group", "aria-label": "Playback" },
    keys.button("rewind", "Rewind", "rewind", "round-button"),
    keys.button("play", "Play", "play", "round-button big"),
    keys.button("pause", "Pause", "pause", "round-button big"),
    keys.button("stop", "Stop", "stop", "round-button"),
    keys.button("fastforward", "Fast-forward", "fastforward", "round-button"),
  );
  keys.groups.push(playback);

  const shortcutList = shortcutsPart(ctx, off, 8);

  // The keys a remote has and a pad doesn't: numbers and the four colours.
  const more = h(
    "details",
    { class: "more-keys glass" },
    h("summary", null, icon("keypad"), h("span", { text: "Numbers and colours" })),
    numbersPart(keys),
    coloursPart(keys),
  );
  keys.groups.push(more);

  // Power. On is Wake-on-LAN, which only the PC app can send (remote.md):
  // the TV's own server is off whenever the TV is. A phone has Google Home.
  // Off asks once more before it acts.
  const pc = ctx.host.pc;
  const wakeButton = h("button", { class: "pill-button wide", type: "button", hidden: true }, icon("power"), h("span", { text: "Turn on" }));
  if (pc) {
    wakeButton.addEventListener("click", () => {
      wakeButton.disabled = true;
      pc.wake().then(
        () => {
          ctx.say("Wake-on-LAN sent. The TV takes a few seconds to answer.");
          ctx.client.retryNow();
        },
        (e: unknown) => ctx.say(String(e)),
      ).finally(() => (wakeButton.disabled = false));
    });
    const paintWake = () => {
      const l = ctx.link.get();
      const p = ctx.tv.get().power;
      wakeButton.hidden = l.kind === "open" && p !== "standby";
    };
    off.push(ctx.link.subscribe(paintWake), ctx.tv.subscribe(paintWake));
  }
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
  const power = h("div", { class: "power-row" }, wakeButton, screenButton, powerButton);

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
    text: "This TV has no route for keys yet (remote.md, risk 1), so the pad is hidden: power, sound, apps, inputs and typing still work.",
  }));
  off.push(
    ctx.link.subscribe((l) => {
      noKeys.hidden = !(l.kind === "open" && !l.verbs.has("key"));
    }),
  );

  const el = h(
    "section",
    { class: "remote" },
    h("div", { class: "remote-main" }, noKeys, now, switcher, deck, nav),
    h("div", { class: "remote-side" }, shortcutList, power, playback, more, form),
  );
  off.push(ctx.link.subscribe(() => el.classList.toggle("no-keys", ctx.client.lastVerbs?.has("key") !== true)));
  return {
    el,
    destroy: () => off.forEach((f) => f()),
  };
}

function labelled(button: HTMLButtonElement, text: string): HTMLButtonElement {
  button.append(h("span", { text }));
  return button;
}
