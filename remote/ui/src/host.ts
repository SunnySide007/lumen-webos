// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the page runs in: a browser (the TV's server in R4, the mock) or the
// PC app (R3, Tauri). In the PC app the link to lumen-ctl is an SSH session
// the app runs (remote/app, lumen-remote-link), handed to RemoteClient as a
// SocketLike: one JSON line per message each way, as over the WebSocket, so
// nothing else in the page changes. The app also turns the TV on by
// Wake-on-LAN and keeps the TV's address in ~/.lumen/remote/tv.conf.
import type { SocketFactory, SocketLike } from "./client.ts";

/** The PC app's settings, as `settings_get` gives them. */
export interface PcSettings {
  host: string | null;
  port: number;
  mac: string | null;
  /** Where the app's key is, and whether it is there. */
  key: string;
  keyFound: boolean;
  ssh: string;
}

/** The app's three windows: the popup above the tray icon, the full window,
 * and the thin remote at the side of the desktop. */
export type PcWindow = "popup" | "main" | "side";

export interface PcHost {
  readonly window: PcWindow;
  wake(): Promise<void>;
  /** Shows the full window, in front. */
  openMain(): Promise<void>;
  /** Hides this page's window; the tray icon shows it again. */
  hide(): Promise<void>;
  settings(): Promise<PcSettings>;
  saveSettings(s: { host: string; port: string; mac: string }): Promise<PcSettings>;
}

export interface Host {
  /** The address the client is given, and shows. */
  address: string;
  socket?: SocketFactory;
  helloTimeoutMs?: number;
  /** How long a hidden page keeps its link. */
  hideGraceMs: number;
  /** Called when the page's window shows or hides. */
  onVisible(fn: (visible: boolean) => void): void;
  pc: PcHost | null;
}

// ------------------------------------------------------------ Tauri's globals

/** The part of `window.__TAURI__` (withGlobalTauri) this page uses. */
export interface TauriChannel<T> {
  onmessage: (message: T) => void;
}
export interface TauriGlobal {
  core: {
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    Channel: new <T>() => TauriChannel<T>;
  };
  event: {
    /** `target` a window's label: only events sent to that window. Without
     * it, Tauri passes on events sent to any window. */
    listen: <T>(name: string, fn: (e: { payload: T }) => void, options?: { target: string }) => Promise<() => void>;
  };
}

/** What the app sends down a session's channel (`LinkMsg` in main.rs). */
export type LinkMsg = { kind: "open" } | { kind: "line"; line: string } | { kind: "closed"; reason: string };

export function tauriGlobal(): TauriGlobal | null {
  return (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__ ?? null;
}

/** The address shown for the PC app's link. */
export function sshAddress(s: Pick<PcSettings, "host" | "port">): string {
  return s.host ? `ssh://root@${s.host}${s.port === 22 ? "" : `:${s.port}`}` : "ssh: no TV set";
}

/** A SocketLike over one SSH session of the app's. Lines that the page
 * sends before the app has named the session wait for its name, a few
 * milliseconds; nothing is sent once it has closed. */
export function sshSocket(tauri: TauriGlobal): SocketFactory {
  return () => {
    const { invoke, Channel } = tauri.core;
    let id: number | null = null;
    let closed = false;
    const early: string[] = [];
    const channel = new Channel<LinkMsg>();
    /** The session is over: `ended` when the app said so, else this side
     * gives up on it and tells the app. */
    const end = (reason: string, ended = false) => {
      if (closed) return;
      closed = true;
      if (!ended && id !== null) void invoke("link_close", { id }).catch(() => undefined);
      socket.onclose?.({ reason });
    };
    const socket: SocketLike = {
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
      send(data) {
        if (closed) return;
        // RemoteClient's lines carry their own "\n"; the app adds one.
        const line = data.replace(/\r?\n$/, "");
        if (id === null) early.push(line);
        else void invoke("link_send", { id, line }).catch((e: unknown) => end(String(e)));
      },
      close() {
        if (closed) return;
        closed = true;
        if (id !== null) void invoke("link_close", { id }).catch(() => undefined);
      },
    };
    channel.onmessage = (m) => {
      if (closed) return;
      if (m.kind === "open") socket.onopen?.();
      else if (m.kind === "line") socket.onmessage?.({ data: m.line });
      else end(m.reason, true);
    };
    invoke<number>("link_open", { onEvent: channel }).then(
      (n) => {
        id = n;
        if (closed) {
          void invoke("link_close", { id }).catch(() => undefined);
          return;
        }
        for (const line of early.splice(0)) {
          void invoke("link_send", { id, line }).catch((e: unknown) => end(String(e)));
        }
      },
      (e: unknown) => end(String(e), true),
    );
    return socket;
  };
}

export function pcHost(tauri: TauriGlobal, settings: PcSettings, window: PcWindow): Host {
  const { invoke } = tauri.core;
  return {
    address: sshAddress(settings),
    socket: sshSocket(tauri),
    // ssh has its own handshake before lumen-ctl's hello (ConnectTimeout=8).
    helloTimeoutMs: 15_000,
    // The popup comes and goes; a link kept a minute makes reopening it warm.
    hideGraceMs: 60_000,
    onVisible(fn) {
      const listening = tauri.event.listen<boolean>(
        "lumen-visibility",
        (e) => {
          fn(e.payload);
          if (e.payload && window === "popup") {
            // The timing line: two frames after showing, the page has painted.
            requestAnimationFrame(() => requestAnimationFrame(() => void invoke("popup_painted").catch(() => undefined)));
          }
        },
        { target: window },
      );
      // Shown before this page had loaded (the first run opens the window).
      void listening
        .then(() => invoke<boolean>("window_visible"))
        .then((v) => v && fn(true))
        .catch(() => undefined);
    },
    pc: {
      window,
      wake: () => invoke<void>("wake"),
      openMain: () => invoke<void>("open_window"),
      hide: () => invoke<void>("hide_window"),
      settings: () => invoke<PcSettings>("settings_get"),
      saveSettings: (s) => invoke<PcSettings>("settings_set", s),
    },
  };
}

/** The WebSocket: ?ctl=… when given (tests), else /ctl on the host that
 * served this page (the TV, or the mock). */
export function defaultAddress(loc: Pick<Location, "search" | "protocol" | "host">): string {
  const given = new URLSearchParams(loc.search).get("ctl");
  if (given) return given;
  return `${loc.protocol === "https:" ? "wss" : "ws"}://${loc.host}/ctl`;
}

export function browserHost(loc: Location, doc: Document): Host {
  return {
    address: defaultAddress(loc),
    // A phone with its screen off lets the radio sleep.
    hideGraceMs: 15_000,
    onVisible(fn) {
      doc.addEventListener("visibilitychange", () => fn(!doc.hidden));
    },
    pc: null,
  };
}

/** The host this page runs in. */
export async function detectHost(loc: Location, doc: Document): Promise<Host> {
  const tauri = tauriGlobal();
  if (!tauri) return browserHost(loc, doc);
  const asked = new URLSearchParams(loc.search).get("window");
  const window: PcWindow = asked === "popup" || asked === "side" ? asked : "main";
  const settings = await tauri.core.invoke<PcSettings>("settings_get").catch(
    (): PcSettings => ({ host: null, port: 22, mac: null, key: "", keyFound: false, ssh: "" }),
  );
  return pcHost(tauri, settings, window);
}
