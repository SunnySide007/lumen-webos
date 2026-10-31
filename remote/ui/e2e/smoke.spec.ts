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

test("Pairing: a wrong code, then a paired stub", async ({ page }) => {
  await open(page, "pairing");
  const code = page.getByLabel("Pairing code, six digits");
  await code.fill("000000");
  await page.getByRole("button", { name: "Pair" }).click();
  await expect(page.locator(".pair-status")).toContainText("4 of 5 tries left");
  await code.fill("482913");
  await page.getByRole("button", { name: "Pair" }).click();
  await expect(page.locator(".pair-status")).toContainText("Paired");
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
