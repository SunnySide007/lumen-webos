// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pairing (R4), the PC's side. The PC app opens pairing over its SSH key
// (`pair.open`): lumen-ctl makes a six-digit code that stands two minutes,
// toasts it on the TV and gives it back here to show. The phone types it
// into lumen-remote's own pairing page, a page with no script that the TV
// serves to any unpaired browser, and gets its token as an HttpOnly cookie
// this front-end never sees. Five wrong codes close pairing.
//
// Here the PC also lists the paired devices, gives or takes back owner
// rights (decision 12) and revokes them; each change reaches the TV's
// devices file at once, and lumen-remote closes a revoked phone's link.
// A phone's Pairing screen shows only who it is, from lumen-remote's hello.
import type { RemoteClient } from "./client.ts";
import type { DeviceInfo, DevicesState, Event, Role } from "./protocol.ts";
import { Store } from "./store.ts";

export const MAX_ATTEMPTS = 5;

export interface PairingView {
  devices: DeviceInfo[];
  /** The open window: its code (shown only to the PC that opened it),
   * when it ends, and the tries left. */
  window: { code: string | null; until: number; attemptsLeft: number; toasted: boolean } | null;
  /** Whether the list has been read since the link opened. */
  loaded: boolean;
}

const EMPTY: PairingView = { devices: [], window: null, loaded: false };

export class Pairing {
  readonly state = new Store<PairingView>(EMPTY);
  private readonly client: RemoteClient;
  private readonly now: () => number;

  constructor(client: RemoteClient, now: () => number = Date.now) {
    this.client = client;
    this.now = now;
  }

  /** Whether this link can manage pairing: the PC's own session. */
  get available(): boolean {
    return this.client.supports("devices.list");
  }

  /** Takes the `devices` event and forgets everything when the link drops. */
  onEvent(e: Event): void {
    if (e.event === "devices") this.apply(e.data);
  }

  onLinkLost(): void {
    this.state.set(EMPTY);
  }

  private apply(d: DevicesState): void {
    const before = this.state.get().window;
    const window = d.pairing.open
      ? {
          code: before?.code ?? null,
          until: this.now() + d.pairing.secondsLeft * 1000,
          attemptsLeft: d.pairing.attemptsLeft,
          toasted: before?.toasted ?? false,
        }
      : null;
    this.state.set({ devices: d.devices, window, loaded: true });
  }

  async refresh(): Promise<void> {
    this.apply(await this.client.request("devices.list"));
  }

  /** Opens a window; the code is kept here to show until it ends. */
  async open(): Promise<void> {
    const r = await this.client.request("pair.open");
    this.state.update((s) => ({
      ...s,
      window: { code: r.code, until: this.now() + r.seconds * 1000, attemptsLeft: MAX_ATTEMPTS, toasted: r.toasted },
    }));
  }

  async close(): Promise<void> {
    await this.client.request("pair.close");
    this.state.update((s) => ({ ...s, window: null }));
  }

  async revoke(id: string): Promise<void> {
    await this.client.request("devices.revoke", { id });
    await this.refresh();
  }

  async setRole(id: string, role: Role): Promise<void> {
    await this.client.request("devices.role", { id, role });
    await this.refresh();
  }

  /** Seconds left in the window, 0 when there is none. */
  secondsLeft(): number {
    const w = this.state.get().window;
    return w ? Math.max(0, Math.ceil((w.until - this.now()) / 1000)) : 0;
  }
}
