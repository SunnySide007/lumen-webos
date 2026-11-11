// SPDX-License-Identifier: GPL-3.0-or-later
//
// One smoke test per screen, against the mock: each control sends its verb,
// and the page follows what the TV pushes, whoever changed it.
import { expect, test } from "@playwright/test";
import { OtherClient, open, watchRequests } from "./tv.ts";

let other: OtherClient;
test.beforeEach(() => {
  other = new OtherClient();
});
test.afterEach(() => other.close());

test("Remote: keys, keyboard and typing become verbs", async ({ page }) => {
  const sent = watchRequests(page);
  await open(page, "remote");
  await page.getByRole("button", { name: "OK", exact: true }).click();
  await page.locator("body").press("ArrowLeft");
  await page.getByRole("button", { name: "Back" }).click();
  const field = page.getByPlaceholder("Type into the TV's text field");
  await field.pressSequentially("hey");
  await field.press("Backspace");
  await field.press("Enter");
  await expect
    .poll(() => sent.map((r) => `${r.verb} ${JSON.stringify(r.args)}`))
    .toEqual(
      expect.arrayContaining([
        'key {"name":"ok"}',
        'key {"name":"left"}',
        'key {"name":"back"}',
        'text.insert {"text":"h"}',
        'text.insert {"text":"y"}',
        'text.delete {"count":1}',
        "text.enter {}",
      ]),
    );
  await expect(page.locator(".snack.show")).toHaveCount(0);
});

test("Remote: the touchpad, the rockers, the number keys and a shortcut become verbs", async ({ page }) => {
  const sent = watchRequests(page);
  await open(page, "remote");
  await page.getByRole("button", { name: "Touchpad", exact: true }).click();
  const pad = page.locator(".touchpad");
  await expect(pad).toBeVisible();
  await expect(page.locator(".pad")).toBeHidden();
  const box = (await pad.boundingBox())!;
  const [cx, cy] = [box.x + box.width / 2, box.y + box.height / 2];
  // A swipe of 100 px is two arrows; a quick touch is OK.
  await page.mouse.move(cx - 50, cy);
  await page.mouse.down();
  await page.mouse.move(cx, cy, { steps: 4 });
  await page.mouse.move(cx + 50, cy, { steps: 4 });
  await page.mouse.up();
  await page.mouse.click(cx, cy);
  await page.getByRole("button", { name: "Channel up" }).click();
  await page.getByRole("button", { name: "Volume up" }).click();
  await page.getByText("Numbers and colours").click();
  await page.getByRole("button", { name: "7", exact: true }).click();
  await page.getByRole("button", { name: "Red", exact: true }).click();
  await page.locator(".shortcut", { hasText: "YouTube" }).click();
  await expect
    .poll(() => sent.map((r) => `${r.verb} ${JSON.stringify(r.args)}`))
    .toEqual(
      expect.arrayContaining([
        'key {"name":"right"}',
        'key {"name":"ok"}',
        'key {"name":"channelup"}',
        "volume.up {}",
        'key {"name":"7"}',
        'key {"name":"red"}',
      ]),
    );
  expect(sent.filter((r) => r.verb === "key" && r.args.name === "right")).toHaveLength(2);
  expect(sent.some((r) => r.verb === "launch")).toBe(true);
  // The choice of touchpad is kept for the next visit.
  await page.reload();
  await expect(page.locator(".touchpad")).toBeVisible();
});

test("Remote: the touchpad as a pointer moves and clicks, and the choice is kept", async ({ page }) => {
  const sent = watchRequests(page);
  await open(page, "remote");
  await page.getByRole("button", { name: "Touchpad", exact: true }).click();
  const pad = page.locator(".touchpad");
  const mode = pad.getByRole("button", { name: "Pointer" });
  await expect(mode).toHaveAttribute("aria-pressed", "false");
  await mode.click();
  await expect(mode).toHaveAttribute("aria-pressed", "true");
  await expect(pad).toContainText("Drag to point");
  const box = (await pad.boundingBox())!;
  const [cx, cy] = [box.x + box.width / 2, box.y + box.height / 2];
  await page.mouse.move(cx - 40, cy + 30);
  await page.mouse.down();
  await page.mouse.move(cx, cy + 30, { steps: 5 });
  await page.mouse.move(cx + 40, cy + 30, { steps: 5 });
  await page.mouse.up();
  await page.mouse.click(cx, cy + 30);
  await expect.poll(() => sent.some((r) => r.verb === "pointer.click")).toBe(true);
  const moves = sent.filter((r) => r.verb === "pointer.move");
  expect(moves.length).toBeGreaterThan(0);
  const dx = moves.reduce((n, r) => n + (r.args.dx as number), 0);
  // 80 px of travel less the slop, at a gain of 2.2 or more.
  expect(dx).toBeGreaterThan(140);
  expect(moves.every((r) => r.args.dy === 0)).toBe(true);
  // A drag as a pointer presses no arrow.
  expect(sent.some((r) => r.verb === "key")).toBe(false);
  await expect(page.locator(".snack.show")).toHaveCount(0);
  await page.reload();
  await expect(page.locator(".touchpad").getByRole("button", { name: "Pointer" })).toHaveAttribute("aria-pressed", "true");
});

test("The thin remote: one column with the pad, the rockers and shortcuts", async ({ page }) => {
  const sent = watchRequests(page);
  await page.setViewportSize({ width: 140, height: 700 });
  await page.goto("/?window=side");
  await page.locator(".side-light[data-tone='ok']").waitFor();
  await expect(page.locator(".nav")).toHaveCount(0);
  await page.getByRole("button", { name: "OK", exact: true }).click();
  await page.getByRole("button", { name: "Home" }).click();
  await page.getByRole("button", { name: "Volume down" }).click();
  await page.locator("body").press("ArrowUp");
  await expect
    .poll(() => sent.map((r) => `${r.verb} ${JSON.stringify(r.args)}`))
    .toEqual(expect.arrayContaining(['key {"name":"ok"}', 'key {"name":"home"}', "volume.down {}", 'key {"name":"up"}']));
  // Nothing is cut off at the side: the column fits its window.
  const wide = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(wide).toBeLessThanOrEqual(140);
});

