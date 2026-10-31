// SPDX-License-Identifier: GPL-3.0-or-later
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteClient, RequestError, backoffDelay } from "../src/client.ts";
import type { LinkState, SocketLike } from "../src/client.ts";
import type { Event } from "../src/protocol.ts";

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: unknown[] = [];
  closed: { code?: number; reason?: string } | null = null;
  readonly url: string;
  constructor(url: string) {
    this.url = url;
  }
  send(data: string): void {
    this.sent.push(JSON.parse(data));
  }
  close(code?: number, reason?: string): void {
    this.closed = { code, reason };
  }
  // The server's side.
  open(): void {
    this.onopen?.();
  }
  push(...lines: unknown[]): void {
    this.onmessage?.({ data: lines.map((l) => JSON.stringify(l)).join("\n") });
  }
  hello(verbs = ["volume.set", "volume.up", "apps.list", "key"], protocol = 1): void {
    this.open();
    this.push({ event: "hello", data: { protocol, server: "fake 1", verbs } });
  }
  drop(): void {
    this.onclose?.();
  }
}

let sockets: FakeSocket[];
let states: LinkState[];
let client: RemoteClient;

function make(opts: Partial<ConstructorParameters<typeof RemoteClient>[0]> = {}): RemoteClient {
  const c = new RemoteClient({
    url: "ws://tv/ctl",
    socket: (url) => {
      const s = new FakeSocket(url);
      sockets.push(s);
      return s;
    },
    random: () => 1,
    ...opts,
  });
  c.onState((s) => states.push(s));
  return c;
}

const last = <T>(xs: T[]): T => {
  const x = xs[xs.length - 1];
  if (x === undefined) throw new Error("empty");
  return x;
};

beforeEach(() => {
  vi.useFakeTimers();
  sockets = [];
  states = [];
  client = make();
});

afterEach(() => {
  client.pause();
  vi.useRealTimers();
});

describe("backoffDelay", () => {
  it("doubles from the base to the cap, less up to half at random", () => {
    expect([0, 1, 2, 3, 4, 5, 6].map((a) => backoffDelay(a, 500, 15_000, 1))).toEqual([
      500, 1000, 2000, 4000, 8000, 15_000, 15_000,
    ]);
    expect(backoffDelay(2, 500, 15_000, 0)).toBe(1000);
    expect(backoffDelay(1000, 500, 15_000, 1)).toBe(15_000);
  });
});

