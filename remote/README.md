# Lumen Remote: the web front-end and a mock lumen-ctl

[docs/specs/remote.md](../docs/specs/remote.md) plans control of the TV from
the owner's PC (R3, a Tauri 2 tray app) and Pixel (R4, a page the TV serves),
both through `lumen-ctl`, a root verb server on the TV. Neither exists yet.
This directory holds what both will show, and a stand-in for the TV:

| Path | What |
|---|---|
| `ui/` | The one web front-end: TypeScript and CSS, no framework, bundled by Vite. Remote, Sound, Apps and inputs, Picture and sound, Lumen, and Pairing |
| `ui/src/protocol.ts` | The protocol, typed: every verb, its arguments, its result, every event, and the checks. The source of truth that `lumen-ctl` (R2) is written against |
| `mock-ctl/` | A Node WebSocket server that speaks the protocol with a fake TV: it checks arguments as `lumen-ctl` will, changes its state and pushes events. No TV, no root, no luna |

Nothing here runs on the TV yet, and nothing touches one.

## Run it

Node 22.18 or newer (the mock runs its TypeScript directly).

```sh
cd remote/mock-ctl && npm ci && npm run mock     # ws://127.0.0.1:8765/ctl
cd remote/ui && npm ci && npm run dev            # http://localhost:5173, /ctl goes to the mock
```

Or the way the TV will serve it: `npm run build` in `ui/`, then `npm run
mock` serves `ui/dist/` and the WebSocket on one port, at
http://127.0.0.1:8765/.

The mock's options (`npm run mock -- --help`):

* `--demo` (or `npm run demo`): the volume changes every 5 s and the app in
  front every 15 s, as if someone held the Magic Remote.
* `--no-keys`: a TV with no route for keys (remote.md, risk 1). `key` is left
  out of `hello`, and the Remote screen says so.
* `--guard`: Lumen Guard's counts in `privacy.state`. Without it, Guard
  doesn't exist yet, as on the B9 today.
* `--off-seconds <n>`: how long `power.off` keeps the TV off (15). Every
  client is dropped and refused until it "wakes", which shows the UI
  reconnecting with backoff.
* `--host 0.0.0.0`: lets a phone on the LAN in. Only the origin check guards
  it: no pairing, no TLS. For trying the layout on a phone at home, nothing
  more.
* In a terminal, the mock reads commands that stand in for the TV's own
  remote: `volume 30`, `mute on`, `app netflix`, `power screen-off`,
  `input 2 on PlayStation`, `silence 40` (sends nothing, to show the stale
  badge), `drop`.

