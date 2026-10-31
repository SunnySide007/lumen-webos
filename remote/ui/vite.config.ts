// SPDX-License-Identifier: GPL-3.0-or-later
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// Where `npm run dev` and `npm run preview` forward /ctl: the mock by default.
const CTL = process.env.LUMEN_CTL ?? "ws://127.0.0.1:8765";

// The built page loads only its own files and talks only to a WebSocket:
// the TV that served it, or the address typed on the Pairing screen.
const CSP = [
  "default-src 'self'",
  "connect-src 'self' ws: wss:",
  "img-src 'self' data:",
  "font-src 'self'",
  "style-src 'self'",
  "script-src 'self'",
  "base-uri 'none'",
  "form-action 'none'",
].join("; ");

/** Adds the content security policy to the built page only: Vite's dev
 * server injects styles inline, which the policy would block. */
function csp(): Plugin {
  return {
    name: "lumen-csp",
    apply: "build",
    transformIndexHtml: () => [
      {
        tag: "meta",
        attrs: { "http-equiv": "Content-Security-Policy", content: CSP },
        injectTo: "head-prepend",
      },
    ],
  };
}

export default defineConfig({
  plugins: [csp()],
  build: {
    target: "es2023",
    modulePreload: { polyfill: false },
    assetsInlineLimit: 0,
    reportCompressedSize: false,
  },
  server: { proxy: { "/ctl": { target: CTL, ws: true } } },
  preview: { proxy: { "/ctl": { target: CTL, ws: true } } },
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
  },
});
