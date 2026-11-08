// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pairing (R4). In the PC app: open pairing (the TV and this screen show a
// six-digit code for two minutes), the paired devices, owner rights given
// or taken back, revoking; then the PC's own settings (the TV's address,
// SSH port and MAC). On a phone, which the TV serves only once it is
// paired: who this phone is and what its role allows, then this device's
// settings (the TV's address and tilt from the phone's motion).
import type { Ctx, Screen } from "../context.ts";
import type { PcHost, PcSettings } from "../host.ts";
import { sshAddress } from "../host.ts";
import { flash, h, setText } from "../dom.ts";
import { icon } from "../icons.ts";
import { deviceTilt, disableDeviceTilt, enableDeviceTilt, reducedMotion, rimLight } from "../motion.ts";
import type { DeviceInfo } from "../protocol.ts";
import { MAX_ATTEMPTS } from "../pairing.ts";

/** The port lumen-remote listens on (tools/remote.sh web start). */
export const WEB_PORT = 8443;

const ROLE_TEXT = {
  remote: "Remote only: power, volume, inputs, apps, keys, typing and the picture and sound modes.",
  owner: "Owner: everything here, privacy and every setting included, as the PC granted.",
} as const;

export function pairingScreen(ctx: Ctx): Screen {
  const off: (() => void)[] = [];
  const pc = ctx.host.pc;
  const card = pc ? pcPairing(ctx, pc, off) : phoneCard(ctx, off);

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
    ...(pc
      ? [pcForm(ctx, pc)]
      : [
          addressForm,
          h("div", { class: "switch-row" }, icon("tilt"), h("span", { id: "tilt-label", text: "Tilt with the phone's motion" }), tiltSwitch),
          tiltNote,
        ]),
  );
  off.push(rimLight(card), rimLight(device));

  const el = h("section", { class: "pairing" }, card, device);
  return { el, destroy: () => off.forEach((f) => f()) };
}

/** A phone's card: who it is to the TV, from lumen-remote's hello. */
function phoneCard(ctx: Ctx, off: (() => void)[]): HTMLElement {
  const name = h("h2", { text: "This phone" });
  const role = h("p", { class: "pair-status", role: "status" });
  const what = h("p", { class: "hint" });
  const card = h(
    "div",
    { class: "pair-card glass" },
    h("div", { class: "card-head" }, icon("pair"), name),
    role,
    what,
    h("p", { class: "hint", text: "To unpair it, revoke it in Lumen Remote on the PC. Owner rights come only from there too." }),
  );
  off.push(
    ctx.link.subscribe((l) => {
      if (l.kind !== "open") {
        card.dataset.state = "unknown";
        setText(role, l.kind === "revoked" ? "Revoked from the PC." : "Not connected to the TV.");
        setText(what, "");
        return;
      }
      if (!l.device || !l.role) {
        card.dataset.state = "unknown";
        setText(name, "Pairing");
        setText(role, "Not paired through the TV: this page didn't come from lumen-remote.");
        setText(what, "On the TV's own address, an unpaired phone gets the pairing page first.");
        return;
      }
      card.dataset.state = "paired";
      setText(name, l.device.name);
      setText(role, l.role === "owner" ? "Paired, with owner rights" : "Paired, remote only");
      setText(what, ROLE_TEXT[l.role]);
    }),
  );
  return card;
}

