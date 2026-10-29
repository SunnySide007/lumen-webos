# Decisions

Architecture decision records, newest last. Each states the context, the
choice, and what it costs. A decision is changed by adding a record that
supersedes it, not by editing the old one.

## 1. A native home screen (Rust, SDL2, OpenGL ES 2)

**Context.** The B9's web engine is Chromium 53 (2016): no `backdrop-filter`,
no CSS Grid, ES5 only, on a Mali-T820 MP2. The look asked for (tvOS 26's glass,
focus lift, parallax) needs GPU control a web app on that engine cannot get,
and the existing web launchers report black frames on open and dropped frames
even on far faster TVs. plx-native runs a native Rust + SDL2 + GLES2 UI at
60 fps, with a Liquid Glass material, on the same hardware class.

**Decision.** The shell is native Rust on SDL2 and GLES2, built with webosbrew's
toolchain. SDL2 is loaded at run time rather than linked.

**Cost.** A heavier toolchain (handled by the Docker image) and fewer casual
contributors than a web app would attract. Glass is still limited to small,
grouped regions by the GPU.

## 2. GPL-3.0-or-later

**Context.** plx-native, the most relevant prior art, is GPL-3.0-or-later.
Reusing its webOS, GPU and media code is the fastest route to a solid shell.

**Decision.** Lumen is GPL-3.0-or-later. Files written for upstream projects
under other licenses (for example a Glasshouse screensaver, MIT) say so in
their header and are dual-licensed. Original video content is CC BY-SA 4.0.

**Cost.** Anyone building on Lumen must keep their work open, which is the
point for a proof of concept, but rules out closed forks.

## 3. Runtime overlays only

**Context.** webOS system partitions are signed squashfs. Custom firmware for
LG TVs is not a realistic or safe route, and a firmware update can remove root.

**Decision.** Lumen never flashes or writes system partitions. It uses bind
mounts, configd values it owns, and boot scripts, each with an undo. See
[safety.md](safety.md).

**Cost.** Mods must be reapplied at every boot, and a few things (the boot
logo, early-boot behaviour) are out of reach.

## 4. Glasshouse as the system layer

**Context.** Glasshouse (MIT, verified on webOS 4 including a C9) already does
ad and telemetry blocking, privacy toggles, switching off LG services and
preloaded apps, key remapping, OLED care, backups and recovery, with careful
documentation of every TV change. Its own home screen only runs on webOS 9/10.

**Decision.** Install Glasshouse as-is and configure it from
`profiles/glasshouse`. Lumen builds what it lacks (the native home, HDR
screensavers) and contributes fixes and the B9's hardware profile upstream.

**Cost.** A dependency on a young, fast-moving project: Lumen pins a tested
Glasshouse version and reruns its regression checklist on each update. The two
must not fight over Home or the screensaver (see architecture.md).

## 5. Aggressive privacy, but store-safe

**Context.** The owner wants aggressive debloating, and also needs Netflix and
Prime Video to keep updating through the LG Content Store. On webOS 4 the store
runs on `lgtvsdp.com`, which Glasshouse's strictest tier blocks.

**Decision.** Glasshouse's "ads & telemetry" tier, every privacy toggle off,
LG's diagnostics, ACR and voice services off, plus a custom hosts list for
ThinQ, voice, AI and recommendation hosts, measured from a 24-hour DNS capture
of the TV. A hard allowlist (store, DRM, Netflix, Amazon including AWS, Plex)
is never blocked, and `tools/store-safe-check.sh` proves it after each change.

**Cost.** Voice search, the ThinQ app and LG Channels stop working, and that is
documented where the profile is applied.

## 6. Glasshouse pinned, its dashboard on localhost, and an on-TV capture (proposed)

**Status.** Proposed on 2026-10-20, before any of it ran on the TV. It
becomes accepted, with any changes, after Phase 1's checks pass on the B9
([runbook](runbooks/phase1-tv.md)).

**Context.** Decision 4 makes Glasshouse the system layer and decision 5 asks
for a block list measured from the TV. Glasshouse moves fast (0.118.0 came
out on 2026-10-19), its dashboard has no password and listens on every
interface by default (`"host": "0.0.0.0"`), and neither Glasshouse nor
webosbrew logs DNS. The TV resolves through a connman proxy on 127.0.0.1, so
a capture on the PC or the router misses which app asked, and the router (a
Google Nest Wifi) cannot log queries.

**Decision.**

- Pin Glasshouse at **0.118.0**, installed only from the release IPK whose
  SHA-256 matches GitHub's published digest (`287f1a5c…8e2a`,
  profiles/glasshouse/PINNED.md). A newer version is taken only after the
  same checks pass again.
- Run the dashboard **on localhost only** (`"host": "127.0.0.1"` in
  `/var/lib/tvweb/config.json`) and reach it with `tools/tv.sh tunnel`, an
  SSH forward bound to the PC's loopback. Never port-forward it.
