// SPDX-License-Identifier: GPL-3.0-or-later
//
// Screenshots of every screen at the PC's 1280x800 and the phone's 412x915,
// and the frames of a short recording: volume changes and an app change
// arriving from the mock. CAPTURE_DIR says where (test-results/capture by
// default); scripts/frames-to-gif.py turns the frames into a GIF. Chromium
// only: the recording uses its screencast.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { OtherClient } from "./tv.ts";

const DIR = process.env.CAPTURE_DIR ?? "test-results/capture";
const SIZES = [
  { name: "pc", viewport: { width: 1280, height: 800 }, mobile: false },
  { name: "phone", viewport: { width: 412, height: 915 }, mobile: true },
];
const ROUTES = ["remote", "sound", "apps", "picture", "lumen", "pairing"];

/** Waits until a screen has what it reads and its motion has settled. */
async function settle(page: Page, route: string): Promise<void> {
  await page.locator(".chip.link[data-tone='ok']").waitFor();
  if (route === "apps") await expect(page.locator(".tile")).toHaveCount(16);
  if (route === "lumen") await expect(page.locator(".checks li")).toHaveCount(7);
  if (route === "picture") await expect(page.locator(".chips.has-value")).toHaveCount(3);
  await page.mouse.move(0, 0);
  await page.waitForTimeout(1600);
}

test.beforeAll(async () => {
  mkdirSync(DIR, { recursive: true });
  // The same state for every run.
  const tv = new OtherClient();
  await tv.send("volume.set", { level: 14 });
  await tv.send("mute.set", { muted: false });
  await tv.send("launch", { id: "netflix" });
  await tv.send("settings.set", { key: "picture.mode", value: "cinema" });
  await tv.send("settings.set", { key: "sound.mode", value: "standard" });
  await new Promise((r) => setTimeout(r, 600));
  tv.close();
});

for (const size of SIZES) {
  test(`screens at ${size.viewport.width}x${size.viewport.height}`, async ({ browser }) => {
    const context = await browser.newContext({
      viewport: size.viewport,
      deviceScaleFactor: 1,
      isMobile: size.mobile,
      hasTouch: size.mobile,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    for (const route of ROUTES) {
      await page.goto(`/#/${route}`);
      await settle(page, route);
      await page.screenshot({ path: join(DIR, `${route}-${size.name}-${size.viewport.width}x${size.viewport.height}.png`) });
    }
    await context.close();
  });
}

test("recording: the TV changes the volume, then the app", async ({ browser }) => {
  test.setTimeout(60_000);
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: "dark" });
  const page = await context.newPage();
  await page.goto("/#/sound");
  await settle(page, "sound");

  const cdp = await context.newCDPSession(page);
  const frames: { file: string; t: number }[] = [];
  const out = join(DIR, "recording-frames");
  mkdirSync(out, { recursive: true });
  cdp.on("Page.screencastFrame", (f: { data: string; sessionId: number; metadata: { timestamp?: number } }) => {
    const file = `frame-${String(frames.length).padStart(4, "0")}.jpg`;
    writeFileSync(join(out, file), Buffer.from(f.data, "base64"));
    frames.push({ file, t: f.metadata.timestamp ?? Date.now() / 1000 });
    void cdp.send("Page.screencastFrameAck", { sessionId: f.sessionId });
  });
  await cdp.send("Page.startScreencast", { format: "jpeg", quality: 85, maxWidth: 960, maxHeight: 600 });

  const tv = new OtherClient();
  const wait = (ms: number) => page.waitForTimeout(ms);
  await wait(800);
  for (const level of [18, 23, 31, 26]) {
    await tv.send("volume.set", { level });
    await wait(1100);
  }
  await tv.send("launch", { id: "spotify-beehive" });
  await wait(2200);
  await tv.send("mute.set", { muted: true });
  await wait(1400);
  await cdp.send("Page.stopScreencast");
  tv.close();
  writeFileSync(join(out, "frames.json"), JSON.stringify({ frames, end: Date.now() / 1000 }, null, 1));
  expect(frames.length).toBeGreaterThan(10);
  await context.close();
});
