# Architecture

## Goals

A TV that is just a TV: a home screen that opens instantly and moves at 60 fps,
no ads or telemetry, LG's picture and sound engine untouched, the streaming
apps working and updating, and screensavers that show off an OLED panel.

## Two layers

| Layer | What | Who maintains it |
|---|---|---|
| **Experience** | `shell/` (home screen), `takeover/` (Home key), `ambient/` (HDR screensaver), `studio/` (content), `profiles/` (settings recipes) | This repository |
| **System** | Homebrew Channel (root, boot hooks, SSH), Glasshouse (privacy, blocking, services, OLED care, key remapping, backups), plx-native (Plex) | Upstream projects, installed from Homebrew Channel and configured, not forked |
| **Platform** | webOS 4.5: Settings, the picture and sound engine, LG's apps, Netflix, Prime Video | LG, untouched |

Lumen adds what the system layer lacks rather than rebuilding it: Glasshouse's
own home screen runs only on webOS 9 and 10, and its screensavers are SDR
graphics.

## Components

### `shell/`: the home screen

Native Rust on SDL2 and OpenGL ES 2, the approach plx-native proved at 60 fps
on this hardware class. SDL2 is loaded at run time, so the binary links only
glibc and runs on every webOS release from the oldest supported up.

* **Rendering:** one batched draw list per frame in a fixed 1920×1080 space,
  letterboxed on a PC. A settled screen stops repainting.
* **Budget** (plx-native's measurements on an α7 Gen 2 / Mali-T820 MP2 set):
  16.7 ms a frame, about 9 ms already spent by a moving grid. Backdrop-blur
  glass fits only when the blurred region stays under ~300,000 px, which is a
  top bar or a dock, not a full screen. Glass can never sit over playing video:
  video is on a hardware plane the GPU cannot read.
* **Settings:** the Control Center reads and writes through
  `com.webos.settingsservice` and `com.webos.service.tvpower`, so LG's
  settings stay the source of truth. "All Settings" opens LG's Settings app.
* **Simulator:** the same code runs in a window on a PC (`tools/sim.ps1`), using
  desktop GL so colours match the TV (see the comment in `screen.rs`).

### `takeover/`: making Home open the shell (Phase 3)

Route D ([takeover/README.md](../takeover/README.md)): LG's own setting,
settingsservice's `defaultApps.home`, names the shell, so the compositor's
Home handler opens it on a short press, with `tools/takeover.sh undo` as
the way back. A boot hook closes LG's Home, which SAM still preloads, while
the shell is Home. A small ES5 handler for the compositor's key-filter
chain (configd's `com.webos.surfacemanager.keyFilters`) makes holding Home
open LG's Home; it is delivered inert, for the owner to try step by step.

### `ambient/`: HDR screensavers (Phase 2)

webOS's screensaver is a QML app; a bind mount over its `main.qml` replaces it
(the proven custom-screensaver mechanism). Lumen's QML plays a playlist of
HDR10 and Dolby Vision loops: original ones from `studio/`, optional Apple
aerials streamed at run time, and the owner's own videos. It goes upstream as
a Glasshouse screensaver if that fits; until then Glasshouse's screensaver
stays on "LG default" so only one tool owns the mount.

### `studio/`: HDR content (Phase 2)

An offline pipeline: procedural scenes rendered to 16-bit linear EXR, encoded
with x265 as HEVC Main10, BT.2020 / PQ, with mastering-display and MaxCLL
metadata, plus optional Dolby Vision Profile 8.1 via `dovi_tool`. Scenes are
made for OLED: near-black, low average light, small highlights mastered to the
panel's ~700–800 nit peak, seamless loops.

### `profiles/glasshouse/`: privacy, store-safe (Phase 1)

A settings recipe and a custom hosts list for Glasshouse: its "ads & telemetry"
tier plus measured extras, with the Content Store, DRM, Netflix, Amazon and
Plex hosts on a hard allowlist. Glasshouse's "everything" tier is not used: on
webOS 4 it blocks `lgtvsdp.com`, which is the Content Store.

## Rules that keep the layers apart

* Glasshouse Home stays off; Lumen's shell owns Home.
* One owner at a time for the screensaver mount.
* Lumen's key filter is its own entry, handles Home only, and sits after
  Glasshouse's filter; Glasshouse's Remote keys never offers Home.

## Decisions

The reasoning behind each choice is in [decisions.md](decisions.md), and the
research it rests on, with sources, in [research.md](research.md).