- **Capture on the TV** with a static tcpdump (4.99.7, libpcap 1.10.7, built
  from signed, hash-pinned sources by `tools/capture/build.sh`), filtered to
  DNS, TCP SYNs, TLS ClientHellos and QUIC long headers, time-boxed with
  `timeout`, size-capped in a 500 MB ring under `/media/developer/temp`, and
  never started at boot. `tools/capture/summarize.py` turns it into the host
  table the block list is written from.

**Cost.** Upgrading Glasshouse is a deliberate task, not a click. Using the
dashboard needs a PC with the SSH key, and Glasshouse's phone setup link
still opens on the LAN during setup. The capture puts a root packet sniffer
on the TV for a day at a time; it holds names and addresses, not content,
and its files never leave `tools/out/`, which git ignores.

## 7. Play: RetroArch for games, one controller proven first (proposed)

**Status.** Proposed on 2026-10-21, before any of it was built. The owner
approved the plan ([specs/play.md](specs/play.md)) as written the same day,
and it becomes accepted after P1 and P3 pass on the B9.

**Context.** The owner wants games on the B9 with an Xbox Series controller,
and their Android phone as a backup. webosbrew already ports RetroArch to
webOS (1.22.2, a GLES 3 build for 4.10+). The B9's CPU is a Raspberry Pi 3's
class, so systems up to PS1 should run. By cable the Series pad binds to the
kernel's `xpad` but sends no input, and its light keeps blinking. LG's
Bluetooth stack is its own, with no BlueZ, and it's untested with a gamepad.
No Android app can host a game stream (research.md, "Play").

**Decision.**

- **Use RetroArch** (webosbrew's GLES 3 build, pinned by version and hash) as
  the games runtime instead of writing an emulator. Lumen adds the Games row,
  controller mappings, OLED care and the phone path around it.
- **Prove one controller end to end first** (milestone P1), in order:
  1. a pad this kernel already knows
  2. Bluetooth pairing through LG's stack
  3. a root helper that drives the pad over `usbfs` and presents it through
     `uinput`, started by hand, never at boot until tested.

  Nothing in the shell depends on a pad before then.
- **Local only:** games the owner dumped or homebrew, with no ROMs, BIOS files
  or links shipped. No box-art scraping and no cloud. A LAN listener is off by
  default and paired.
- **LG's picture settings stay as they are.** An automatic Game picture mode
  is built only if the owner sets that rule aside, and then only as an opt-in
  toggle that restores the previous mode.

**Cost.** RetroArch's own menus, not Lumen's, run during play, and Lumen
can't draw over them. Starting a game straight from a tile may need root
through Homebrew Channel's `exec` service, and P3 finds out. If neither of
the first two routes works, the pad needs a small root helper, and that
helper needs a security review. Speed claims wait for measurements, because
the B9 doesn't expose its clock speeds.

## 9. Remote: Lumen's own control path, LG's second-screen gateway closed (proposed)

**Status.** Proposed on 2026-10-28, before any of it was built. It becomes
accepted, with any changes, once the owner approves
[specs/remote.md](specs/remote.md) and its R2 passes on the B9. Number 8 is
taken by the home-loop record on `refactor/loop-pass`.

**Context.** The owner wants to control the B9 from their Windows PC, from
their Pixel's browser and with Google Home, and from away through a VPN. The
PC app is a tray popup with bold web animations. LG's usual routes are:

* **Its second-screen API** on ports 3000 and 3001 (SSAP). Its PIN check
  could be skipped on the B9's generation (CVE-2023-6317, confirmed on
  webOS 4.9.7), and a pre-auth chain through 3001 was published in October
  2026 for webOS 7.5. The B9's updates stay blocked (safety rule 8), so the
  gateway won't be fixed, and nobody at home uses it.
* **The ThinQ cloud,** which decision 5 blocks.

**Decision.**

- **Close SSAP to the LAN** (privacy P10) and build Lumen's own path:
  `lumen-ctl` on the TV, the only part with root, with a fixed list of verbs.
- **The PC app** (Tauri 2) reaches it through the SSH door that's already
  there, with a key that can run only `lumen-ctl`. The PC adds no port.
- **The phone** reaches it through `lumen-remote`, an unprivileged HTTPS
  server on one new port. It's off by default and paired with a code shown
  on the TV.
- **Power on** is Wake-on-LAN from the PC, and HDMI-CEC through the Google TV
  Streamer from Google Home.
- **Google Home** goes through the Streamer's CEC first. Home Assistant runs
  on a separate always-on box only if CEC isn't enough, and never through
  SSAP.
- **No telemetry and no updater** in the PC app.

**Cost.**

* **Code exposed on the TV:** a new LAN listener (R4) and a root process that
  reads requests (R2), each needing a security review. With 3000 and 3001
  closed, the LAN still sees fewer of the TV's doors than today.
* **Remote keys** may have no clean route on webOS 4.
* **Data:** Google hears every voice command, and the PC app's WebView2 sends
  Windows' required diagnostic data whatever Lumen does.
* **LG's phone apps** (ThinQ, and remote apps that use SSAP) stop working.