/** The PC's card: pairing a phone, and the paired devices. */
function pcPairing(ctx: Ctx, pc: PcHost, off: (() => void)[]): HTMLElement {
  const p = ctx.pairing;
  const where = h("li", { text: `On the phone, open the TV's page, https://<the TV>:${WEB_PORT}.` });
  void pc.settings().then(
    (s) => {
      if (s.host) setText(where, `On the phone, open https://${s.host}:${WEB_PORT}.`);
    },
    () => undefined,
  );
  const cells = Array.from({ length: 6 }, () => h("span", { class: "code-cell filled" }));
  const code = h("div", { class: "code pc-code", "aria-label": "The pairing code", role: "group" }, ...cells);
  const left = h("p", { class: "pair-status", role: "status" });
  const openButton = h("button", { class: "pill-button accent", type: "button" }, icon("link"), h("span", { text: "Pair a phone" }));
  const closeButton = h("button", { class: "pill-button", type: "button" }, h("span", { text: "Close pairing" }));
  openButton.addEventListener("click", () => {
    openButton.disabled = true;
    p.open()
      .then(() => flash(code, "pop", 400))
      .catch((e: unknown) => ctx.say(`Pairing didn't open: ${String(e)}`))
      .finally(() => (openButton.disabled = !p.available));
  });
  closeButton.addEventListener("click", () => void p.close().catch((e: unknown) => ctx.say(String(e))));

  const list = h("ul", { class: "device-list", "aria-label": "Paired devices" });
  const empty = h("p", { class: "hint", text: "No phone is paired." });
  const card = h(
    "div",
    { class: "pair-card glass" },
    h("div", { class: "card-head" }, icon("pair"), h("h2", { text: "Pair a phone" })),
    h(
      "ol",
      { class: "steps" },
      h("li", { text: "Choose Pair a phone: the TV and this screen show a six-digit code for two minutes." }),
      where,
      h("li", { text: `Type the code there. ${MAX_ATTEMPTS} wrong codes close pairing.` }),
    ),
    code,
    left,
    h("div", { class: "pair-actions" }, openButton, closeButton),
    h("h3", { class: "field-label", text: "Paired devices" }),
    list,
    empty,
  );

  const row = (d: DeviceInfo): HTMLElement => {
    const owner = d.role === "owner";
    const role = h("button", { class: "pill-button small", type: "button" }, h("span", { text: owner ? "Make remote only" : "Make owner" }));
    role.addEventListener("click", () => {
      role.disabled = true;
      p.setRole(d.id, owner ? "remote" : "owner")
        .then(() => ctx.say(owner ? `${d.name} is remote only now` : `${d.name} has owner rights now`))
        .catch((e: unknown) => ctx.say(String(e)))
        .finally(() => (role.disabled = false));
    });
    // Revoking asks once more, as Turn off does.
    const label = h("span", { text: "Revoke" });
    const revoke = h("button", { class: "pill-button small danger", type: "button" }, label);
    let armed: ReturnType<typeof setTimeout> | null = null;
    revoke.addEventListener("click", () => {
      if (!armed) {
        setText(label, "Revoke? Tap again");
        revoke.classList.add("armed");
        armed = setTimeout(() => {
          armed = null;
          revoke.classList.remove("armed");
          setText(label, "Revoke");
        }, 4000);
        return;
      }
      clearTimeout(armed);
      revoke.disabled = true;
      p.revoke(d.id)
        .then(() => ctx.say(`${d.name} is revoked: its link closed`))
        .catch((e: unknown) => {
          revoke.disabled = false;
          ctx.say(String(e));
        });
    });
    return h(
      "li",
      { class: "device-row", "data-role": d.role, "data-id": d.id },
      h(
        "div",
        { class: "device-what" },
        h("span", { class: "device-name", text: d.name }),
        h("span", { class: "device-meta", text: `${owner ? "Owner" : "Remote only"} · paired ${new Date(d.paired).toLocaleDateString()}` }),
      ),
      role,
      revoke,
    );
  };

  let shown = "";
  let ticker: ReturnType<typeof setInterval> | null = null;
  const paint = () => {
    const v = p.state.get();
    const on = p.available;
    openButton.disabled = !on;
    const w = v.window;
    const secs = p.secondsLeft();
    const open = w !== null && secs > 0;
    card.dataset.state = open ? "open" : "idle";
    code.hidden = !open || !w.code;
    closeButton.hidden = !open;
    const openLabel = openButton.querySelector("span");
    if (openLabel) setText(openLabel, open ? "New code" : "Pair a phone");
    if (open) {
      const digits = (w.code ?? "").split("");
      cells.forEach((c, i) => setText(c, digits[i] ?? ""));
      setText(
        left,
        `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")} left · ${w.attemptsLeft} of ${MAX_ATTEMPTS} tries${w.toasted ? " · shown on the TV" : ""}`,
      );
    } else {
      setText(left, on ? "Pairing is closed." : "Pairing needs the PC's link to the TV.");
    }
    // The list changes rarely; rebuild it only when it does.
    const key = JSON.stringify(v.devices);
    if (key !== shown) {
      shown = key;
      list.replaceChildren(...v.devices.map(row));
    }
    empty.hidden = v.devices.length > 0 || !v.loaded;
    // Still once the window has ended.
    if (!open && ticker) {
      clearInterval(ticker);
      ticker = null;
    }
  };
  off.push(
    p.state.subscribe(() => {
      paint();
      const ticking = p.secondsLeft() > 0;
      if (ticking && !ticker) ticker = setInterval(paint, 1000);
      if (!ticking && ticker) {
        clearInterval(ticker);
        ticker = null;
      }
    }),
    ctx.link.subscribe((l) => {
      if (l.kind === "open" && p.available) void p.refresh().catch(() => undefined);
      paint();
    }),
    () => {
      if (ticker) clearInterval(ticker);
    },
  );
  return card;
}

