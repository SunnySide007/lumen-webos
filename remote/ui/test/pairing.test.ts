// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { RemoteClient } from "../src/client.ts";
import type { SocketLike } from "../src/client.ts";
import { Pairing } from "../src/pairing.ts";
import type { DevicesState } from "../src/protocol.ts";

/** A PC session's socket: answers the pairing verbs from a list. */
function pcSocket(): {
  factory: () => SocketLike;
  sent: unknown[];
  push(line: object): void;
  closeWith(code: number): void;
} {
  const sent: unknown[] = [];
  let sock: SocketLike | null = null;
  const devices: DevicesState = {
    devices: [{ id: "0a1b2c3d", name: "Pixel", role: "remote", paired: "2026-11-08T20:00:00Z" }],
    pairing: { open: false, secondsLeft: 0, attemptsLeft: 0 },
  };
  const reply = (id: number, result: object) => sock?.onmessage?.({ data: JSON.stringify({ id, ok: true, result }) });
  return {
    sent,
    push: (line) => sock?.onmessage?.({ data: JSON.stringify(line) }),
    closeWith: (code) => sock?.onclose?.({ code, reason: "revoked" }),
    factory: () => {
      const s: SocketLike = {
        onopen: null,
        onmessage: null,
        onclose: null,
        onerror: null,
        send(data) {
          const r = JSON.parse(data) as { id: number; verb: string; args: Record<string, string> };
          sent.push(r);
          if (r.verb === "pair.open") reply(r.id, { code: "482913", seconds: 120, toasted: true });
          else if (r.verb === "devices.list") reply(r.id, devices);
          else if (r.verb === "devices.role") {
            devices.devices[0]!.role = r.args.role as "remote" | "owner";
            reply(r.id, {});
          } else if (r.verb === "devices.revoke") {
            devices.devices = [];
            reply(r.id, {});
          } else reply(r.id, {});
        },
        close() {},
      };
      sock = s;
      queueMicrotask(() => {
        s.onopen?.();
        s.onmessage?.({
          data: JSON.stringify({
            event: "hello",
            data: { protocol: 1, server: "lumen-ctl", verbs: ["pair.open", "pair.close", "devices.list", "devices.revoke", "devices.role"] },
          }),
        });
      });
      return s;
    },
  };
}

async function openClient(): Promise<{ client: RemoteClient; pc: ReturnType<typeof pcSocket> }> {
  const pc = pcSocket();
  const client = new RemoteClient({ url: "ws://pc", socket: pc.factory });
  client.start();
  await new Promise((r) => setTimeout(r, 0));
  expect(client.state.kind).toBe("open");
  return { client, pc };
}

describe("Pairing", () => {
  it("opens a window and keeps its code until the window ends", async () => {
    const { client } = await openClient();
    let now = 1_000_000;
    const p = new Pairing(client, () => now);
    expect(p.available).toBe(true);
    await p.open();
    expect(p.state.get().window).toMatchObject({ code: "482913", attemptsLeft: 5, toasted: true });
    expect(p.secondsLeft()).toBe(120);
    now += 61_000;
    expect(p.secondsLeft()).toBe(59);
    // A wrong code on the phone comes back as a devices event: the code stays.
    p.onEvent({ event: "devices", data: { devices: [], pairing: { open: true, secondsLeft: 59, attemptsLeft: 4 } } });
    expect(p.state.get().window).toMatchObject({ code: "482913", attemptsLeft: 4 });
    // Paired, or closed: the window and its code go.
    p.onEvent({ event: "devices", data: { devices: [], pairing: { open: false, secondsLeft: 0, attemptsLeft: 0 } } });
    expect(p.state.get().window).toBeNull();
    expect(p.secondsLeft()).toBe(0);
    client.pause();
  });

  it("lists, promotes and revokes devices", async () => {
    const { client, pc } = await openClient();
    const p = new Pairing(client);
    await p.refresh();
    expect(p.state.get().devices.map((d) => d.role)).toEqual(["remote"]);
    await p.setRole("0a1b2c3d", "owner");
    expect(p.state.get().devices[0]!.role).toBe("owner");
    await p.revoke("0a1b2c3d");
    expect(p.state.get().devices).toEqual([]);
    expect(pc.sent.map((r) => (r as { verb: string }).verb)).toEqual([
      "devices.list",
      "devices.role",
      "devices.list",
      "devices.revoke",
      "devices.list",
    ]);
    p.onLinkLost();
    expect(p.state.get().loaded).toBe(false);
    client.pause();
  });

  it("stops trying once lumen-remote says the device was revoked", async () => {
    const { client, pc } = await openClient();
    pc.closeWith(1006);
    expect(client.state.kind).toBe("waiting");
    client.retryNow();
    await new Promise((r) => setTimeout(r, 0));
    expect(client.state.kind).toBe("open");
    pc.closeWith(4401);
    expect(client.state).toEqual({ kind: "revoked" });
    await new Promise((r) => setTimeout(r, 20));
    expect(client.state.kind).toBe("revoked");
  });

  it("isn't available on a phone's link", () => {
    const pc = pcSocket();
    const client = new RemoteClient({
      url: "ws://phone",
      socket: () => {
        const s = pc.factory();
        s.onopen = null;
        return s;
      },
    });
    const p = new Pairing(client);
    expect(p.available).toBe(false);
  });
});
