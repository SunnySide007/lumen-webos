// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { MAX_ATTEMPTS, PairingStub, STUB_WRONG_CODE } from "../src/pairing.ts";

describe("PairingStub", () => {
  it("pairs with six digits and keeps the token in memory only", async () => {
    const p = new PairingStub(0);
    await expect(p.pair("12345")).rejects.toThrow("six digits");
    await p.pair("482913");
    expect(p.paired).toBe(true);
    expect(p.state.get().kind).toBe("paired");
    expect(JSON.stringify(p.state.get())).not.toMatch(/[0-9a-f]{64}/);
    p.forget();
    expect(p.paired).toBe(false);
  });

  it("closes after five wrong codes", async () => {
    const p = new PairingStub(0);
    for (let i = 1; i < MAX_ATTEMPTS; i++) {
      await p.pair(STUB_WRONG_CODE);
      expect(p.state.get()).toEqual({ kind: "unpaired", attemptsLeft: MAX_ATTEMPTS - i, wrong: true });
    }
    await p.pair(STUB_WRONG_CODE);
    expect(p.state.get().kind).toBe("closed");
    await p.pair("482913");
    expect(p.paired).toBe(false);
  });
});
