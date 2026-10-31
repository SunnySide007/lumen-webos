// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lumen Remote's page: the frame around the screens (navigation, the status
// line, the snackbar), the link to lumen-ctl, and the TV's state.
import "./styles/fonts.css";
import "./styles/base.css";
import "./styles/controls.css";
import "./styles/screens.css";
import { RemoteClient } from "./client.ts";
import type { LinkState } from "./client.ts";
import { failureText } from "./context.ts";
import type { Ctx, Screen, SnackAction } from "./context.ts";
import { h, setText, flash } from "./dom.ts";
import { icon, muteIcon } from "./icons.ts";
import type { IconName } from "./icons.ts";
import { POWER_LABELS } from "./labels.ts";
import { detectRefraction, installSprings, morph, rimLight } from "./motion.ts";
import { PairingStub } from "./pairing.ts";
import { EMPTY_STATE, Store, ambientColor, applyEvent, frontApp, frontTitle } from "./store.ts";
import type { TvState } from "./store.ts";
import { appsScreen } from "./screens/apps.ts";
import { lumenScreen } from "./screens/lumen.ts";
import { pairingScreen } from "./screens/pairing.ts";
import { pictureScreen } from "./screens/picture.ts";
import { remoteScreen } from "./screens/remote.ts";
import { soundScreen } from "./screens/sound.ts";

interface Route {
  id: string;
  title: string;
  short: string;
  icon: IconName;
  make: (ctx: Ctx) => Screen;
}

const ROUTES: Route[] = [
  { id: "remote", title: "Remote", short: "Remote", icon: "remote", make: remoteScreen },
  { id: "sound", title: "Sound", short: "Sound", icon: "sound", make: soundScreen },
  { id: "apps", title: "Apps and inputs", short: "Apps", icon: "apps", make: appsScreen },
  { id: "picture", title: "Picture and sound", short: "Picture", icon: "picture", make: pictureScreen },
  { id: "lumen", title: "Lumen", short: "Lumen", icon: "lumen", make: lumenScreen },
  { id: "pairing", title: "Pairing", short: "Pairing", icon: "pair", make: pairingScreen },
];

/** The WebSocket: ?ctl=… when given (the PC app, tests), else /ctl on the
 * host that served this page (the TV, or the mock). */
export function defaultAddress(loc: Location): string {
  const given = new URLSearchParams(loc.search).get("ctl");
  if (given) return given;
  return `${loc.protocol === "https:" ? "wss" : "ws"}://${loc.host}/ctl`;
}

function boot(): void {
  installSprings();
  detectRefraction();

  const client = new RemoteClient({ url: defaultAddress(location) });
  const tv = new Store<TvState>(EMPTY_STATE);
  const link = new Store<LinkState>(client.state);
  const pairing = new PairingStub();
  client.onState((s) => link.set(s));
  client.onEvent((e) => {
    if (e.event === "hello") void refreshApps();
    tv.update((s) => applyEvent(s, e));
  });

  async function refreshApps(): Promise<void> {
    try {
      const { apps } = await client.request("apps.list");
      tv.update((s) => ({ ...s, apps }));
    } catch {
      // The Apps screen asks again and says why if it fails there.
    }
  }

  const snack = makeSnack();
  const ctx: Ctx = {
    client,
    tv,
    link,
    pairing,
    async send(verb, args) {
      try {
        return await client.call(verb, args);
      } catch (err) {
        snack.say(failureText(verb, err));
        return null;
      }
    },
    say: (message, action) => snack.say(message, action),
  };

  // The ambient colour: whatever is in front on the TV tints the page.
  tv.subscribe((s) => document.documentElement.style.setProperty("--ambient", ambientColor(s)));

  const nav = h("nav", { class: "nav glass refract", "aria-label": "Screens" });
  const links = new Map<string, HTMLAnchorElement>();
  for (const r of ROUTES) {
    const a = h(
      "a",
      { href: `#/${r.id}`, class: "nav-link", "data-route": r.id, title: r.title },
      h("span", { class: "nav-pill", "aria-hidden": "true" }),
      icon(r.icon),
      h("span", { class: "nav-label", text: r.short }),
    );
    links.set(r.id, a);
    nav.append(a);
  }
  nav.prepend(
    h("div", { class: "brand" }, h("span", { class: "brand-mark", "aria-hidden": "true" }), h("span", { text: "Lumen" })),
  );

  const title = h("h1", { class: "screen-title" });
  const header = h("header", { class: "topbar" }, title, statusLine(tv, link, client));
  const main = h("main", { class: "screen", id: "screen", tabindex: "-1" });
  const app = document.getElementById("app");
  if (!app) throw new Error("no #app");
  app.className = "app";
  app.append(header, main, nav, snack.el);
  rimLight(nav);

  let current: { route: Route; screen: Screen } | null = null;
  const show = () => {
    const id = location.hash.replace(/^#\/?/, "") || "remote";
    const route = ROUTES.find((r) => r.id === id) ?? ROUTES[0]!;
    if (current?.route === route) return;
    morph(() => {
      current?.screen.destroy();
      const screen = route.make(ctx);
      main.replaceChildren(screen.el);
      setText(title, route.title);
      document.title = `${route.title} · Lumen Remote`;
      for (const [rid, a] of links) {
        if (rid === route.id) a.setAttribute("aria-current", "page");
        else a.removeAttribute("aria-current");
      }
      current = { route, screen };
      screen.shown?.();
    });
  };
  window.addEventListener("hashchange", show);
  show();

  // A hidden page (the phone's screen off, the tray popup closed) lets the
  // link go after a while instead of keeping the radio awake for it.
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      hideTimer = setTimeout(() => client.pause(), 15_000);
    } else {
      if (hideTimer) clearTimeout(hideTimer);
      if (client.state.kind === "paused") client.start();
    }
  });

  client.start();
}