describe("RemoteClient", () => {
  it("is open only after hello, with the server's verbs", () => {
    client.start();
    expect(client.state).toEqual({ kind: "connecting", attempt: 0 });
    last(sockets).open();
    expect(client.state.kind).toBe("connecting");
    last(sockets).hello(["key"]);
    expect(client.state).toMatchObject({ kind: "open", server: "fake 1", stale: false });
    expect(client.supports("key")).toBe(true);
    expect(client.supports("volume.set")).toBe(false);
  });

  it("matches replies to requests by id", async () => {
    client.start();
    const s = last(sockets);
    s.hello();
    const a = client.request("volume.set", { level: 30 });
    const b = client.request("volume.up");
    expect(s.sent).toEqual([
      { id: 1, verb: "volume.set", args: { level: 30 } },
      { id: 2, verb: "volume.up", args: {} },
    ]);
    s.push(
      { id: 2, ok: true, result: {} },
      { id: 1, ok: false, error: { code: "bad-args", message: "too loud" } },
    );
    await expect(b).resolves.toEqual({});
    await expect(a).rejects.toMatchObject({ code: "bad-args", message: "too loud" });
  });

  it("refuses a reply of the wrong shape", async () => {
    client.start();
    const s = last(sockets);
    s.hello();
    const p = client.request("apps.list");
    s.push({ id: 1, ok: true, result: { apps: "lots" } });
    await expect(p).rejects.toMatchObject({ code: "bad-reply" });
  });

  it("fails a request at once while not connected, and never queues it", async () => {
    client.start();
    await expect(client.request("volume.up")).rejects.toBeInstanceOf(RequestError);
    await expect(client.request("volume.up")).rejects.toMatchObject({ code: "not-connected" });
    last(sockets).hello();
    expect(last(sockets).sent).toEqual([]);
  });

  it("times out a request with no answer", async () => {
    client.start();
    last(sockets).hello();
    const p = client.request("volume.up");
    vi.advanceTimersByTime(5_000);
    await expect(p).rejects.toMatchObject({ code: "timeout" });
  });

  it("passes events on, hello included", () => {
    const got: Event[] = [];
    client.onEvent((e) => got.push(e));
    client.start();
    last(sockets).hello();
    last(sockets).push({ event: "volume", data: { level: 4, muted: true } }, { event: "nonsense" });
    expect(got.map((e) => e.event)).toEqual(["hello", "volume"]);
  });

  it("reconnects with growing delays and starts over after a hello", () => {
    client.start();
    last(sockets).drop();
    expect(client.state).toMatchObject({ kind: "waiting", attempt: 0 });
    vi.advanceTimersByTime(499);
    expect(sockets).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(2);
    last(sockets).drop();
    vi.advanceTimersByTime(999);
    expect(sockets).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(sockets).toHaveLength(3);
    last(sockets).hello();
    last(sockets).drop();
    expect(client.state).toMatchObject({ kind: "waiting", attempt: 0 });
  });

  it("fails pending requests when the link closes", async () => {
    client.start();
    last(sockets).hello();
    const p = client.request("volume.up");
    last(sockets).drop();
    await expect(p).rejects.toMatchObject({ code: "disconnected" });
  });

  it("goes stale after a quiet spell, recovers on the next line, and drops a dead link", () => {
    client.start();
    const s = last(sockets);
    s.hello();
    vi.advanceTimersByTime(24_999);
    expect(client.state).toMatchObject({ stale: false });
    vi.advanceTimersByTime(1);
    expect(client.state).toMatchObject({ kind: "open", stale: true });
    s.push({ event: "alive", data: {} });
    expect(client.state).toMatchObject({ kind: "open", stale: false });
    vi.advanceTimersByTime(45_000);
    expect(s.closed).toMatchObject({ code: 4000 });
    expect(client.state.kind).toBe("waiting");
  });

  it("drops a server that opens but never says hello", () => {
    client.start();
    last(sockets).open();
    vi.advanceTimersByTime(5_000);
    expect(sockets[0]?.closed).not.toBeNull();
    expect(client.state.kind).toBe("waiting");
  });

  it("stops at an incompatible protocol until asked to retry", () => {
    client.start();
    last(sockets).hello([], 2);
    expect(client.state).toEqual({ kind: "incompatible", protocol: 2 });
    vi.advanceTimersByTime(60_000);
    expect(sockets).toHaveLength(1);
    client.retryNow();
    expect(sockets).toHaveLength(2);
  });

  it("closes while paused and reconnects on start", () => {
    client.start();
    last(sockets).hello();
    client.pause();
    expect(sockets[0]?.closed).not.toBeNull();
    expect(client.state.kind).toBe("paused");
    vi.advanceTimersByTime(60_000);
    expect(sockets).toHaveLength(1);
    client.start();
    expect(sockets).toHaveLength(2);
  });

  it("moves to a new address", () => {
    client.start();
    client.setUrl("ws://other/ctl");
    expect(last(sockets).url).toBe("ws://other/ctl");
  });

  it("retries a bad address instead of throwing", () => {
    client.pause();
    client = make({
      socket: () => {
        throw new SyntaxError("bad url");
      },
    });
    client.start();
    expect(client.state).toMatchObject({ kind: "waiting" });
  });

  it("wraps request ids", () => {
    client.start();
    const s = last(sockets);
    s.hello();
    // Reach the top of the id range without sending two billion requests.
    (client as unknown as { nextId: number }).nextId = 2_147_483_647;
    void client.request("volume.up").catch(() => {});
    void client.request("volume.up").catch(() => {});
    expect(s.sent.map((m) => (m as { id: number }).id)).toEqual([2_147_483_647, 1]);
  });
});
