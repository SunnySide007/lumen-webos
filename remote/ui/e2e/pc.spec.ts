// SPDX-License-Identifier: GPL-3.0-or-later
//
// The PC app (remote/app) end to end on Windows: the built app, started with
// a settings folder of its own, reaches lumen-ctl through FAKE_SSH, a stand-in
// for ssh that runs `lumen-ctl serve --bus fake` (the TV in a process). The
// page is driven through WebView2's debugging port. Nothing reaches a TV.
//
//   cd remote/app && cargo build --features tauri/custom-protocol
//   FAKE_SSH=<a .cmd that runs lumen-ctl serve --bus fake> npm run e2e:pc
//
// remote/README.md has a FAKE_SSH that runs lumen-ctl in the build container.
import { spawn } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium, expect, test } from "@playwright/test";
import type { Browser, Page } from "@playwright/test";

const EXE = process.env.LUMEN_REMOTE_EXE ?? resolve("../app/target/debug/lumen-remote.exe");
const FAKE_SSH = process.env.FAKE_SSH ?? "";
const PORT = 9333;
const SHOTS = process.env.CAPTURE_DIR;

test.skip(!FAKE_SSH || !existsSync(EXE), "needs FAKE_SSH and the built app (see the top of this file)");

let app: ChildProcess | null = null;
let browser: Browser | null = null;
let dir = "";

test.afterAll(async () => {
  await browser?.close().catch(() => undefined);
  app?.kill();
  if (dir) rmSync(dir, { recursive: true, force: true });
});

async function pageFor(window: "main" | "popup"): Promise<Page> {
  for (let i = 0; i < 60; i++) {
    try {
      browser ??= await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = browser
        .contexts()
        .flatMap((c) => c.pages())
        .find((p) => p.url().includes(`window=${window}`));
      if (page) return page;
    } catch {
      // WebView2 isn't listening yet.
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`no ${window} page on port ${PORT}`);
}

async function shot(page: Page, name: string): Promise<void> {
  if (!SHOTS) return;
  // Let the view transition and the springs settle first.
  await page.waitForTimeout(900);
  await page.screenshot({ path: join(SHOTS, `pc-${name}.png`) });
}

test("the PC app: first run, settings, then power, sound, apps and no key pad", async () => {
  test.setTimeout(120_000);
  dir = mkdtempSync(join(tmpdir(), "lumen-remote-e2e-"));
  // The key only has to exist: the stand-in ssh never reads it.
  writeFileSync(join(dir, "id_ed25519"), "not a key\n");
  writeFileSync(join(dir, "tv.conf"), `port 22\nssh ${FAKE_SSH}\n`);
  app = spawn(EXE, [], {
    env: {
      ...process.env,
      LUMEN_REMOTE_DIR: dir,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`,
    },
    stdio: "ignore",
  });

  // No TV set yet: the window opens by itself, on the settings.
  const page = await pageFor("main");
  await expect(page).toHaveURL(/#\/pairing$/);
  await expect(page.locator(".chip.link")).toHaveAttribute("data-tone", "bad");
  await expect(page.locator(".chip.link")).toHaveAttribute("title", /Set the TV's address/);
  await expect(page.locator(".pc-form .hint").first()).toContainText("id_ed25519");
  await page.locator("#tv-host").fill("127.0.0.1");
  await page.locator("#tv-mac").fill("02:00:00:00:00:01");
  await page.getByRole("button", { name: "Save and connect" }).click();
  await expect(page.locator(".chip.link")).toHaveAttribute("data-tone", "ok", { timeout: 30_000 });
  await expect(page.locator(".chip.link")).toHaveAttribute("title", /lumen-ctl .* at ssh:\/\/root@127\.0\.0\.1/);
  await shot(page, "settings");

  // Remote: lumen-ctl offers no `key`, so no pad, no Back/Home/Menu, no
  // playback; power and typing stay.
  await page.goto(page.url().replace(/#.*$/, "#/remote"));
  await expect(page.locator(".chip.link")).toHaveAttribute("data-tone", "ok", { timeout: 30_000 });
  await expect(page.locator(".remote")).toHaveClass(/no-keys/);
  await expect(page.locator(".pad")).toBeHidden();
  await expect(page.locator(".key-row")).toBeHidden();
  await expect(page.locator(".play-row")).toBeHidden();
  await expect(page.locator(".notice")).toContainText("no route for keys");
  await expect(page.getByRole("button", { name: "Turn off" })).toBeEnabled();
  await expect(page.locator("#type-field")).toBeEnabled();
  await shot(page, "remote");
  await page.getByRole("button", { name: "Screen off" }).click();
  await expect(page.locator(".chip.power")).toBeVisible();
  await page.getByRole("button", { name: "Screen on" }).click();
  await expect(page.locator(".chip.power")).toBeHidden();

  // Sound: the fake TV's scripted Magic Remote press (14 to 15) shows, then
  // the page's own changes come back as events.
  await page.locator('.nav-link[data-route="sound"]').click();
  await expect(page.locator(".vol-big")).toHaveText("15");
  await page.getByRole("button", { name: "Volume up" }).click();
  await expect(page.locator(".vol-big")).toHaveText("16");
  await expect(page.locator(".chip.vol .vol-num")).toHaveText("16");
  const mute = page.locator(".mute-button");
  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await shot(page, "sound");

  // Apps and inputs: apps.list, and a launch that the foreground follows.
  await page.locator('.nav-link[data-route="apps"]').click();
  await expect(page.locator(".tile.kind-input").first()).toBeVisible();
  await page.locator(".tile", { hasText: "YouTube" }).click();
  await expect(page.locator(".chip.front")).toContainText("YouTube");
  await shot(page, "apps");

  // The popup is the same page, loaded hidden and not connected until shown.
  const popup = await pageFor("popup");
  await expect(popup.locator(".chip.link")).toHaveAttribute("data-tone", "busy");
});
