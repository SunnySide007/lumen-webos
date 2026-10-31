// SPDX-License-Identifier: GPL-3.0-or-later
//
// A second client of the mock, standing in for the TV's own remote or another
// phone: what it changes must show up in the page under test.
import type { Page } from "@playwright/test";
import { PORT } from "../playwright.config.ts";

export class OtherClient {
  private readonly ws: WebSocket;
  private nextId = 1;
  readonly ready: Promise<void>;

  constructor() {
    this.ws = new WebSocket(`ws://127.0.0.1:${PORT}/ctl`);
    this.ready = new Promise((resolve, reject) => {
      this.ws.onmessage = (ev) => {
        if (String(ev.data).includes('"hello"')) resolve();
      };
      this.ws.onerror = () => reject(new Error("the mock refused the second client"));
    });
  }

  async send(verb: string, args?: Record<string, unknown>): Promise<void> {
    await this.ready;
    this.ws.send(JSON.stringify({ id: this.nextId++, verb, args }));
  }

  close(): void {
    this.ws.close();
  }
}

/** Collects the requests the page sends, from its WebSocket frames. */
export function watchRequests(page: Page): { verb: string; args: Record<string, unknown> }[] {
  const sent: { verb: string; args: Record<string, unknown> }[] = [];
  page.on("websocket", (ws) => {
    ws.on("framesent", (f) => {
      if (typeof f.payload === "string") sent.push(JSON.parse(f.payload) as (typeof sent)[number]);
    });
  });
  return sent;
}

/** Opens a screen and waits for the link to the mock. */
export async function open(page: Page, route: string): Promise<void> {
  await page.goto(`/#/${route}`);
  await page.locator(".chip.link[data-tone='ok']").waitFor();
}
