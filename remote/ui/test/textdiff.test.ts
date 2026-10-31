// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { diffText, editCommands } from "../src/textdiff.ts";

describe("diffText", () => {
  it.each([
    ["", "h", 0, "h"],
    ["h", "he", 0, "e"],
    ["hello", "hell", 1, ""],
    ["teh", "the ", 2, "he "],
    ["abc", "", 3, ""],
    ["same", "same", 0, ""],
    ["hi 👋", "hi", 2, ""],
    ["hi", "hi 👋🏽", 0, " 👋🏽"],
  ])("%j to %j deletes %i and types %j", (before, after, del, ins) => {
    expect(diffText(before, after)).toEqual({ delete: del, insert: ins });
  });
});

describe("editCommands", () => {
  it("deletes before it types", () => {
    expect(editCommands({ delete: 2, insert: "he " })).toEqual([
      { verb: "text.delete", args: { count: 2 } },
      { verb: "text.insert", args: { text: "he " } },
    ]);
  });

  it("keeps each request within the protocol's limits", () => {
    const cmds = editCommands({ delete: 300, insert: "x".repeat(600) });
    expect(cmds.map((c) => ("count" in c.args ? c.args.count : c.args.text.length))).toEqual([
      256, 44, 256, 256, 88,
    ]);
  });

  it("drops control characters, such as a pasted newline", () => {
    expect(editCommands({ delete: 0, insert: "a\nb\t" })).toEqual([
      { verb: "text.insert", args: { text: "ab" } },
    ]);
    expect(editCommands({ delete: 0, insert: "\n" })).toEqual([]);
  });
});
