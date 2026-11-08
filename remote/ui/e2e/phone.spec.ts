// SPDX-License-Identifier: GPL-3.0-or-later
//
// The phone end to end (R4): a browser against lumen-remote, which serves
// the built page over HTTPS to paired phones only, in front of `lumen-ctl
// serve --bus fake`, both in the build container (e2e/phone-tv.sh). The PC
// app's side, opening pairing and granting or revoking, is a second lumen-ctl
// session over `docker exec`, as the app's SSH session is on the TV. Nothing
// reaches a TV.
//
//   npm run build && npm run e2e:phone
//
// The certificate is the test CA's (shell/crates/lumen-remote/tests/fixtures),
// which the browser doesn't know: the chain itself is checked by lumen-remote's
// tests and tools/tests/remote_ca.bats.
import { spawn, spawnSync } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { resolve } from "node:path";
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

export const PHONE_PORT = 18443;
const ORIGIN = `https://127.0.0.1:${PHONE_PORT}`;
const NAME = "lumen-phone-e2e";
// Playwright runs from remote/ui.
const ROOT = resolve("../..");
const D = "/tmp/lumen";

const docker = (args: string[]) => spawnSync("docker", args, { encoding: "utf8", env: { ...process.env, MSYS_NO_PATHCONV: "1" } });

let tv: ChildProcess | null = null;
let tvOut = "";

test.describe.configure({ mode: "serial" });
test.use({ baseURL: ORIGIN, ignoreHTTPSErrors: true });

test.beforeAll(async () => {
  test.setTimeout(300_000);
  docker(["rm", "-f", NAME]);
  tv = spawn("sh", [resolve(ROOT, "tools/dev.sh"), "sh", "remote/ui/e2e/phone-tv.sh", String(PHONE_PORT)], {
    cwd: ROOT,
    env: { ...process.env, LUMEN_DEV_NAME: NAME, LUMEN_DEV_PUBLISH: `127.0.0.1:${PHONE_PORT}:${PHONE_PORT}` },
    stdio: ["ignore", "pipe", "pipe"],
  });
  tv.stdout?.on("data", (b: Buffer) => (tvOut += b.toString()));
  tv.stderr?.on("data", (b: Buffer) => (tvOut += b.toString()));
  const until = Date.now() + 280_000;
  while (!tvOut.includes("listening on")) {
    if (tv.exitCode !== null || Date.now() > until) throw new Error(`lumen-remote didn't start:\n${tvOut}`);
    await new Promise((r) => setTimeout(r, 500));
  }
});

test.afterAll(() => {
  docker(["rm", "-f", NAME]);
  tv?.kill();
});

/** The PC app's session: lumen-ctl over the SSH link, on the TV's state. */
class Pc {
  private readonly proc: ChildProcess;
  private nextId = 1;
  private readonly waiting = new Map<number, (r: { ok: boolean; result?: unknown; error?: { code: string } }) => void>();

  constructor() {
    this.proc = spawn(
      "docker",
      ["exec", "-i", NAME, `${D}/lumen-ctl`, "serve", "--bus", "fake", "--state", `${D}/state`, "--log", `${D}/pc.log`, "--config", `${D}/none`],
      { env: { ...process.env, MSYS_NO_PATHCONV: "1" }, stdio: ["pipe", "pipe", "ignore"] },
    );
    createInterface({ input: this.proc.stdout! }).on("line", (line) => {
      const m = JSON.parse(line) as { id?: number; ok: boolean };
      if (m.id !== undefined) this.waiting.get(m.id)?.(m);
    });
  }

  async ask<T = Record<string, unknown>>(verb: string, args?: Record<string, unknown>): Promise<T> {
    const id = this.nextId++;
    const reply = new Promise<{ ok: boolean; result?: unknown; error?: { code: string } }>((res, rej) => {
      this.waiting.set(id, res);
      setTimeout(() => rej(new Error(`no answer to ${verb}`)), 10_000);
    });
    this.proc.stdin!.write(`${JSON.stringify(args ? { id, verb, args } : { id, verb })}\n`);
    const r = await reply;
    if (!r.ok) throw new Error(`${verb}: ${JSON.stringify(r.error)}`);
    return r.result as T;
  }

  close(): void {
    this.proc.stdin?.end();
    this.proc.kill();
  }
}

type Listed = { devices: { id: string; name: string; role: string }[]; pairing: { open: boolean; attemptsLeft: number } };

let pc: Pc;
test.beforeEach(() => {
  pc = new Pc();
});
test.afterEach(() => pc.close());

/** Types the code into lumen-remote's pairing page. */
async function typeCode(page: Page, code: string, name = "Pixel"): Promise<void> {
  await page.getByLabel("The code on the TV").fill(code);
  await page.getByLabel("This phone's name, as the PC will list it").fill(name);
  await page.getByRole("button", { name: "Pair" }).click();
}

/** Pairs the page as a phone named `name`; returns its device id. */
async function pair(page: Page, name: string): Promise<string> {
  const { code } = await pc.ask<{ code: string }>("pair.open");
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Pair this phone" })).toBeVisible();
  await typeCode(page, code, name);
  await page.locator(".chip.link[data-tone='ok']").waitFor();
  const listed = await pc.ask<Listed>("devices.list");
  const d = listed.devices.find((x) => x.name === name);
  expect(d?.role).toBe("remote");
  return d!.id;
}

