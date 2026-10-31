// SPDX-License-Identifier: GPL-3.0-or-later
//
// What every screen gets: the link, the TV's state, and a few helpers.
import type { LinkState, RemoteClient } from "./client.ts";
import { RequestError } from "./client.ts";
import type { PairingStub } from "./pairing.ts";
import type { ArgsOf, ResultOf, Verb } from "./protocol.ts";
import type { Store, TvState } from "./store.ts";

export interface SnackAction {
  label: string;
  run: () => void;
}

export interface Ctx {
  client: RemoteClient;
  tv: Store<TvState>;
  link: Store<LinkState>;
  pairing: PairingStub;
  /** Sends a request; on failure says why in the snackbar and gives null. */
  send<V extends Verb>(verb: V, args?: ArgsOf<V>): Promise<ResultOf<V> | null>;
  /** A short message at the bottom, with an optional action such as Undo. */
  say(message: string, action?: SnackAction): void;
}

export interface Screen {
  el: HTMLElement;
  /** Called after the screen is on the page: focus, first reads. */
  shown?(): void;
  destroy(): void;
}

/** What a failed request says, in a few words. */
export function failureText(verb: Verb, err: unknown): string {
  if (!(err instanceof RequestError)) return `${verb} failed`;
  switch (err.code) {
    case "not-connected":
      return "Not connected to the TV";
    case "timeout":
      return "The TV didn't answer";
    case "disconnected":
      return "The link to the TV dropped";
    case "unavailable":
      return `The TV can't do that now: ${err.message}`;
    default:
      return `The TV refused ${verb}: ${err.message}`;
  }
}

/** Keeps `controls` enabled only while the link is open and the server
 * accepts every one of `verbs`. Returns the unsubscribe. */
export function enableWhen(
  link: Store<LinkState>,
  controls: (HTMLButtonElement | HTMLInputElement)[],
  ...verbs: Verb[]
): () => void {
  return link.subscribe((s) => {
    const on = s.kind === "open" && verbs.every((v) => s.verbs.has(v));
    for (const c of controls) c.disabled = !on;
  });
}

/** Whether the server accepts `verb` right now. */
export function accepts(s: LinkState, verb: Verb): boolean {
  return s.kind === "open" && s.verbs.has(verb);
}
