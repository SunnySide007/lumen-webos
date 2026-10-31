// SPDX-License-Identifier: GPL-3.0-or-later
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MAX_LINE_BYTES } from "../../ui/src/protocol.ts";
import { staticPath, startServer } from "../src/server.ts";
import type { MockServer } from "../src/server.ts";
import { FakeTv } from "../src/tv.ts";

/** A client on Node's own WebSocket, collecting every line it gets. */
class Client {
  readonly lines: Record<string, unknown>[] = [];
  readonly ws: WebSocket;
  closed = false;
  private waiters: (() => void)[] = [];
  constructor(url: string) {
    this.ws = new WebSocket(url);
    this.ws.onmessage = (ev) => {
      this.lines.push(JSON.parse(String(ev.data)) as Record<string, unknown>);
      this.wake();
    };
    this.ws.onclose = () => {
      this.closed = true;
      this.wake();
    };
  }
  private wake(): void {
    for (const w of this.waiters.splice(0)) w();
  }
  /** Waits until `test` holds of what has arrived. */
  async until(test: (lines: Record<string, unknown>[]) => boolean, ms = 2000): Promise<void> {
    const end = Date.now() + ms;
    while (!test(this.lines)) {
      if (this.closed || Date.now() > end) throw new Error(`gave up waiting; got ${JSON.stringify(this.lines)}`);
      await new Promise<void>((r) => {
        this.waiters.push(r);
        setTimeout(r, 50);
      });
    }
  }
  send(o: unknown): void {
    this.ws.send(typeof o === "string" ? o : JSON.stringify(o));
  }
}

let tv: FakeTv;
let server: MockServer;
let url: string;
const clients: Client[] = [];

function connect(): Client {
  const c = new Client(url);
  clients.push(c);
  return c;
}

beforeEach(async () => {
  tv = new FakeTv();
  server = await startServer({ tv, port: 0, staticDir: null, aliveMs: 100, offSeconds: 0.2 });
  url = `ws://127.0.0.1:${server.port}/ctl`;
});

afterEach(async () => {
  for (const c of clients.splice(0)) c.ws.close();
  await server.close();
});

const hasReply = (id: number) => (ls: Record<string, unknown>[]) => ls.some((l) => l.id === id);

describe("the mock's WebSocket", () => {
  it("opens with hello and the current state", async () => {
    const c = connect();
    await c.until((ls) => ls.length >= 6);
    expect(c.lines.map((l) => l.event)).toEqual(["hello", "power", "volume", "foreground", "inputs", "settings"]);
    expect(c.lines[0]).toMatchObject({ data: { protocol: 1, server: "mock-ctl" } });
  });

  it("answers a request and pushes the change to every client", async () => {
    const a = connect();
    const b = connect();
    await a.until((ls) => ls.length >= 6);
    await b.until((ls) => ls.length >= 6);
    a.send({ id: 9, verb: "volume.set", args: { level: 42 } });
    await a.until(hasReply(9));
    expect(a.lines.find((l) => l.id === 9)).toEqual({ id: 9, ok: true, result: {} });
    const pushed = { event: "volume", data: { level: 42, muted: false } };
    await b.until((ls) => ls.some((l) => JSON.stringify(l) === JSON.stringify(pushed)));
  });

  it("answers each line of a message, and refuses bad ones", async () => {
    const c = connect();
    await c.until((ls) => ls.length >= 6);
    c.send('{"id":1,"verb":"volume.up"}\n{"id":2,"verb":"volume.set","args":{"level":-5}}\nnope');
    await c.until((ls) => hasReply(1)(ls) && hasReply(2)(ls) && ls.some((l) => l.id === null));
    expect(c.lines.find((l) => l.id === 2)).toMatchObject({ ok: false, error: { code: "bad-args" } });
    expect(c.lines.find((l) => l.id === null)).toMatchObject({ ok: false, error: { code: "bad-line" } });
  });

  it("closes a link that sends a message over the line limit", async () => {
    const c = connect();
    await c.until((ls) => ls.length >= 6);
    c.send("x".repeat(MAX_LINE_BYTES + 2));
    await c.until(() => false).catch(() => {});
    expect(c.closed).toBe(true);
  });

  it("sends alive while quiet", async () => {
    const c = connect();
    await c.until((ls) => ls.some((l) => l.event === "alive"));
  });

  it("drops every client at power.off and refuses them until the TV wakes", async () => {
    const c = connect();
    await c.until((ls) => ls.length >= 6);
    c.send({ id: 1, verb: "power.off" });
    await c.until(() => false).catch(() => {});
    expect(c.closed).toBe(true);
    expect(c.lines).toContainEqual({ event: "power", data: { state: "standby" } });
    const refused = connect();
    await refused.until(() => false).catch(() => {});
    expect(refused.lines).toEqual([]);
    await new Promise((r) => setTimeout(r, 300));
    const back = connect();
    await back.until((ls) => ls.some((l) => l.event === "power"));
    expect(back.lines.find((l) => l.event === "power")).toEqual({ event: "power", data: { state: "active" } });
  });
});

/** An upgrade request by hand, to set the headers a browser would. */
function upgrade(path: string, origin?: string): Promise<number> {
  return new Promise((resolve, reject) => {
    const req = request({
      host: "127.0.0.1",
      port: server.port,
      path,
      headers: {
        Connection: "Upgrade",
        Upgrade: "websocket",
        "Sec-WebSocket-Version": "13",
        "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==",
        Host: `127.0.0.1:${server.port}`,
        ...(origin ? { Origin: origin } : {}),
      },
    });
    req.on("upgrade", (_res, socket) => {
      socket.destroy();
      resolve(101);
    });
    req.on("response", (res) => resolve(res.statusCode ?? 0));
    req.on("error", reject);
    req.end();
  });
}

describe("the mock's doors", () => {
  it("lets in its own origin and refuses others", async () => {
    expect(await upgrade("/ctl", `http://127.0.0.1:${server.port}`)).toBe(101);
    expect(await upgrade("/ctl")).toBe(101);
    expect(await upgrade("/ctl", "http://evil.example")).toBe(403);
    expect(await upgrade("/other")).toBe(404);
  });
});

describe("staticPath", () => {
  let root: string;
  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), "mock-ctl-"));
    mkdirSync(join(root, "assets"));
    writeFileSync(join(root, "index.html"), "<!doctype html>");
    writeFileSync(join(root, "assets", "a.js"), "");
  });
  afterEach(() => rmSync(root, { recursive: true }));

  it("finds files under the root", () => {
    expect(staticPath(root, "/")).toBe(join(root, "index.html"));
    expect(staticPath(root, "/assets/a.js")).toBe(join(root, "assets", "a.js"));
  });

  it.each(["/../secret", "/%2e%2e/secret", "/assets/../../x", "/%00", "/%E0%A4%A", "/assets", "/nope"])(
    "finds nothing for %s",
    (p) => {
      expect(staticPath(root, p)).toBeNull();
    },
  );
});