/** Sends one request on a WebSocket of the page's own, with its cookie. */
async function rawAsk(page: Page, verb: string, args?: Record<string, unknown>): Promise<{ ok: boolean; error?: { code: string } }> {
  return page.evaluate(
    ([verb, args]) =>
      new Promise((resolve, reject) => {
        const ws = new WebSocket(`wss://${location.host}/ctl`);
        ws.onmessage = (ev) => {
          const m = JSON.parse(String(ev.data)) as { event?: string; id?: number };
          if (m.event === "hello") ws.send(JSON.stringify({ id: 7, verb, args }));
          if (m.id === 7) {
            ws.close();
            resolve(m as { ok: boolean; error?: { code: string } });
          }
        };
        ws.onerror = () => reject(new Error("the WebSocket failed"));
      }),
    [verb, args] as const,
  );
}

test("an unpaired browser gets only the pairing page", async ({ page, request }) => {
  await page.goto("/#/remote");
  await expect(page.getByRole("heading", { name: "Pair this phone" })).toBeVisible();
  await expect(page.locator(".chip.link")).toHaveCount(0);
  expect((await request.get("/whoami")).status()).toBe(401);
  // The page's files and the WebSocket stay shut.
  expect((await request.get("/assets/", { maxRedirects: 0 })).status()).toBe(303);
  const ws = await page.evaluate(
    () =>
      new Promise<string>((resolve) => {
        const s = new WebSocket(`wss://${location.host}/ctl`);
        s.onopen = () => resolve("open");
        s.onerror = () => resolve("refused");
      }),
  );
  expect(ws).toBe("refused");
  // Another site's form is refused.
  const post = await request.post("/pair", { form: { code: "000000", name: "x" }, headers: { Origin: "https://evil.example" } });
  expect(post.status()).toBe(403);
});

test("pairing: the code from the PC pairs the phone, remote only", async ({ page, context }) => {
  await pair(page, "Pixel");
  const cookie = (await context.cookies()).find((c) => c.name === "__Host-lumen");
  expect(cookie).toMatchObject({ secure: true, httpOnly: true, sameSite: "Strict", path: "/" });
  await page.goto("/#/pairing");
  await expect(page.locator(".pair-card h2")).toHaveText("Pixel");
  await expect(page.locator(".pair-status")).toHaveText("Paired, remote only");
  const who = await page.evaluate(() => fetch("/whoami").then((r) => r.json() as Promise<{ name: string; role: string }>));
  expect(who).toMatchObject({ name: "Pixel", role: "remote" });
});

test("five wrong codes close pairing", async ({ page }) => {
  const { code } = await pc.ask<{ code: string }>("pair.open");
  const wrong = code === "000000" ? "111111" : "000000";
  await page.goto("/");
  for (let left = 4; left >= 1; left--) {
    await typeCode(page, wrong);
    await expect(page.locator(".msg")).toHaveText(`That code is wrong. ${left} of 5 tries left.`);
  }
  await typeCode(page, wrong);
  await expect(page.locator(".msg")).toContainText("that was the fifth: pairing is closed");
  expect((await pc.ask<Listed>("devices.list")).pairing.open).toBe(false);
  // The right code is too late now.
  await typeCode(page, code);
  await expect(page.locator(".msg")).toContainText("Pairing is closed");
  await expect(page.getByRole("heading", { name: "Pair this phone" })).toBeVisible();
});

test("a remote-only phone is refused the owner's verbs, until the PC grants them", async ({ page }) => {
  const id = await pair(page, "Remote phone");
  // The screens say so...
  await page.goto("/#/lumen");
  await expect(page.locator(".checks")).toContainText("Privacy is the owner's");
  await page.goto("/#/picture");
  await expect(page.locator(".setting.owner-only").first()).toBeVisible();
  await expect(page.getByRole("group", { name: "Picture mode" }).getByRole("button").first()).toBeEnabled();
  // ...and lumen-ctl refuses them, whatever the page sends.
  expect(await rawAsk(page, "privacy.state")).toMatchObject({ ok: false, error: { code: "forbidden" } });
  expect(await rawAsk(page, "settings.set", { key: "picture.backlight", value: 50 })).toMatchObject({
    ok: false,
    error: { code: "forbidden" },
  });
  expect(await rawAsk(page, "pair.open")).toMatchObject({ ok: false });
  expect(await rawAsk(page, "devices.role", { id, role: "owner" })).toMatchObject({ ok: false });
  expect(await rawAsk(page, "settings.set", { key: "picture.mode", value: "vivid" })).toMatchObject({ ok: true });

  // The PC grants owner rights: the link closes and comes back as the owner's.
  await pc.ask("devices.role", { id, role: "owner" });
  await page.goto("/#/lumen");
  await expect(page.locator(".checks li.ok").first()).toBeVisible({ timeout: 15_000 });
  expect(await rawAsk(page, "privacy.state")).toMatchObject({ ok: true });
  await page.goto("/#/pairing");
  await expect(page.locator(".pair-status")).toHaveText("Paired, with owner rights");
});

test("revoking from the PC closes the phone's link and sends it back to pairing", async ({ page, context }) => {
  const id = await pair(page, "Revoked phone");
  await page.goto("/#/remote");
  await page.locator(".chip.link[data-tone='ok']").waitFor();
  await pc.ask("devices.revoke", { id });
  await expect(page.getByRole("heading", { name: "Pair this phone" })).toBeVisible({ timeout: 10_000 });
  // The pairing page cleared its cookie, which was worth nothing anyway.
  expect((await context.cookies()).some((c) => c.name === "__Host-lumen")).toBe(false);
  expect((await page.request.get("/whoami")).status()).toBe(401);
  expect((await pc.ask<Listed>("devices.list")).devices.some((d) => d.id === id)).toBe(false);
});
