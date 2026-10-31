// SPDX-License-Identifier: GPL-3.0-or-later
//
// The link to lumen-ctl: one WebSocket, reconnected with backoff, with
// requests matched to replies by id and events passed on as they come.
//
// A request is never queued while the link is down: a key press that reached
// the TV ten seconds late would do the wrong thing, so it fails at once and
// the UI says so. The link counts as stale when nothing has arrived for
// `staleMs` (the server sends `alive` every 10 s), and is dropped and
// reconnected after `deadMs`.
import {
  PROTOCOL_VERSION,
  MAX_REQUEST_ID,
  checkResult,
  encode,
  parseServerLine,
  splitLines,
} from "./protocol.ts";
import type { ArgsOf, ErrorCode, Event, Request, ResultOf, Verb } from "./protocol.ts";

/** What the client needs of a WebSocket; Tauri (R3) can hand it another. */
export interface SocketLike {
  onopen: (() => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}
export type SocketFactory = (url: string) => SocketLike;

export type LinkState =
  | { kind: "connecting"; attempt: number }
  | { kind: "open"; server: string; verbs: ReadonlySet<Verb>; stale: boolean }
  | { kind: "waiting"; attempt: number; retryAt: number; reason: string }
  | { kind: "incompatible"; protocol: number }
  | { kind: "paused" };

/** Why a request failed: the server's error code, or the link's. */
export type FailCode = ErrorCode | "not-connected" | "timeout" | "disconnected" | "bad-reply";

export class RequestError extends Error {
  readonly code: FailCode;
  constructor(code: FailCode, message: string) {
    super(message);
    this.name = "RequestError";
    this.code = code;
  }
}

export interface ClientOptions {
  url: string;
  socket?: SocketFactory;
  random?: () => number;
  now?: () => number;
  requestTimeoutMs?: number;
  helloTimeoutMs?: number;
  staleMs?: number;
  deadMs?: number;
  backoffBaseMs?: number;
  backoffMaxMs?: number;
}

/** A verb whose arguments are all optional can be called without them. */
type ArgList<V extends Verb> = Record<never, never> extends ArgsOf<V> ? [] | [ArgsOf<V>] : [ArgsOf<V>];

interface Pending {
  verb: Verb;
  resolve: (result: unknown) => void;
  reject: (err: RequestError) => void;
  timer: ReturnType<typeof setTimeout>;
}

const defaultSocket: SocketFactory = (url) => new WebSocket(url) as unknown as SocketLike;

/** The delay before reconnect attempt `attempt` (0-based): doubling from
 * `base` up to `max`, then a random half of it taken off so that two
 * clients don't retry in step. */
export function backoffDelay(attempt: number, base: number, max: number, random: number): number {
  const full = Math.min(max, base * 2 ** Math.min(attempt, 30));
  return Math.round(full / 2 + (full / 2) * random);
}

export class RemoteClient {
  private url: string;
  private readonly socketFactory: SocketFactory;
  private readonly random: () => number;
  private readonly now: () => number;
  private readonly requestTimeoutMs: number;
  private readonly helloTimeoutMs: number;
  private readonly staleMs: number;
  private readonly deadMs: number;
  private readonly backoffBaseMs: number;
  private readonly backoffMaxMs: number;

  private ws: SocketLike | null = null;
  private current: LinkState = { kind: "paused" };
  private attempt = 0;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly stateListeners = new Set<(s: LinkState) => void>();
  private readonly eventListeners = new Set<(e: Event) => void>();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private helloTimer: ReturnType<typeof setTimeout> | null = null;
  private staleTimer: ReturnType<typeof setTimeout> | null = null;
  private deadTimer: ReturnType<typeof setTimeout> | null = null;
  private running = false;

  constructor(opts: ClientOptions) {
    this.url = opts.url;
    this.socketFactory = opts.socket ?? defaultSocket;
    this.random = opts.random ?? Math.random;
    this.now = opts.now ?? Date.now;
    this.requestTimeoutMs = opts.requestTimeoutMs ?? 5_000;
    this.helloTimeoutMs = opts.helloTimeoutMs ?? 5_000;
    this.staleMs = opts.staleMs ?? 25_000;
    this.deadMs = opts.deadMs ?? 45_000;
    this.backoffBaseMs = opts.backoffBaseMs ?? 500;
    this.backoffMaxMs = opts.backoffMaxMs ?? 15_000;
  }

  get state(): LinkState {
    return this.current;
  }

  get address(): string {
    return this.url;
  }

  /** Whether the link is open and the server accepts `verb`. */
  supports(verb: Verb): boolean {
    return this.current.kind === "open" && this.current.verbs.has(verb);
  }

  onState(fn: (s: LinkState) => void): () => void {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }

  onEvent(fn: (e: Event) => void): () => void {
    this.eventListeners.add(fn);
    return () => this.eventListeners.delete(fn);
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.attempt = 0;
    this.connect();
  }

  /** Closes the link and stays closed until `start`, as when the page hides. */
  pause(): void {
    this.running = false;
    this.teardown("paused");
    this.setState({ kind: "paused" });
  }

  /** Tries again now instead of waiting out the backoff. */
  retryNow(): void {
    if (!this.running) {
      this.start();
    } else if (this.current.kind === "waiting" || this.current.kind === "incompatible") {
      this.attempt = 0;
      this.connect();
    }
  }

