# Play (parallel track): games, a controller and the phone

Proposed on 2026-10-21 as [decision 7](../decisions.md). It runs beside
Phases 1–3 and is approved by the owner before anything is built. The research
is in [research.md, "Play"](../research.md#play-emulators-controllers-and-the-phone),
and the B9's measured input stack is in [b9.md](../device/b9.md#input-devices).

## Where it starts

* **Games:** webosbrew's RetroArch port (1.22.2, GLES 3 build for webOS 4.10+)
  runs libretro cores and needs no root. Lumen doesn't write an emulator.
* **Controller:** an Xbox Series pad (`045e:0b12`) binds to the kernel's
  `xpad` over USB, but a 15-second read gave no input. LG's Bluetooth stack
  (`webos-bluetooth`, no BlueZ) is untested with a gamepad. This is the
  biggest risk, so it comes first.
* **Phone:** an Android phone can't host a game stream, because no Android
  host exists. Its routes are a USB-C to HDMI cable, Screen Share, or acting
  as a pad.
* **Limits:**
  * 4× Cortex-A53 with a 32-bit userland, Mali-T820 MP2 (OpenGL ES 3.2), about
    335 MB of RAM free.
  * 272 MB free on `/var`, so ROMs live on a FAT or NTFS USB drive.
  * USB 2.0 only.
  * No clock speeds are exposed, so every speed claim comes from a run on the
    set.

## Rules for every milestone

* **One pull request each,** on its own branch, ending with a check on the B9
  that's recorded with model and firmware (CLAUDE.md).
* **The TV:** [safety.md](../safety.md) applies. Ask before any `tools/tv.sh`
  command. Anything persistent is run by hand first, and the owner confirms
  it.
* **Games:** only games the owner dumped from their own cartridges and discs,
  or homebrew. Lumen ships no ROMs, BIOS files or links to them.
* **Privacy:** no telemetry, no cloud services and no box-art scraping. A
  listener on the LAN is off by default and needs pairing.
* **Workflow:** milestones run with `/ecc:orch-add-feature` and stop for TV
  steps and failures. Rust changes get `/ecc:rust-review` and
  `/ecc:astra-review` before their PR.

## Milestones

| | Milestone | Needs | Can run beside |
|---|---|---|---|
| P0 | `lumen-tv-check` skill (done: on main on 2026-10-22, 8ec43a9) | — | P1 |
| P1 | One controller working end to end (parked) | — | P0 |
| P2 | The shell reads the pad | P1, shell B3 merged | P3 |
| P3 | RetroArch on the B9, measured | P1 (for input) | P2 |
| P4 | Games row | P2, P3 | — |
| P5 | OLED care (and, if the owner wants it, game mode) | P3 | P6 |
| P6 | The phone path | P3 | P5 |
| P7 | Owner-only features (pick from five) | P4 | — |

P1 and P4 need the strongest model, because P1 may mean a user-space driver
and P4 is the main design work. The rest run on the default model.

### P0: the `lumen-tv-check` skill

**Brief.** Every later milestone checks itself on the TV the same way. A
project skill in `.claude/skills/lumen-tv-check/` does that check:

1. Build the IPK with `tools/dev.sh make ipk`, from whichever checkout the
   milestone is in.
2. Ask the owner, then install it and launch it with `tools/tv.sh`.
3. Collect the log's frame statistics for a set time.
4. Close the app and restore what was there before.
5. Report the numbers, with model and firmware, for the milestone to record.

It never runs a TV step without asking.

**Done when:** the skill measures `hello-gles` on the B9 and gets b9.md's
steady 60 fps, then runs the home screen's B2 frame-time check from the
shell worktree.

**Done on 2026-10-22.** The skill is on main (8ec43a9). It measured
`hello-gles` at 60.0 fps on 2026-10-21, and ran the B2/B3 frame-time check
on 2026-10-22 (c500289, report `tools/out/archive/tv-check-20261022T015118Z`;
the numbers are in [look-and-feel.md](look-and-feel.md#snappy), target 1).

**Undo:** the skill is a repo file; the app it installs is removed with
`tools/tv.sh remove`.

### P1: one controller working end to end

**Brief.** The Series pad has to send input to an app on the B9. The routes
go from least change to most, and the first that works is the one used:

1. **Look and compare (read-only):**
   * By cable, the pad's Xbox button keeps blinking (2026-10-21): the
     start-up handshake never completes, so the Series pad needs route 2 or 3.
   * Does a pad this kernel knows send input? Candidates: an Xbox
     360-protocol pad (many third-party pads have that mode), or a DualShock 4
     by cable through the built-in `hid-sony`.
   * The read uses a probe kept as `tools/tv/pad-probe.sh` (today's scratch
     script: find the pad's node, read it for 15 s).
2. **Pair it over Bluetooth (with the owner's OK; pairing changes the TV's
   state):**
   * Pair through LG's `com.webos.service.bluetooth2` (discovery, then pair).
   * See whether a HID input device appears and the probe reads events.
   * **Undo:** unpair with `adapter/unpair`, or from LG's Bluetooth menu.
   * **Tried 2026-10-21; it fails for the Series pad.** It pairs as a BLE
     device, but HID never connects, and a GATT link drops before its services
     can be read (b9.md). Route 3 is next.
3. **Drive it from user space (if both fail):**
   * A small root helper takes interface 0 from `xpad` through `usbfs` and
     sends the pad's start-up packets.
   * It reads the pad's reports and presents a standard pad through
     `uinput`.
   * It starts by hand through Homebrew Channel's root `exec` service, never
     at boot, until it has been tested by hand.
   * **Undo:** stop the helper and unplug the pad; `xpad` binds again.

**Done when:** on the B9, every button, both sticks and both triggers of one
pad show up in the probe, and an SDL 2.0.5 test reads them as a game
controller through Lumen's own mapping. Record which route won, plus the
pad's LED, in b9.md.

**Risks:**
* LG's stack may not offer the HID profile.
* The Series pad's start-up sequence may need more than its power-on packet.
* Route 3 adds a root process, which needs a security review.

**Parked on 2026-10-21, the owner's call.** Routes 1 and 2 don't work for the
Series pad: by cable it never starts, and over Bluetooth it pairs but never
connects. Route 3 waits. When P1 comes back, in order:
1. **Research:** which start-up packets Linux's current `xpad` sends a Series
   pad (`045e:0b12`), and whether the 4.4 driver can be made to send them
   without replacing it.
2. **Route 3,** the user-space USB helper. This is the brute-force route.
3. **Bluetooth, deeper:** does LG's 2019 stack support what the pad's link
   needs (LE Secure Connections, HID over GATT)? Can `/gatt` or `/hid/internal`
   hold the link?
4. **The cheap way out:** a pad this kernel knows (an Xbox 360-mode pad, or a
   DualShock 4 by cable or classic Bluetooth). It unblocks P2 at once.

Tonight's TV scripts are kept in `tools/out/play-p1/` (not in git): `pad-read`,
`pad-diag`, `bt-methods`, `bt-pair` and `bt-gatt-diag`, with their output. The
pad is still paired with the TV until it's unpaired.

**While P1 is parked:**
* P0 carried on and is done (2026-10-22).
* P3 can measure RetroArch with the Magic Remote, and its pad check waits.
* P6's phone-as-a-pad route matters more now.
* P2 and P4 wait.

### P2: the shell reads the pad

**Brief.** The shell (Rust, SDL 2.0.5, built on the B3 launcher) opens game
controllers. It adds `SDL_GameControllerAddMapping` entries for the pads P1
proved, because SDL 2.0.5 predates them.
* **Controls:** the D-pad and left stick move focus with the remote's repeat
  timing; A is OK and B is Back. The Xbox button is left to P4's design.
* **Hot-plug:** plugging and unplugging the pad works, and the remote keeps
  working throughout.

**Done when:** `make check` is green, and on the B9 the pad navigates at 60 fps
with frame times recorded. The log shows how long each input takes to move
focus, and unplugging the pad mid-move leaves the shell working.

**Undo:** reinstall the previous IPK.

### P3: RetroArch on the B9, measured

**Brief.** Install webosbrew's RetroArch GLES 3 IPK as an ordinary app, pinned
to a version and hash in a PINNED.md like Glasshouse's. Install cores with
RetroArch's Online Updater.

Measure each system with games the owner provides on a USB drive: NES (FCEUmm),
SNES (Snes9x 2005), Genesis (Genesis Plus GX), GB/GBA (mGBA), PS1 (PCSX
ReARMed, which has its own BIOS replacement) and N64 (Mupen64Plus-Next, GLES 3
only). For each, record:
* frame rate and frame time, from RetroArch's statistics
* audio drop-outs
* whether the P1 pad drives it
* whether LG's own screensaver interrupts play (it may count only remote
  input)

Also answer the Games row's question: **can a given game start directly?**
The port starts through `jailer` with RetroArch's normal arguments
(`-L <core> <game>`), which would need root through Homebrew Channel's `exec`
service. The alternative is launching RetroArch and using its playlists.

**Done when:** a per-system table with model and firmware is in a new
`docs/device/b9-play.md`, and the direct-launch answer is recorded.

**Undo:** `tools/tv.sh remove com.retroarch.webos`, and the USB drive comes out.

### P4: the Games row

**Brief.** A Games row in the shell holds RetroArch, ScummVM and Moonlight when
they are installed. If P3 found a direct launch, it also holds per-game tiles
from a scan of the USB drive, with box art the owner supplies (local files
only).
* **Return:** coming back from a game lands on the tile that launched it.
* **Design:** Codex proposes three directions through `/ecc:multi-frontend`,
  with the liquid-glass-design skill. Claude writes the code, and the owner
  picks a direction in `tools/sim.ps1`.
* **Drawing limits:** every direction stays within what the shell draws:
  rects, rounded shapes, the icon and text atlas, at most ~300,000 px of glass,
  none over video, and no repaint once settled.
* **No in-game overlay:** Lumen can't draw over RetroArch, which is a separate
  app surface, so there isn't one.

**Done when:** on the B9 the row holds 60 fps while moving and launches each
title. The log has the time from tile to first game frame, and Back from
RetroArch returns to the row.

**Undo:** reinstall the previous IPK.

### P5: OLED care, and game mode if the owner wants it

**Brief.**
* **OLED care,** in Lumen's RetroArch settings file, applied with a backup:
  * integer scaling and the correct aspect ratio
  * a dim, slowly moving fill in the 4:3 bars, if a shader or overlay can do it
    within budget
  * pause and dim after a set idle time, then hand over to the HDR screensaver
* **LG's settings:** Screen Shift and Logo Luminance stay as LG set them.
* **Game mode:** switching LG's picture mode to Game during play would change
  LG's settings, which [architecture.md](../architecture.md) keeps untouched.
  It's built only if the owner changes that, and then only as an opt-in
  toggle that restores the previous mode on exit.

**Done when:** the idle dim and pause work on the B9, the bars' fill costs no
frames in P3's measurement, and the screensaver handover works from a paused
game.

**Undo:** restore the backed-up RetroArch settings.

### P6: the phone path

**Brief.**
* **The phone as a screen:**
  * If the owner's phone has DisplayPort Alt Mode, a USB-C to HDMI cable into
    a spare input shows up as an HDMI tile in the shell, with no code.
  * Otherwise, Screen Share is the wireless route.
  * Measure each route's lag from a slow-motion video of a button press and
    the screen.
* **The phone as a pad,** trying RetroArch's Remote RetroPad from RetroArch on
  the phone to RetroArch on the TV (UDP, port 55400 by default). It has no
  authentication, so it's turned on per session and off afterwards, and never
  at boot.

**Done when:** b9-play.md has the measured lag of each route that exists, and
a verdict on whether Remote RetroPad is usable.

**Undo:** remove the cable, and turn network remote off in RetroArch.

### P7: owner-only features (the owner picks)

| Feature | Value | Mechanism | Undo | Cost |
|---|---|---|---|---|
| Continue playing on Top Shelf | tvOS-style resume, one click back into the last game | Read RetroArch's content history playlist (local JSON) | Turn the shelf off | Small; needs P3's direct launch |
| Phone remote over Wi-Fi | Type, browse and control without the Magic Remote | A small server in Lumen with a one-time pairing code; input through `uinput` if P1 built the helper | Off by default; disabling it stops the listener | Medium; security review, a LAN listener |
| Saves backed up to the PC | Saves survive a reset or a dead USB drive | A host-side `tools/play-backup.sh` that pulls saves over SSH into a folder the owner chooses | Delete the folder | Small; no cloud |
| HDMI-CEC handoff | Picking a console tile powers it on and switches input | LG's CEC services through luna (to be researched) | Turn it off per tile | Medium; CEC must keep working (safety rule 7) |
| Ambient between games | A paused game hands over to the HDR loops, and back on any input | Lumen's screensaver path from Phase 2 | Turn it off | Small, after Phase 2 |

## Open questions

1. Has the owner got a second pad (Xbox 360 mode or DualShock 4) and a USB
   drive for P1 and P3?
2. Which phone model, for P6?
3. Does the owner want game mode, which means setting aside "LG's settings
   untouched"?
4. Which P7 features, if any?
