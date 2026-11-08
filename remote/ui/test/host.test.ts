// SPDX-License-Identifier: GPL-3.0-or-later
//
// The PC app's link: RemoteClient over sshSocket, with Tauri's globals faked.
// The lines are what `lumen-ctl serve --bus fake` sent (fixtures/
// lumen-ctl-fake.jsonl, captured from the binary in shell/crates/lumen-ctl):
// its hello, which offers no `key`, the snapshot, and its reply to apps.list.
// lumen-remote-link's tests/fake_bus.rs runs the app's ssh session against
// the same binary.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { RemoteClient } from "../src/client.ts";
import type { LinkState } from "../src/client.ts";
import { pcHost, sshAddress, sshSocket } from "../src/host.ts";
import type { LinkMsg, TauriChannel, TauriGlobal } from "../src/host.ts";

const FIXTURE = readFileSync(new URL("./fixtures/lumen-ctl-fake.jsonl", import.meta.url), "utf8")
  .split("\n")
  .filter((l) => l.trim() !== "");
const HELLO_AND_SNAPSHOT = FIXTURE.filter((l) => !l.startsWith('{"id"'));
const APPS_REPLY = FIXTURE.find((l) => l.startsWith('{"id":1,'))!;

const tick = () => new Promise((r) => setTimeout(r, 0));

class FakeChannel<T> implements TauriChannel<T> {
  onmessage: (message: T) => void = () => undefined;
}

interface Call {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

/** Tauri's globals, with link_open answered by `open` (default: id 7). */
function fakeTauri(open: () => Promise<number> = () => Promise.resolve(7)) {
  const calls: Call[] = [];
  const channels: FakeChannel<LinkMsg>[] = [];
  const tauri: TauriGlobal = {
    core: {
      invoke: <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
        calls.push({ cmd, args });
        if (cmd === "link_open") {
          channels.push(args?.onEvent as FakeChannel<LinkMsg>);
          return open() as Promise<T>;
        }
        return Promise.resolve(undefined as T);
      },
      Channel: FakeChannel,
    },
    event: { listen: () => Promise.resolve(() => undefined) },
  };
  const tv = (m: LinkMsg) => channels.at(-1)!.onmessage(m);
  const sent = () => calls.filter((c) => c.cmd === "link_send").map((c) => c.args);
  return { tauri, calls, tv, sent };
}

function client(tauri: TauriGlobal): RemoteClient {
  return new RemoteClient({
    url: "ssh://root@tv",
    socket: sshSocket(tauri),
    random: () => 0,
    backoffBaseMs: 60_000,
    helloTimeoutMs: 60_000,
  });
}