  setUrl(url: string): void {
    this.url = url;
    if (this.running) {
      this.teardown("address changed");
      this.attempt = 0;
      this.connect();
    }
  }

  request<V extends Verb>(verb: V, ...rest: ArgList<V>): Promise<ResultOf<V>> {
    return this.call(verb, rest[0]);
  }

  /** `request` for a caller that holds the arguments as one value. */
  call<V extends Verb>(verb: V, maybeArgs: ArgsOf<V> | undefined): Promise<ResultOf<V>> {
    const args = maybeArgs ?? {};
    const ws = this.ws;
    if (!ws || this.current.kind !== "open") {
      return Promise.reject(new RequestError("not-connected", "not connected to the TV"));
    }
    const id = this.nextId;
    this.nextId = this.nextId >= MAX_REQUEST_ID ? 1 : this.nextId + 1;
    return new Promise<ResultOf<V>>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new RequestError("timeout", `no answer to ${verb} in ${this.requestTimeoutMs / 1000} s`));
      }, this.requestTimeoutMs);
      this.pending.set(id, {
        verb,
        resolve: resolve as (r: unknown) => void,
        reject,
        timer,
      });
      ws.send(encode({ id, verb, args } as Request));
    });
  }

  private setState(next: LinkState): void {
    this.current = next;
    for (const fn of this.stateListeners) fn(next);
  }

  private connect(): void {
    this.clearTimers();
    this.setState({ kind: "connecting", attempt: this.attempt });
    let ws: SocketLike;
    try {
      ws = this.socketFactory(this.url);
    } catch {
      this.scheduleRetry("the address isn't a WebSocket URL");
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      if (this.ws !== ws) return;
      this.helloTimer = setTimeout(() => this.drop(ws, "no hello from the TV"), this.helloTimeoutMs);
    };
    ws.onmessage = (ev) => {
      if (this.ws !== ws || typeof ev.data !== "string") return;
      for (const line of splitLines(ev.data)) this.receive(ws, line);
    };
    ws.onerror = () => {
      // onclose follows and does the work.
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.failPending("disconnected", "the link to the TV closed");
      this.scheduleRetry("the link closed");
    };
  }

  private receive(ws: SocketLike, line: string): void {
    const msg = parseServerLine(line);
    if (!msg) return;
    this.armWatchdog(ws);
    if ("event" in msg) {
      if (msg.event === "hello") {
        this.clearHello();
        if (msg.data.protocol !== PROTOCOL_VERSION) {
          this.teardown("incompatible");
          this.setState({ kind: "incompatible", protocol: msg.data.protocol });
          return;
        }
        this.attempt = 0;
        this.setState({
          kind: "open",
          server: msg.data.server,
          verbs: new Set(msg.data.verbs),
          stale: false,
        });
      } else if (this.current.kind === "open" && this.current.stale) {
        this.setState({ ...this.current, stale: false });
      }
      for (const fn of this.eventListeners) fn(msg);
      return;
    }
    if (this.current.kind === "open" && this.current.stale) {
      this.setState({ ...this.current, stale: false });
    }
    if (msg.id === null) return;
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    clearTimeout(p.timer);
    if (!msg.ok) {
      p.reject(new RequestError(msg.error.code, msg.error.message));
      return;
    }
    const result = checkResult(p.verb, msg.result);
    if (result === null) p.reject(new RequestError("bad-reply", `the TV's answer to ${p.verb} had the wrong shape`));
    else p.resolve(result);
  }

  private armWatchdog(ws: SocketLike): void {
    if (this.staleTimer) clearTimeout(this.staleTimer);
    if (this.deadTimer) clearTimeout(this.deadTimer);
    this.staleTimer = setTimeout(() => {
      if (this.ws === ws && this.current.kind === "open") this.setState({ ...this.current, stale: true });
    }, this.staleMs);
    this.deadTimer = setTimeout(() => this.drop(ws, "no word from the TV"), this.deadMs);
  }

  /** Gives up on a socket that has gone quiet and reconnects. */
  private drop(ws: SocketLike, reason: string): void {
    if (this.ws !== ws) return;
    this.ws = null;
    ws.close(4000, reason);
    this.failPending("disconnected", reason);
    this.scheduleRetry(reason);
  }

  private scheduleRetry(reason: string): void {
    this.clearTimers();
    if (!this.running) return;
    const delay = backoffDelay(this.attempt, this.backoffBaseMs, this.backoffMaxMs, this.random());
    this.setState({ kind: "waiting", attempt: this.attempt, retryAt: this.now() + delay, reason });
    this.attempt++;
    this.retryTimer = setTimeout(() => this.connect(), delay);
  }

  private teardown(reason: string): void {
    this.clearTimers();
    const ws = this.ws;
    this.ws = null;
    if (ws) ws.close(1000, reason);
    this.failPending("disconnected", reason);
  }

  private failPending(code: FailCode, message: string): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new RequestError(code, message));
    }
    this.pending.clear();
  }

  private clearHello(): void {
    if (this.helloTimer) clearTimeout(this.helloTimer);
    this.helloTimer = null;
  }

  private clearTimers(): void {
    this.clearHello();
    for (const t of [this.retryTimer, this.staleTimer, this.deadTimer]) if (t) clearTimeout(t);
    this.retryTimer = this.staleTimer = this.deadTimer = null;
  }
}
