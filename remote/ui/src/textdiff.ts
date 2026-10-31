// SPDX-License-Identifier: GPL-3.0-or-later
//
// Typing on the TV. The text field on the Remote screen is compared with what
// was last sent after every change, and the difference becomes text.delete
// and text.insert. Comparing whole values, not keystrokes, copes with a
// phone keyboard's composition, autocorrect and paste the same way.
import { MAX_DELETE, MAX_INSERT } from "./protocol.ts";
import type { Verb } from "./protocol.ts";

export interface TextEdit {
  /** Characters to delete from the end of what was sent. */
  delete: number;
  /** Text to type after that. */
  insert: string;
}

/** The edit that turns `before` into `after`: keep their common start,
 * delete the rest of `before`, type the rest of `after`. Counts are in code
 * points, so an emoji is one character. */
export function diffText(before: string, after: string): TextEdit {
  const a = [...before];
  const b = [...after];
  let same = 0;
  while (same < a.length && same < b.length && a[same] === b[same]) same++;
  return { delete: a.length - same, insert: b.slice(same).join("") };
}

export type TextCommand =
  | { verb: Extract<Verb, "text.delete">; args: { count: number } }
  | { verb: Extract<Verb, "text.insert">; args: { text: string } };

/** The requests for an edit, each within the protocol's limits. Characters
 * the protocol refuses (control characters, such as a pasted newline) are
 * dropped. */
export function editCommands(edit: TextEdit): TextCommand[] {
  const out: TextCommand[] = [];
  for (let left = edit.delete; left > 0; left -= MAX_DELETE) {
    out.push({ verb: "text.delete", args: { count: Math.min(left, MAX_DELETE) } });
  }
  // eslint-disable-next-line no-control-regex
  const chars = [...edit.insert.replace(/[\u0000-\u001f\u007f-\u009f]/g, "")];
  for (let i = 0; i < chars.length; i += MAX_INSERT) {
    out.push({ verb: "text.insert", args: { text: chars.slice(i, i + MAX_INSERT).join("") } });
  }
  return out;
}
