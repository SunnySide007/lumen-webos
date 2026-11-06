# Architecture

## Goals

A TV that is just a TV, for one set (decision 10): the owner's LG OLED B9
does its HDMI inputs, Netflix, Prime Video, YouTube and Plex, a screensaver
made for its panel, and LG's picture and sound settings. A home screen that
opens at once and moves at 60 fps. No ads, no telemetry, no ACR. The TV
reaches the internet only for those apps and their updates, and only the
owner can loosen that.

This page describes the design accepted on 2026-11-05 (decisions 10 to 17).
Each part says whether it is built; [specs/depth.md](specs/depth.md) has
the order of the work.

## Layers

| Layer | What | Who maintains it |
|---|---|---|
| **Lumen** | The shell, `lumend`, Lumen Guard, `lumen-ctl` and `lumen-remote` on the TV; the studio, the tools, Lumen Remote and the simulator off it | This repository |
| **Root** | Homebrew Channel: root, `init.d` boot hooks, SSH by key | webosbrew, installed as is |
| **Apps** | Netflix and Prime Video (LG's Content Store), YouTube AdFree and plx-native (IPKs fetched and hash-checked on the PC, decision 12) | Their projects |
| **Platform** | webOS 4.5: the compositor, the app manager (SAM), LG's settings, the picture and sound engine, the video pipeline | LG, untouched |

There is no system layer in between: Glasshouse is never installed
(decision 13).

## On the TV

### The shell: the home screen

A jailed developer-mode app (uid 5727), the TV's Home through LG's own
`defaultApps.home` setting (route D, [takeover/README.md](../takeover/README.md)).

* **Rendering:** one batched draw list a frame in a fixed 1920 x 1080
  space; a settled screen stops presenting. Built on OpenGL ES 2 today;
  moving to ES 3.0 with ASTC heroes, cached shaders and glyphs, and partial
  redraw from diffed draw lists (decision 15). The hand-written renderer
  stays; no toolkit.
* **Platform layer:** LG's SDL 2.0.5 today. Planned: Lumen's own, in Rust,
  over the TV's `libwayland-client` and EGL loaded at run time, with its
  own lifecycle registration, so a Home press with Lumen in front, a pause
  and a close all reach the shell. SDL leaves only when the Rust layer
  wins on the B9 (decision 15).
* **Resident (planned):** started at boot hidden behind the last input,
  kept alive behind apps by its registration, SAM's keep-alive list and
  `lumend`; while hidden it frees the Top Shelf art and keeps the icon
  atlas. A Home press is then a return to the front (2 to 170 ms measured),
  not a cold start (1,173 ms).
* **Grid (decision 10):** the contract's apps, then the inputs, then
  Settings; every other launch point in one "Other apps" tile. Edited from
  the Remote.
* **Ambient (planned, decision 16):** with Lumen in front, it holds LG's
  screensaver off and plays Lumen's HDR loops on the video plane through
  LG's media libraries; screen off after 2 hours. Video sits on a hardware
  plane GL can't read, so nothing is drawn over it but the shell's own
  clear surface.
* **Control Center (planned):** LG's settings through
  `com.webos.settingsservice`, which stays their source of truth; the
  Privacy panel, read-only (decision 12); Reduce Motion, one switch per
  effect. "All Settings" opens LG's app.
* **Never on the network:** the shell opens no socket (decision 14). Its
  art and icons come from its package or LG's own files.
* **Simulator:** the same code in a window on a PC (`tools/sim.ps1`), on
  desktop GL with PNG art, for layout only.

### `lumend`: the root daemon (planned, decision 14)

One Rust process started by a boot hook's door. It holds LG's reporting
programs down (S1, ACR and the UEI scanner among them), closes LG Home
after boot, copies LG's icons for the shell, watches Lumen's file modes,
starts the shell hidden and restarts it if the memory manager closes it,
and turns the screen off after the screensaver. It reacts to the app
manager's events instead of polling. It talks to the shell over a socket
inside the shell's jail, each side checking the other's uid, and to luna
through a role of its own that allows outgoing calls only. It opens no
network socket.

### Lumen Guard: names (built, log only)

A small Rust DNS forwarder that every lookup goes through
([specs/privacy.md](specs/privacy.md)). Log only today (G1). Next: default
deny with an allowlist and learning windows (G2), then Quad9 over DNS over
TLS (G3, decision 11). It is the only part of Lumen that reaches the
internet, and it fails open on its own.

### `lumen-ctl` and `lumen-remote`: control (built, and planned)

`lumen-ctl` is the root control binary behind an SSH forced command, for
the PC ([specs/remote.md](specs/remote.md)). `lumen-remote` (planned) is
the unprivileged HTTPS server for paired phones: off by default, the one
LAN listener Lumen adds. Paired devices start remote-only; owner rights
come from the PC (decision 12).

### Boot hooks and the key filter

Each boot hook in Homebrew Channel's `init.d` is a door: a few lines of
POSIX `sh` that check the kill switch, time-box the start and run a Rust
binary. The key filter is ES5 in LG's compositor chain, delivered inert:
a long Home opens LG Home, the gear opens the Control Center on Lumen's
home, the microphone does nothing.

## Off the TV

* **`studio/`:** procedural scenes rendered to 16-bit EXR, encoded as HEVC
  Main10 HDR10 (and Dolby Vision 8.1 when the owner says), plus the art
  drawn by code (`tools/art/`). Python, in its own container.
* **`tools/`:** the build container, the TV tools (every TV step asked
  first), the surveys, the frame statistics.
* **`remote/`:** Lumen Remote's web front-end (TypeScript), shared by the
  PC app and the phone.
* **`profiles/`:** the privacy hooks and Guard's rules; the generic
  allowlist only (the owner's own additions stay private, decision 11).

## Rules that keep the layers apart

* **Only Guard reaches the internet;** `lumen-remote` is the one LAN
  listener. Everything else Lumen runs opens no socket.
* **Only the owner loosens privacy,** from the PC or a phone the PC
  promoted. The TV shows the state.
* **Software and content arrive from the PC,** hash-checked. The Content
  Store keeps updating Netflix and Prime Video.
* **LG's settings stay LG's:** read and written through LG's services,
  never by editing their files.
* **Nothing is written to a system partition,** every change is
  reversible, and nothing persists until it has passed by hand
  ([safety.md](safety.md)).

## Decisions

The reasoning behind each choice is in [decisions.md](decisions.md), and the
research it rests on, with sources, in [research.md](research.md).
