// SPDX-License-Identifier: GPL-3.0-or-later
//
// Browser checks against the mock, which serves the built page as lumen-remote
// will: `npm run build` first. PW_CHANNEL picks an installed browser
// (msedge or chrome) instead of Playwright's own download.
import { defineConfig } from "@playwright/test";

export const PORT = 8799;

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  workers: 1,
  fullyParallel: false,
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    channel: process.env.PW_CHANNEL || undefined,
    colorScheme: "dark",
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: `node ../mock-ctl/src/main.ts --port ${PORT} --static dist --guard --off-seconds 3 --quiet`,
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: false,
    timeout: 20_000,
  },
  projects: [
    { name: "smoke", testMatch: /smoke\.spec\.ts/ },
    { name: "capture", testMatch: /capture\.spec\.ts/ },
  ],
});
