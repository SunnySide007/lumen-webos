// SPDX-License-Identifier: GPL-3.0-or-later
//
// npm run mock [-- options]: the mock lumen-ctl, with a console that stands in
// for the Magic Remote. See remote/README.md.
import { existsSync } from "node:fs";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { POWER_STATES } from "../../ui/src/protocol.ts";
import type { PowerState } from "../../ui/src/protocol.ts";
import { startDemo } from "./demo.ts";
import { APPS } from "./fixtures.ts";
import { startServer } from "./server.ts";
import { FakeTv } from "./tv.ts";

const USAGE = `usage: npm run mock -- [options]

  --port <n>           port to listen on (8765)
  --host <addr>        address to listen on (127.0.0.1; 0.0.0.0 lets a phone
                       on the LAN in, with nothing but the origin check)
  --static <dir>       the built UI to serve at / (../ui/dist when it exists)
  --no-static          serve no page, only the WebSocket at /ctl
  --allow-origin <o>   another origin allowed to open the WebSocket (repeat)
  --demo               change the volume every 5 s and the app every 15 s
  --no-keys            act as a TV with no route for keys (remote.md risk 1)
  --guard              give Lumen Guard counts in privacy.state
  --off-seconds <n>    how long power.off keeps the TV off (15)
  --quiet              log nothing but the address
`;

const CONSOLE_HELP = `console, standing in for the Magic Remote and LG's menus:
  volume <0-100>     mute on|off       app <id>          apps
  power active|screen-off|screensaver|standby
  input <1-4> on|off [device name]     silence <seconds> drop
  help               quit`;

function main(): void {
  let args;
  try {
    args = parseArgs({
      options: {
        port: { type: "string", default: "8765" },
        host: { type: "string", default: "127.0.0.1" },
        static: { type: "string" },
        "no-static": { type: "boolean", default: false },
        "allow-origin": { type: "string", multiple: true, default: [] },
        demo: { type: "boolean", default: false },
        "no-keys": { type: "boolean", default: false },
        guard: { type: "boolean", default: false },
        "off-seconds": { type: "string", default: "15" },
        quiet: { type: "boolean", default: false },
        help: { type: "boolean", default: false },
      },
    }).values;
  } catch (err) {
    console.error(`${(err as Error).message}\n\n${USAGE}`);
    process.exit(2);
  }
  if (args.help) {
    console.log(USAGE);
    return;
  }
  const port = Number(args.port);
  const offSeconds = Number(args["off-seconds"]);
  if (!Number.isInteger(port) || port < 0 || port > 65535 || !(offSeconds >= 0)) {
    console.error(`bad --port or --off-seconds\n\n${USAGE}`);
    process.exit(2);
  }
  const builtUi = fileURLToPath(new URL("../../ui/dist/", import.meta.url));
  const staticDir = args["no-static"] ? null : (args.static ?? (existsSync(builtUi) ? builtUi : null));
  const log = args.quiet ? () => {} : (line: string) => console.log(`${clock()} ${line}`);

  const tv = new FakeTv({ keys: !args["no-keys"], guard: args.guard, log });
  void startServer({
    tv,
    host: args.host,
    port,
    staticDir,
    allowOrigins: args["allow-origin"],
    offSeconds,
    server: "mock-ctl 0.1.0",
    log,
  }).then((server) => {
    const where = `http://${args.host === "0.0.0.0" ? "<this PC's address>" : args.host}:${server.port}`;
    console.log(`mock-ctl: WebSocket at ${where.replace("http", "ws")}/ctl`);
    console.log(staticDir ? `mock-ctl: the UI at ${where}/ (from ${staticDir})` : "mock-ctl: no UI served");
    if (args.demo) {
      startDemo(tv);
      console.log("mock-ctl: demo on: the volume changes every 5 s, the app every 15 s");
    }
    if (process.stdin.isTTY) {
      console.log(CONSOLE_HELP);
      runConsole(tv, server);
    }
  });
}

function clock(): string {
  return new Date().toTimeString().slice(0, 8);
}

function runConsole(tv: FakeTv, server: Awaited<ReturnType<typeof startServer>>): void {
  const rl = createInterface({ input: process.stdin });
  rl.on("line", (input) => {
    const [cmd, a, ...rest] = input.trim().split(/\s+/);
    switch (cmd) {
      case undefined:
      case "":
        return;
      case "volume": {
        const level = Number(a);
        if (Number.isInteger(level) && level >= 0 && level <= 100) tv.setVolume(level, tv.muted);
        else console.log("volume takes 0 to 100");
        return;
      }
      case "mute":
        tv.setVolume(tv.volume, a !== "off");
        return;
      case "app":
        if (a && APPS.some((x) => x.id === a)) tv.setForeground(a);
        else console.log("app takes one of the ids `apps` lists");
        return;
      case "apps":
        console.log(APPS.map((x) => `  ${x.id}  (${x.title})`).join("\n"));
        return;
      case "power":
        if ((POWER_STATES as readonly string[]).includes(a ?? "")) tv.setPower(a as PowerState);
        else console.log(`power takes ${POWER_STATES.join(", ")}`);
        return;
      case "input": {
        const n = Number(a);
        if (n >= 1 && n <= 4) tv.setInput(n - 1, rest[0] !== "off", rest.slice(1).join(" ") || "A device");
        else console.log("input takes 1 to 4, then on or off");
        return;
      }
      case "silence":
        server.silence(Number(a) || 30);
        return;
      case "drop":
        server.dropAll("dropped from the console");
        return;
      case "quit":
        void server.close().then(() => process.exit(0));
        return;
      default:
        console.log(CONSOLE_HELP);
    }
  });
}

main();