test("Sound: follows a volume change made elsewhere, and mutes", async ({ page }) => {
  await open(page, "sound");
  await other.send("volume.set", { level: 37 });
  await expect(page.locator(".vol-big")).toHaveText("37");
  await expect(page.locator(".chip.vol .vol-num")).toHaveText("37");
  await page.getByRole("button", { name: "Volume up" }).click();
  await expect(page.locator(".vol-big")).toHaveText("38");
  const mute = page.locator(".mute-button");
  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await other.send("mute.set", { muted: false });
  await expect(mute).toHaveAttribute("aria-pressed", "false");
});

test("Apps and inputs: lists apps.list and launches", async ({ page }) => {
  await open(page, "apps");
  await expect(page.locator(".tile")).toHaveCount(16);
  await expect(page.locator(".tile.kind-input.connected")).toHaveCount(1);
  await page.locator(".tile", { hasText: "YouTube" }).click();
  await expect(page.locator(".tile.in-front")).toContainText("YouTube");
  await expect(page.locator(".chip.front")).toContainText("YouTube");
  await other.send("launch", { id: "com.webos.app.hdmi1" });
  await expect(page.locator(".tile.in-front")).toContainText("HDMI 1");
});

test("Picture and sound: shows the value, writes on request, undoes", async ({ page }) => {
  await open(page, "picture");
  const mode = page.getByRole("group", { name: "Picture mode" });
  const vivid = mode.getByRole("button", { name: "Vivid" });
  const cinema = mode.getByRole("button", { name: "Cinema", exact: true });
  await expect(cinema).toHaveAttribute("aria-pressed", "true");
  await vivid.click();
  await expect(vivid).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".snack.show")).toContainText("Picture mode: Cinema → Vivid");
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(cinema).toHaveAttribute("aria-pressed", "true");
  await other.send("settings.set", { key: "sound.mode", value: "music" });
  const sound = page.getByRole("group", { name: "Sound mode" });
  await expect(sound.getByRole("button", { name: "Music" })).toHaveAttribute("aria-pressed", "true");
});

test("Lumen: privacy state, and starting ambient", async ({ page }) => {
  await open(page, "lumen");
  await expect(page.locator(".checks li.ok")).toHaveCount(7);
  await expect(page.locator(".checks")).toContainText("Port 11111 closed to the LAN");
  await expect(page.locator(".checks")).toContainText("Guard: 4,812 lookups");
  await page.getByRole("button", { name: /Start ambient/ }).click();
  await expect(page.locator(".chip.power")).toContainText("Screensaver");
  await other.send("key", { name: "ok" });
  await expect(page.locator(".chip.power")).toBeHidden();
});

test("Pairing: a page the TV didn't serve says how a phone pairs", async ({ page }) => {
  // The mock's hello carries no device: pairing itself is e2e/phone.spec.ts,
  // against lumen-remote and lumen-ctl.
  await open(page, "pairing");
  await expect(page.locator(".pair-status")).toContainText("Not paired through the TV");
  await expect(page.getByRole("button", { name: "Pair a phone" })).toHaveCount(0);
});

test("The link: drops when the TV turns off and comes back by itself", async ({ page }) => {
  await open(page, "remote");
  const off = page.getByRole("button", { name: "Turn off" });
  await off.click();
  await page.getByRole("button", { name: "Turn off the TV?" }).click();
  await expect(page.locator(".chip.link")).toHaveAttribute("data-tone", "bad");
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeDisabled();
  // The mock wakes after 3 s; the client retries with backoff.
  await expect(page.locator(".chip.link")).toHaveAttribute("data-tone", "ok", { timeout: 15_000 });
  await expect(page.getByRole("button", { name: "OK", exact: true })).toBeEnabled();
});

test.describe("with reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("nothing tilts and screens change without a transition", async ({ page }) => {
    await open(page, "apps");
    const tile = page.locator(".tile").first();
    await tile.hover({ position: { x: 10, y: 10 } });
    await expect(tile).not.toHaveClass(/tilting/);
    expect(await tile.evaluate((el) => el.style.getPropertyValue("--tilt-x"))).toBe("");
    // Count View Transitions from here on.
    await page.evaluate(() => {
      const w = window as unknown as { transitions: number };
      const doc = document as unknown as { startViewTransition: (cb: () => void) => unknown };
      const real = doc.startViewTransition.bind(document);
      w.transitions = 0;
      doc.startViewTransition = (cb) => {
        w.transitions++;
        return real(cb);
      };
    });
    await page.getByRole("link", { name: "Sound" }).click();
    await expect(page.locator(".screen-title")).toHaveText("Sound");
    expect(await page.evaluate(() => (window as unknown as { transitions: number }).transitions)).toBe(0);
  });
});

test("Every screen: no request the page sends is refused", async ({ page }) => {
  const errors: string[] = [];
  page.on("websocket", (ws) =>
    ws.on("framereceived", (f) => {
      if (typeof f.payload === "string" && f.payload.includes('"ok":false')) errors.push(f.payload);
    }),
  );
  page.on("pageerror", (e) => errors.push(e.message));
  for (const route of ["remote", "sound", "apps", "picture", "lumen", "pairing"]) {
    await open(page, route);
    await expect(page.locator(".screen-title")).not.toBeEmpty();
  }
  expect(errors).toEqual([]);
});
