// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pairing the phone: a stub until R4. There, the PC app opens pairing, the TV
// shows a six-digit code for two minutes, the phone sends it to lumen-remote
// over HTTPS and gets a long random token; five wrong codes close pairing.
// Here the code is checked by nothing but this file, so the screens can be
// designed and tested. The token lives in this object only: never in
// storage, never in the URL, gone when the page closes.
import { Store } from "./store.ts";

export type PairState =
  | { kind: "unpaired"; attemptsLeft: number; wrong: boolean }
  | { kind: "checking" }
  | { kind: "paired"; since: Date }
  | { kind: "closed" };

/** The stub's stand-in for a wrong code, to show that path. */
export const STUB_WRONG_CODE = "000000";
export const MAX_ATTEMPTS = 5;

export class PairingStub {
  readonly state = new Store<PairState>({ kind: "unpaired", attemptsLeft: MAX_ATTEMPTS, wrong: false });
  private token: string | null = null;
  private attemptsLeft = MAX_ATTEMPTS;
  private readonly delayMs: number;

  constructor(delayMs = 700) {
    this.delayMs = delayMs;
  }

  get paired(): boolean {
    return this.token !== null;
  }

  async pair(code: string): Promise<void> {
    if (!/^\d{6}$/.test(code)) throw new Error("the code is six digits");
    const s = this.state.get();
    if (s.kind === "closed" || s.kind === "checking") return;
    this.state.set({ kind: "checking" });
    await new Promise((r) => setTimeout(r, this.delayMs));
    if (code === STUB_WRONG_CODE) {
      this.attemptsLeft--;
      this.state.set(
        this.attemptsLeft > 0
          ? { kind: "unpaired", attemptsLeft: this.attemptsLeft, wrong: true }
          : { kind: "closed" },
      );
      return;
    }
    const bytes = crypto.getRandomValues(new Uint8Array(32));
    this.token = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    this.state.set({ kind: "paired", since: new Date() });
  }

  /** Forgets the token, as revoking it on the PC will. */
  forget(): void {
    this.token = null;
    this.attemptsLeft = MAX_ATTEMPTS;
    this.state.set({ kind: "unpaired", attemptsLeft: MAX_ATTEMPTS, wrong: false });
  }
}
