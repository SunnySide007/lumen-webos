// SPDX-License-Identifier: GPL-3.0-or-later
//
// The mock's network side, shaped like lumen-remote (R4): one port serving the
// built web UI and the protocol's WebSocket at /ctl. Each WebSocket message
// carries one line. A browser from another origin is refused, as lumen-remote
// will refuse it; pairing and HTTPS are R4's and not mocked.
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { extname, join, normalize, resolve, sep } from "node:path";
import { WebSocketServer } from "ws";
import type { WebSocket } from "ws";
import {
  ALIVE_INTERVAL_MS,
  MAX_LINE_BYTES,
  encode,
  splitLines,
} from "../../ui/src/protocol.ts";
import type { Event } from "../../ui/src/protocol.ts";
import type { FakeTv } from "./tv.ts";

export interface ServerOptions {
  tv: FakeTv;
  host?: string;
  port?: number;
  /** The built UI to serve at /, or null to serve none. */
  staticDir?: string | null;
  /** Origins allowed besides the page's own (a Tauri app's, say). */
  allowOrigins?: string[];
  /** How long the TV stays off after power.off before it "wakes". */
  offSeconds?: number;
  aliveMs?: number;
  server?: string;
  log?: (line: string) => void;
}

export interface MockServer {
  port: number;
  /** Closes every client's link, as a dropped Wi-Fi would. */
  dropAll(reason: string): void;
  /** Sends nothing at all for `seconds`, to show the UI's stale badge. */
  silence(seconds: number): void;
  close(): Promise<void>;
}

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".json": "application/json",
  ".webmanifest": "application/manifest+json",
  ".txt": "text/plain; charset=utf-8",
};

/** The file under `root` that a request path names, or null if it names
 * none or tries to leave `root`. */
export function staticPath(root: string, urlPath: string): string | null {
  let path: string;
  try {
    path = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  if (path.includes("\0")) return null;
  const base = resolve(root);
  const full = resolve(join(base, normalize(path === "/" ? "/index.html" : path)));
  if (full !== base && !full.startsWith(base + sep)) return null;
  try {
    return statSync(full).isFile() ? full : null;
  } catch {
    return null;
  }
}

function sameOrigin(req: IncomingMessage, allow: string[]): boolean {
  const origin = req.headers.origin;
  if (origin === undefined) return true; // not a browser: curl, a test, Tauri's Rust side
  if (allow.includes(origin)) return true;
  try {
    return new URL(origin).host === req.headers.host;
  } catch {
    return false;
  }
}

export function startServer(opts: ServerOptions): Promise<MockServer> {
  const tv = opts.tv;
  const log = opts.log ?? (() => {});
  const allow = opts.allowOrigins ?? [];
  const name = opts.server ?? "mock-ctl";
  const offMs = (opts.offSeconds ?? 15) * 1000;
  const clients = new Map<WebSocket, number>();
  let nextClient = 1;
  let quietUntil = 0;
  let offUntil = 0;

  const send = (ws: WebSocket, line: string) => {
    if (Date.now() < quietUntil) return;
    ws.send(line);
  };
  const broadcast = (e: Event) => {
    const line = encode(e);
    for (const ws of clients.keys()) send(ws, line);
  };
  const stopEvents = tv.onEvent((e) => {
    if (e.event !== "alive") log(`event ${e.event} ${JSON.stringify(e.data)}`);
    broadcast(e);
  });
  const alive = setInterval(() => broadcast({ event: "alive", data: {} }), opts.aliveMs ?? ALIVE_INTERVAL_MS);

  const dropAll = (reason: string) => {
    for (const ws of clients.keys()) ws.close(1001, reason);
  };

  tv.onPowerOff = () => {
    offUntil = Date.now() + offMs;
    log(`the TV is off for ${offMs / 1000} s, as if waiting for Wake-on-LAN`);
    dropAll("the TV turned off");
    setTimeout(() => {
      log("the TV woke up");
      tv.setPower("active");
    }, offMs).unref();
  };

  const http = createServer((req: IncomingMessage, res: ServerResponse) => {
    const reply = (status: number, text: string) => {
      res.writeHead(status, { "Content-Type": "text/plain; charset=utf-8" });
      res.end(text);
    };
    if (req.method !== "GET" && req.method !== "HEAD") return reply(405, "GET only\n");
    if (!opts.staticDir) return reply(404, "mock-ctl serves no page; the WebSocket is at /ctl\n");
    const url = new URL(req.url ?? "/", "http://mock");
    const file = staticPath(opts.staticDir, url.pathname);
    if (!file) return reply(404, "not found\n");
    res.writeHead(200, {
      "Content-Type": TYPES[extname(file)] ?? "application/octet-stream",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
    });
    if (req.method === "HEAD") return res.end();
    createReadStream(file).pipe(res);
  });

  const wss = new WebSocketServer({ noServer: true, maxPayload: MAX_LINE_BYTES + 1 });

  http.on("upgrade", (req, socket, head) => {
    const refuse = (status: string) => {
      socket.end(`HTTP/1.1 ${status}\r\nConnection: close\r\n\r\n`);
    };
    const path = new URL(req.url ?? "/", "http://mock").pathname;
    if (path !== "/ctl") return refuse("404 Not Found");
    if (!sameOrigin(req, allow)) {
      log(`refused a WebSocket from origin ${req.headers.origin ?? "?"}`);
      return refuse("403 Forbidden");
    }
    if (Date.now() < offUntil) return refuse("503 Service Unavailable");
    wss.handleUpgrade(req, socket, head, (ws) => wss.emit("connection", ws));
  });

  wss.on("connection", (ws: WebSocket) => {
    const n = nextClient++;
    clients.set(ws, n);
    log(`client ${n} connected (${clients.size} now)`);
    send(ws, encode(tv.hello(name)));
    for (const e of tv.snapshot()) send(ws, encode(e));
    ws.on("message", (data, isBinary) => {
      if (isBinary) {
        send(ws, encode({ id: null, ok: false, error: { code: "bad-line", message: "binary message" } }));
        return;
      }
      // ws hands text frames over as one Buffer (its binaryType is nodebuffer).
      const text = Array.isArray(data)
        ? Buffer.concat(data).toString("utf8")
        : Buffer.isBuffer(data)
          ? data.toString("utf8")
          : Buffer.from(new Uint8Array(data)).toString("utf8");
      for (const line of splitLines(text)) send(ws, encode(tv.handle(line)));
    });
    ws.on("close", () => {
      clients.delete(ws);
      log(`client ${n} left (${clients.size} now)`);
    });
    ws.on("error", (err) => log(`client ${n}: ${err.message}`));
  });

  return new Promise((resolvePromise, reject) => {
    http.once("error", reject);
    http.listen(opts.port ?? 8765, opts.host ?? "127.0.0.1", () => {
      const port = (http.address() as AddressInfo).port;
      resolvePromise({
        port,
        dropAll,
        silence(seconds) {
          quietUntil = Date.now() + seconds * 1000;
          log(`sending nothing for ${seconds} s`);
        },
        close: () =>
          new Promise<void>((done) => {
            clearInterval(alive);
            stopEvents();
            tv.onPowerOff = null;
            for (const ws of clients.keys()) ws.terminate();
            wss.close();
            http.close(() => done());
          }),
      });
    });
  });
}