/** The PC app's settings: where the TV is, and its MAC for Wake-on-LAN.
 * Saved to ~/.lumen/remote/tv.conf by the app, which checks them again. */
function pcForm(ctx: Ctx, pc: PcHost): HTMLElement {
  const field = (id: string, label: string, attrs: Record<string, string>) => {
    const input = h("input", { class: "text-field", id, autocomplete: "off", spellcheck: "false", ...attrs });
    return { input, el: h("div", { class: "pc-field" }, h("label", { for: id, class: "field-label", text: label }), input) };
  };
  const host = field("tv-host", "The TV's address", { inputmode: "url", placeholder: "192.168.86.20" });
  const port = field("tv-port", "SSH port", { inputmode: "numeric", placeholder: "22" });
  const mac = field("tv-mac", "MAC address, for Turn on", { placeholder: "a8:23:fe:01:02:03" });
  const key = h("p", { class: "hint" });
  const save = h("button", { class: "pill-button accent", type: "submit" }, h("span", { text: "Save and connect" }));
  const fill = (s: PcSettings) => {
    host.input.value = s.host ?? "";
    port.input.value = String(s.port);
    mac.input.value = s.mac ?? "";
    setText(
      key,
      s.keyFound
        ? `Key: ${s.key}. ssh: ${s.ssh}.`
        : `No key at ${s.key}: tools/remote.sh keygen makes one, and tools/remote.sh install puts it on the TV.`,
    );
    key.classList.toggle("bad", !s.keyFound);
  };
  void pc.settings().then(fill, (e: unknown) => ctx.say(String(e)));
  const form = h(
    "form",
    { class: "address-form pc-form" },
    h("div", { class: "pc-fields" }, host.el, port.el, mac.el),
    key,
    save,
    h("p", {
      class: "hint",
      text: "The app reaches lumen-ctl over SSH with its own key, which runs nothing else on the TV. It talks to nothing but the TV.",
    }),
  );
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    save.disabled = true;
    pc.saveSettings({ host: host.input.value, port: port.input.value || "22", mac: mac.input.value })
      .then(
        (s) => {
          fill(s);
          ctx.client.setUrl(sshAddress(s));
          ctx.say(`Saved. Connecting to ${s.host ?? "the TV"}`);
        },
        (err: unknown) => ctx.say(String(err)),
      )
      .finally(() => (save.disabled = false));
  });
  return form;
}
