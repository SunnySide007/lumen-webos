// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pairing: the phone's side of R4's pairing, as a stub (pairing.ts), and this
// device's settings: the TV's address and tilt from the phone's motion.
import type { Ctx, Screen } from "../context.ts";
import { flash, h, setText } from "../dom.ts";
import { icon } from "../icons.ts";
import { deviceTilt, disableDeviceTilt, enableDeviceTilt, reducedMotion, rimLight } from "../motion.ts";
import { MAX_ATTEMPTS, STUB_WRONG_CODE } from "../pairing.ts";

export function pairingScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];

  // Six cells over one real input, so typing, pasting and the phone's
  // one-time-code suggestion all work.
  const cells = Array.from({ length: 6 }, () => h("span", { class: "code-cell", "aria-hidden": "true" }));
  const code = h("input", {
    class: "code-input",
    id: "pair-code",
    inputmode: "numeric",
    autocomplete: "one-time-code",
    pattern: "[0-9]{6}",
    maxlength: "6",
    "aria-label": "Pairing code, six digits",
  });
  const codeBox = h("div", { class: "code" }, ...cells, code);
  const pairButton = h("button", { class: "pill-button accent", type: "submit" }, icon("link"), h("span", { text: "Pair" }));
  const status = h("p", { class: "pair-status", role: "status" });
  const forget = h("button", { class: "pill-button", type: "button" }, h("span", { text: "Forget this pairing" }));
  forget.addEventListener("click", () => ctx.pairing.forget());

  const paintCells = () => {
    const digits = code.value.replace(/\D/g, "").slice(0, 6);
    if (digits !== code.value) code.value = digits;
    cells.forEach((c, i) => {
      setText(c, digits[i] ?? "");
      c.classList.toggle("filled", i < digits.length);
      c.classList.toggle("caret", i === Math.min(digits.length, 5) && document.activeElement === code);
    });
    pairButton.disabled = digits.length !== 6 || ctx.pairing.state.get().kind !== "unpaired";
  };
  for (const ev of ["input", "focus", "blur"]) code.addEventListener(ev, paintCells);

  const form = h(
    "form",
    { class: "pair-form" },
    h("label", { for: "pair-code", class: "field-label", text: "The code on the TV" }),
    codeBox,
    pairButton,
  );
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    void ctx.pairing.pair(code.value);
  });

  const card = h(
    "div",
    { class: "pair-card glass" },
    h("div", { class: "card-head" }, icon("pair"), h("h2", { text: "Pair this phone" })),
    h(
      "ol",
      { class: "steps" },
      h("li", { text: "On the PC, open Lumen Remote and choose Pair a phone." }),
      h("li", { text: "The TV shows a six-digit code for two minutes." }),
      h("li", { text: "Type it here. Five wrong codes close pairing." }),
    ),
    form,
    status,
    forget,
    h("p", {
      class: "stub-note",
      text: `A stub until R4: nothing on the TV checks the code yet. Any six digits pair, except ${STUB_WRONG_CODE}, which stands for a wrong one. The token stays in this page's memory only.`,
    }),
  );

  off.push(
    ctx.pairing.state.subscribe((s, before) => {
      card.dataset.state = s.kind;
      form.hidden = s.kind === "paired" || s.kind === "closed";
      forget.hidden = s.kind !== "paired";
      code.disabled = s.kind === "checking";
      switch (s.kind) {
        case "unpaired":
          setText(
            status,
            s.wrong ? `That code is wrong. ${s.attemptsLeft} of ${MAX_ATTEMPTS} tries left.` : "Not paired.",
          );
          if (s.wrong && before.kind === "checking") {
            flash(codeBox, "shake", 500);
            code.value = "";
          }
          break;
        case "checking":
          setText(status, "Checking the code…");
          break;
        case "paired":
          setText(status, `Paired at ${s.since.toLocaleTimeString()}. Closing this page forgets the token.`);
          code.value = "";
          break;
        case "closed":
          setText(status, "Pairing closed after five wrong codes. Open it again from the PC app.");
          break;
      }
      paintCells();
    }),
  );

  // This device.
  const address = h("input", {
    class: "text-field",
    id: "tv-address",
    type: "url",
    inputmode: "url",
    autocomplete: "off",
    spellcheck: "false",
    value: ctx.client.address,
  });
  const connect = h("button", { class: "pill-button", type: "submit" }, h("span", { text: "Connect" }));
  const addressForm = h(
    "form",
    { class: "address-form" },
    h("label", { for: "tv-address", class: "field-label", text: "The TV's address" }),
    h("div", { class: "type-row" }, address, connect),
    h("p", { class: "hint", text: "The page's own host when the TV serves it; type another to reach a TV (or the mock) elsewhere." }),
  );
  addressForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const url = address.value.trim();
    if (!/^wss?:\/\/[^/\s]+/.test(url)) {
      ctx.say("The address is a ws:// or wss:// URL");
      return;
    }
    ctx.client.setUrl(url);
    ctx.say(`Connecting to ${url}`);
  });

  const tiltSwitch = h(
    "button",
    { class: "switch", type: "button", role: "switch", "aria-checked": "false", "aria-labelledby": "tilt-label" },
    h("span", { class: "switch-knob", "aria-hidden": "true" }),
  );
  const tiltNote = h("p", { class: "hint" });
  tiltSwitch.addEventListener("click", () => {
    if (deviceTilt.get() === "on") disableDeviceTilt();
    else void enableDeviceTilt();
  });
  const paintTilt = () => {
    const t = deviceTilt.get();
    const reduced = reducedMotion.get();
    tiltSwitch.setAttribute("aria-checked", String(t === "on"));
    tiltSwitch.disabled = t === "unsupported" || reduced;
    setText(
      tiltNote,
      reduced
        ? "Off: this device asks for reduced motion."
        : t === "unsupported"
          ? "This browser has no motion sensors."
          : t === "denied"
            ? "The browser refused the motion sensors."
            : "Leans the pad and the tiles as the phone moves. Off by default; uses the motion sensors only while on.",
    );
  };
  off.push(deviceTilt.subscribe(paintTilt), reducedMotion.subscribe(paintTilt));

  const device = h(
    "div",
    { class: "device glass" },
    h("div", { class: "card-head" }, icon("tv"), h("h2", { text: "This device" })),
    addressForm,
    h("div", { class: "switch-row" }, icon("tilt"), h("span", { id: "tilt-label", text: "Tilt with the phone's motion" }), tiltSwitch),
    tiltNote,
  );
  off.push(rimLight(card), rimLight(device));

  const el = h("section", { class: "pairing" }, card, device);
  return { el, destroy: () => off.forEach((f) => f()) };
}
