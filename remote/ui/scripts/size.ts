// SPDX-License-Identifier: GPL-3.0-or-later
//
// Prints the size of every file in dist/, raw and gzipped, and fails if the
// whole is over the budget: the TV serves this to a phone over Wi-Fi.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const BUDGET_KB = 600;
const root = fileURLToPath(new URL("../dist/", import.meta.url));

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

let raw = 0;
let gz = 0;
for (const path of files(root).sort()) {
  const bytes = readFileSync(path);
  const zipped = gzipSync(bytes, { level: 9 }).length;
  raw += bytes.length;
  gz += zipped;
  const name = relative(root, path).replaceAll("\\", "/");
  console.log(`${name.padEnd(48)} ${kb(bytes.length).padStart(9)} ${kb(zipped).padStart(9)} gz`);
}
console.log(`${"total".padEnd(48)} ${kb(raw).padStart(9)} ${kb(gz).padStart(9)} gz (budget ${BUDGET_KB} KB)`);
if (gz > BUDGET_KB * 1024) {
  console.error(`dist/ is ${kb(gz)} gzipped, over the ${BUDGET_KB} KB budget`);
  process.exit(1);
}

function kb(n: number): string {
  return `${(n / 1024).toFixed(1)} KB`;
}