describe("RemoteClient over the PC app's SSH session", () => {
  it("opens on lumen-ctl's hello, without key, and keeps its verbs", async () => {
    const { tauri, tv } = fakeTauri();
    const c = client(tauri);
    const states: LinkState["kind"][] = [];
    c.onState((s) => states.push(s.kind));
    const events: string[] = [];
    c.onEvent((e) => events.push(e.event));
    c.start();
    await tick();
    tv({ kind: "open" });
    for (const line of HELLO_AND_SNAPSHOT) tv({ kind: "line", line });
    expect(states.at(-1)).toBe("open");
    expect(c.supports("volume.set")).toBe(true);
    expect(c.supports("key")).toBe(false);
    expect(c.lastVerbs?.has("apps.list")).toBe(true);
    expect(events.slice(0, 6)).toEqual(["hello", "power", "volume", "foreground", "inputs", "settings"]);
    c.pause();
    // The verbs outlive the link, so the Remote screen keeps its pad hidden.
    expect(c.lastVerbs?.has("key")).toBe(false);
  });

  it("sends each request as one line and resolves it from lumen-ctl's reply", async () => {
    const { tauri, tv, sent } = fakeTauri();
    const c = client(tauri);
    c.start();
    await tick();
    tv({ kind: "open" });
    for (const line of HELLO_AND_SNAPSHOT) tv({ kind: "line", line });
    const reply = c.request("apps.list");
    expect(sent()).toEqual([{ id: 7, line: '{"id":1,"verb":"apps.list","args":{}}' }]);
    tv({ kind: "line", line: APPS_REPLY });
    const { apps } = await reply;
    expect(apps.map((a) => a.kind)).toContain("input");
    expect(apps.find((a) => a.id === "netflix")?.iconColor).toBe("#e50914");
    c.pause();
  });

  it("shows ssh's reason when the session ends, and closes it on pause", async () => {
    const { tauri, tv, calls } = fakeTauri();
    const c = client(tauri);
    c.start();
    await tick();
    tv({ kind: "open" });
    tv({ kind: "closed", reason: "The TV refused the key: is it in authorized_keys?" });
    const s = c.state;
    expect(s.kind === "waiting" && s.reason).toBe("The TV refused the key: is it in authorized_keys?");
    c.retryNow();
    await tick();
    tv({ kind: "open" });
    c.pause();
    await tick();
    expect(calls.filter((x) => x.cmd === "link_close")).toEqual([{ cmd: "link_close", args: { id: 7 } }]);
  });

  it("says why when the app can't open a session at all", async () => {
    const { tauri } = fakeTauri(() => Promise.reject(new Error("No key at C:\\k: tools/remote.sh keygen makes one")));
    const c = client(tauri);
    c.start();
    await tick();
    await tick();
    const s = c.state;
    expect(s.kind === "waiting" && s.reason).toBe("Error: No key at C:\\k: tools/remote.sh keygen makes one");
    c.pause();
  });

  it("holds lines sent before the app names the session, then sends them in order", async () => {
    let name: (n: number) => void = () => undefined;
    const { tauri, sent } = fakeTauri(() => new Promise((r) => (name = r)));
    const socket = sshSocket(tauri)("ssh://root@tv");
    socket.send('{"id":1}\n');
    socket.send('{"id":2}\n');
    expect(sent()).toEqual([]);
    name(3);
    await tick();
    expect(sent()).toEqual([
      { id: 3, line: '{"id":1}' },
      { id: 3, line: '{"id":2}' },
    ]);
  });

  it("closes a session that was given up before it was named", async () => {
    let name: (n: number) => void = () => undefined;
    const { tauri, calls } = fakeTauri(() => new Promise((r) => (name = r)));
    const socket = sshSocket(tauri)("ssh://root@tv");
    let closes = 0;
    socket.onclose = () => closes++;
    socket.close();
    name(4);
    await tick();
    expect(calls.filter((x) => x.cmd === "link_close")).toEqual([{ cmd: "link_close", args: { id: 4 } }]);
    // A close the page asked for isn't reported back to it.
    expect(closes).toBe(0);
  });
});

describe("sshAddress", () => {
  it("names the TV, and the port only when it isn't 22", () => {
    expect(sshAddress({ host: "192.168.86.20", port: 22 })).toBe("ssh://root@192.168.86.20");
    expect(sshAddress({ host: "tv.lan", port: 2222 })).toBe("ssh://root@tv.lan:2222");
    expect(sshAddress({ host: null, port: 22 })).toBe("ssh: no TV set");
  });
});

describe("pcHost", () => {
  it("hears only its own window's visibility", async () => {
    const listens: { name: string; target: string | undefined }[] = [];
    const { tauri } = fakeTauri();
    tauri.event.listen = (name, _fn, options) => {
      listens.push({ name, target: options?.target });
      return Promise.resolve(() => undefined);
    };
    const host = pcHost(tauri, { host: "tv", port: 22, mac: null, key: "k", keyFound: true, ssh: "ssh" }, "popup");
    host.onVisible(() => undefined);
    await tick();
    expect(listens).toEqual([{ name: "lumen-visibility", target: "popup" }]);
    expect(host.address).toBe("ssh://root@tv");
    expect(host.helloTimeoutMs).toBeGreaterThanOrEqual(15_000);
  });
});