The page connects to `/ctl` on the host that served it. To reach another
address (the PC app's case), add `?ctl=ws://host:port/ctl`, or type it on the
Pairing screen under "This device".

## Checks

In each of `ui/` and `mock-ctl/`:

```sh
npm ci && npm run lint && npm test && npm run build
```

`lint` is ESLint (type-checked rules) and `tsc --noEmit`; `test` is vitest.
`build` in `ui/` writes `dist/` and fails if it is over 600 KB gzipped (today
about 73 KB); in `mock-ctl/` it type-checks.

The browser checks drive the built page against the mock with Playwright:
one smoke test per screen, the link dropping and coming back, and reduced
motion. They need a Chromium; `PW_CHANNEL=msedge` (or `chrome`) uses an
installed one instead of Playwright's download.

```sh
cd remote/ui && npm run build && PW_CHANNEL=msedge npm run e2e
CAPTURE_DIR=<dir> PW_CHANNEL=msedge npm run capture   # screenshots, recording frames
python scripts/frames-to-gif.py <dir>                 # the recording as a GIF (Pillow)
```

## The protocol

One JSON object per line, in each direction. Over SSH the lines end in
`\n`; over a WebSocket each text message carries one line.

```text
client  {"id": 7, "verb": "volume.set", "args": {"level": 20}}
server  {"id": 7, "ok": true, "result": {}}
        {"id": 7, "ok": false, "error": {"code": "bad-args", "message": "level must be an integer from 0 to 100"}}
        {"event": "volume", "data": {"level": 20, "muted": false}}
```

The server speaks first: `hello` with the protocol version (1) and the verbs
it accepts, then the current value of each watched state as events, the way a
luna subscription's first reply carries it. After that, an event whenever a
state changes, whoever changed it, and `alive` every 10 s.

| Verb | Arguments | Result |
|---|---|---|
| `power.off`, `screen.off`, `screen.on` | none | `{}` |
| `volume.up`, `volume.down` | none | `{}` |
| `volume.set` | `level`: integer 0–100 | `{}` |
| `mute.set` | `muted`: boolean | `{}` |
| `apps.list` | none | `apps`: `[{id, title, kind: "app" \| "input", iconColor: "#rrggbb" \| null}]` |
| `launch` | `id`: an app or input id (`[A-Za-z0-9][A-Za-z0-9._-]{0,127}`) | `{}` |
| `key` | `name`: up, down, left, right, ok, back, home, menu, play, pause, stop, rewind, fastforward | `{}` |
| `text.insert` | `text`: 1–256 characters, no control characters | `{}` |
| `text.delete` | `count`: integer 1–256 | `{}` |
| `text.enter` | none | `{}` |
| `lumen.open`, `ambient.start` | none | `{}` |
| `privacy.state` | none | `{hook: {installed, log}, port11111: {rejected}, jobs: [{name, running}], guard: {since, lookups, blocked} \| null}` |
| `settings.get` | `keys` (optional): settings from the list below | `values`: `{key: value}` |
| `settings.set` | `key`, `value`: one setting and an allowed value | `{}` |
| `toast` | `text`: 1–120 characters | `{}` |

| Setting | Values |
|---|---|
| `picture.mode` | vivid, standard, eco, cinema-home, cinema, sports, game, filmmaker, expert-bright, expert-dark |
| `picture.oledLight` | integer 0–100 |
| `picture.energySaving` | auto, off, minimum, medium, maximum |
| `sound.mode` | ai, standard, cinema, clear-voice, sports, music, game |

Events: `hello`, `power` (`active`, `screen-off`, `screensaver`, `standby`),
`volume` (`level`, `muted`), `foreground` (`id` or null), `inputs` (`[{id,
label, device, connected}]`), `settings` (the values that changed) and
`alive`.

Errors: `bad-line` (not JSON, not an object, too long, or no usable id; the
reply's id is then null), `unknown-verb`, `bad-args`, `not-found`,
`unavailable` (a verb the TV can't do now) and `failed` (LG's service
refused). Unknown fields, in a request or in its arguments, are refused.

### Where remote.md left the shape open

These are this directory's choices; `protocol.ts` has them in code.

1. **Envelope:** `id`, `verb`, `args` (an object, omitted when empty); replies
   `ok` with `result` or `error`; events `event` and `data`.
2. **Actions answer `{}`.** The state they change arrives as an event, so the
   page that asked and every other client update the same way.
3. **The snapshot is events, not a verb.** remote.md's verb table has no
   "get state", so after `hello` the server sends each watched state once.
4. **`hello` lists the verbs** the server accepts, so a TV without a key
   route (risk 1) can leave `key` out and the UI can say why.
5. **`alive` every 10 s.** The UI calls the link stale after 25 s with no line
   at all, and drops and reconnects it after 45 s.
6. **Settings and key names are Lumen's,** not LG's: `lumen-ctl` maps them to
   settingsservice keys and the key route once R0 has read those on the B9.
   The setting values follow LG's menu names; R0 confirms which the B9 has.
7. **`text.delete` counts code points,** and `text.insert` takes at most 256
   at once; the UI splits longer edits.
8. **A line is at most 64 KiB.** Request ids are integers 1 to 2³¹−1, chosen
   by the client.
9. **A `settings` event** pushes setting changes, like volume; it isn't in
   remote.md's list of pushed state (power, volume, app in front, inputs).
10. **Icons:** `apps.list` carries no icon; tiles are letters on `iconColor`,
    or the shell's 8-colour palette by position. Serving icons waits for R4.

## The look

Look-and-feel's tokens (background `#0E1222` to black, text `#EBEEF5`, dim
`#8C94A5`, panels at radius 28, pills fully round, Inter SemiBold titles),
with Remote's bolder motion (D6):

* **Liquid glass:** blurred, tinted panels with a rim light that follows the
  pointer. Where the engine allows it (Chromium: WebView2 and Chrome on the
  Pixel), the dock and the pad also bend what's behind them through an SVG
  displacement filter.
* **Morphing controls:** View Transitions between screens, a navigation pill
  and setting chips that glide to the new choice, a mute icon whose waves
  give way to a cross, a power button that asks again before it acts.
  Springs are sampled into CSS `linear()` easings, so no script runs per
  frame.
* **Ambient colour:** the page takes the colour of the app or input in
  front (`iconColor` from `apps.list`, followed through `foreground`).
* **Depth and tilt:** the pad and the tiles lean toward the pointer, and
  toward the phone's tilt when "Tilt with the phone's motion" is on (off by
  default; it asks the browser's permission where needed).
* **Still when nothing happens:** no animation loops; a settled page draws
  nothing. With `prefers-reduced-motion`, nothing tilts, morphs or springs.

Inter is bundled (the variable font's Latin subset, OFL; its licence ships
in `dist/licenses/`). The built page has a content security policy that
loads only its own files and allows only WebSocket connections. It stores
nothing: no cookies, no local storage, and the pairing stub's token lives in
memory only.

## What waits

**R2, `lumen-ctl`:** speak `protocol.ts` over SSH: the same checks
(`parseRequest` is the reference), `hello` with the verbs that work on this
TV, the first value of each subscription after it, `alive`, and an event for
every change from luna's subscriptions. Map the settings and keys once R0 has
read LG's names. The mock's tests are a starting list of cases.

**R3, Tauri:** wrap `ui/` and give `RemoteClient` a transport over the SSH
forced command instead of a WebSocket (`SocketLike` in `client.ts` is the
seam); power on by Wake-on-LAN; the tray popup's size and Acrylic behind the
glass.

**R4, `lumen-remote`:** serve `ui/dist/` and `/ctl` on one port as the mock
does, over HTTPS (D4), with real pairing behind the Pairing screen
(`pairing.ts` is the stub to replace) and its own origin check.