/** The line at the top: what's in front, the volume, the TV's power and the
 * link. It follows pushed events on every screen. */
function statusLine(tv: Store<TvState>, link: Store<LinkState>, client: RemoteClient): HTMLElement {
  const dot = h("span", { class: "front-dot", "aria-hidden": "true" });
  const front = h("span", { class: "chip front", title: "In front on the TV" }, dot, h("span", { class: "front-name" }));
  const volNum = h("span", { class: "vol-num" });
  const vol = h("span", { class: "chip vol", title: "Volume" }, muteIcon(), volNum);
  const power = h("span", { class: "chip power" }, icon("tv"), h("span", { class: "power-label" }));
  const linkText = h("span", { class: "link-text" });
  const linkChip = h(
    "button",
    { class: "chip link", type: "button", onclick: () => client.retryNow() },
    h("span", { class: "link-dot", "aria-hidden": "true" }),
    linkText,
  );

  tv.subscribe((s, before) => {
    const name = frontTitle(s);
    front.hidden = name === null;
    setText(front.querySelector(".front-name")!, name ?? "");
    dot.style.setProperty("--dot", frontApp(s)?.color ?? "var(--dim)");
    vol.hidden = s.volume === null;
    if (s.volume) {
      setText(volNum, String(s.volume.level));
      vol.classList.toggle("muted", s.volume.muted);
      vol.setAttribute("aria-label", `Volume ${s.volume.level}${s.volume.muted ? ", muted" : ""}`);
      if (before.volume && before.volume.level !== s.volume.level) flash(vol, "bump", 400);
    }
    power.hidden = s.power === null || s.power === "active";
    setText(power.querySelector(".power-label")!, s.power ? POWER_LABELS[s.power] : "");
  });

  let ticker: ReturnType<typeof setInterval> | null = null;
  let staleSince = 0;
  const paint = () => {
    const s = link.get();
    let text: string;
    let tone: string;
    switch (s.kind) {
      case "connecting":
        text = s.attempt === 0 ? "Connecting…" : "Reconnecting…";
        tone = "busy";
        break;
      case "open":
        if (s.stale) {
          text = `Stale · ${Math.round((Date.now() - staleSince) / 1000)} s`;
          tone = "stale";
        } else {
          text = "Connected";
          tone = "ok";
        }
        break;
      case "waiting":
        text = `Retry in ${Math.max(0, Math.ceil((s.retryAt - Date.now()) / 1000))} s`;
        tone = "bad";
        break;
      case "incompatible":
        text = `Protocol ${s.protocol}: update`;
        tone = "bad";
        break;
      case "paused":
        text = "Paused";
        tone = "busy";
        break;
    }
    setText(linkText, text);
    linkChip.dataset.tone = tone;
    linkChip.title =
      s.kind === "waiting"
        ? `${s.reason}. Click to retry now.`
        : s.kind === "open"
          ? `${s.server} at ${client.address}`
          : client.address;
    linkChip.setAttribute("aria-label", `Link to the TV: ${text}`);
  };
  link.subscribe((s, before) => {
    if (s.kind === "open" && s.stale && !(before.kind === "open" && before.stale)) {
      staleSince = Date.now() - 25_000;
    }
    const ticking = s.kind === "waiting" || (s.kind === "open" && s.stale);
    if (ticking && !ticker) ticker = setInterval(paint, 1000);
    if (!ticking && ticker) {
      clearInterval(ticker);
      ticker = null;
    }
    paint();
  });

  return h("div", { class: "status" }, h("div", { class: "status-tv" }, front, vol, power), linkChip);
}

function makeSnack(): { el: HTMLElement; say(message: string, action?: SnackAction): void } {
  const text = h("span", { class: "snack-text" });
  const button = h("button", { class: "snack-action", type: "button" });
  const el = h("div", { class: "snack glass", role: "status", "aria-live": "polite" }, text, button);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let run: (() => void) | null = null;
  button.addEventListener("click", () => {
    run?.();
    el.classList.remove("show");
  });
  return {
    el,
    say(message, action) {
      setText(text, message);
      button.hidden = !action;
      setText(button, action?.label ?? "");
      run = action?.run ?? null;
      el.classList.add("show");
      flash(el, "pop", 400);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => el.classList.remove("show"), action ? 6000 : 3500);
    },
  };
}

boot();
