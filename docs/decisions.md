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
