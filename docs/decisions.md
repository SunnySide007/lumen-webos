# Decisions

Architecture decision records, newest last. Each states the context, the
choice, and what it costs. A decision is changed by adding a record that
supersedes it, not by editing the old one.

## 1. A native home screen (Rust, SDL2, OpenGL ES 2)

**Status.** Partly superseded by 15 (2026-11-05): Rust and OpenGL ES stay;
SDL leaves once the shell's own platform layer wins on the B9.

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

**Status.** Its last sentence is superseded by 17 (2026-11-05).

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

**Status.** Superseded by 13 (2026-11-05): Glasshouse is never installed.

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

**Status.** Its custom hosts list is superseded by 11 (2026-11-05); the
store-safe rule stands.

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

**Status.** Superseded by 13 (2026-11-05) before it was accepted.
Proposed on 2026-10-20, before any of it ran on the TV. It
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

## 8. The home loop behind adapters, tested through fakes

**Status.** Accepted by the owner on 2026-10-31, after `lumen-tv-check`
ran it on the B9 back to back with 3a89955: the missed vsyncs, frame-3
times and key to frame were the same; start-up was not compared like for
like, since the two were launched over different apps ("Measured on the
B9", 17, in specs/shell-mvp.md). Proposed on 2026-10-23 with the code that
carries it out ([specs/shell-mvp.md](specs/shell-mvp.md), "Home loop:
deepening the loop"), to be accepted once that check showed the same
missed vsyncs, frame-3 times, key to frame and start-up as 3a89955; merged
into `main` on 2026-10-31.

**Context.** The home loop (GLOSSARY.md) called SDL, GL and the clock from
inside one 323-line function, and no test reached it. Three of the four bugs
fixed in 5726aa3 lived there: the dip drawn at dt 0, a repeat after the
release, and the `fps` window spanning a hide. The tests that touched the
loop acted parts of it out by hand, and they had drifted from it. Each
feature since the timing lines had added 180 to 451 lines to `app.rs`.

**Decision.**

- The loop runs behind **three adapters**, each a trait with the TV's
  version and a test's (`home_loop/fake.rs`):
  - **events**: SDL's queue on the TV.
  - **screen**: SDL's window and GL on the TV; in tests, the real draw code
    on the CPU atlas, with the uploads recorded.
  - **wall clock**: the system's on the TV; in tests, a manual one that the
    fakes move.
- **The loop keeps every decision**: when to wait, draw, warm, take in
  icons, read GL's flag and launch. The adapters only carry them out, so a
  fake can record them. Paced motion, `--fixed-dt` and `--keys` stay
  policies inside the loop.
- **Generics, not `dyn`**, and a pass allocates nothing new.
- **`run` keeps start-up**: SDL, the window and GL, the renderer, the
  assets, listing the apps, the watch thread and the start phases. The
  loop owns the rest, the `start:` line included.
- The loop's behaviour is tested **at its interface**, through the fakes,
  not by acting part of it out by hand.

**Cost.** Three traits and a fake module have to keep step with the loop.
The fakes paint with the real draw code but never call GL, so GL's own
costs (uploads, the swap's wait) show only in a TV run, and the fake's
slow-frame lines say `faults -` every time. Start-up hands the loop the
moment its clock starts and the moment listing began, so the stamps, the
deadlines and the watch's last input stay where they were.

## 9. Remote: Lumen's own control path, LG's second-screen gateway closed (proposed)

**Status.** Proposed on 2026-10-28, before any of it was built. The owner
approved [specs/remote.md](specs/remote.md) as drafted on 2026-10-31; the
record becomes accepted once R2 passes on the B9. Number 8 is
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

## 10. A TV that is just a TV, for one set

**Status.** Accepted by the owner on 2026-11-05, in a design session with
three research reports behind it (research.md, "Research of 2026-11-05"),
before any of it was built. Records 10 to 17 come from that session.

**Context.** Lumen began as a proof of concept "for rooted LG webOS TVs",
with a Homebrew Channel release planned (the roadmap's Phase 5). The owner
wants complete control of one TV that does what a TV should and nothing
more, for a family. The B9's driver reports OpenGL ES 3.2, and its
compositor and app manager have quirks no other set shares.

**Decision.**

- **One set.** The owner's OLED55B9PUA (webOS 4.10.2, firmware 05.50.00)
  is the only supported target, and Lumen may lean on what it has: ES 3,
  its compositor's protocol, its app manager. The code stays public under
  GPL-3.0-or-later. On any other set Lumen refuses to start, with a message
  that says why, rather than half working.
- **The contract.** The TV does the HDMI inputs; Netflix, Prime Video,
  YouTube (the ad-free build, D3) and Plex; the screensaver; and LG's
  picture and sound settings. Everything else LG ships is off or out of
  reach: voice, ThinQ, LG Channels, ads, ACR, the LG account, browsing the
  store, firmware updates. The TV reaches the internet only for those apps
  and their updates, and the Content Store keeps updating Netflix and
  Prime Video. A feature fits the contract, or a new record changes it.
- **The grid shows the contract.** The first row holds the contract's
  apps, then come the inputs, then Settings. Every other launch point,
  Live TV and LG Home among them, sits in one "Other apps" tile at the
  end, one press away. The order and contents are edited from the Remote
  (phone or PC), not on the TV.
- **Power-on goes to the last input,** as LG does; the shell loads hidden
  behind it (record 15).
- **Each of the remote's buttons does one thing.** The gear opens Lumen's
  Control Center on Lumen's home and LG's quick settings inside apps
  (unless record 15's overlay trial passes); the microphone does nothing;
  a long Home opens LG Home; Guide and Live go nowhere. All of it goes
  through the ES5 key filter, delivered inert and turned on step by step.
- **Release means the owner's rebuild.** Phase 5's bar becomes: the B9
  rebuilt from a factory reset with only this repository's docs and the
  owner's backups. No Homebrew Channel submission; fixes go upstream when
  they are cheap.
- **The name stays Lumen,** with a line in the README saying what this one
  is: a web-based "Lumen" for webOS 6 and later appeared in October 2026.

**Cost.** Nobody else's TV is tested or helped. Live TV, LG Channels,
voice and ThinQ are a press further away or gone. Editing the grid needs
the phone or the PC. Anything the family wants outside the contract needs
a new record first.

## 11. Block by default, one resolver, few open doors

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).
Each stage becomes real only after its check on the B9
([specs/privacy.md](specs/privacy.md)).

**Context.** privacy.md's G2 was going to block names on a list and allow
everything else. Guard's week of log (G1, to 2026-11-07) shows what the B9
asks for. The research of 2026-11-05 found:

* Mullvad's public DNS closed on 2026-11-02 and dns0.eu in 2025. Quad9 is
  a Swiss foundation that records no client addresses.
* On webOS 4.10, LG's ACR program reads the consent flag `acrOnAllowed`,
  which Lumen never recorded, and starts at boot whatever the flags say.
* LG's UEI program (`iconnectivity`) sweeps the home network with reverse
  lookups.
* Ports 3000 and 3001 carry root bugs confirmed on a 2019 webOS 4.9.7 set,
  and no fixed webOS 4.5 firmware has been seen.
* The Nest Wifi Pro has no per-device outbound rules; its only isolation
  is the guest network.

**Decision.**

- **Default deny (G2).** Guard answers 0.0.0.0 and `::` for every name
  that isn't on the allowlist. The allowlist starts from G1's week and
  always holds the store and DRM hosts. Suffix rules take carve-outs:
  names under an allowed suffix that stay blocked, such as the store's
  request log, `elastic.lgwebostv.com`. The Privacy panel lists what was
  denied.
- **New apps learn in a window.** The owner starts it by hand from the
  phone or the PC ("learn for 15 minutes"). Guard allows and logs new
  names during it, then shows the list to keep or drop. Never automatic;
  one window at a time.
- **The public repository holds only the generic list:** the store, DRM
  and the contract's apps, with a reason per name. The owner's own
  additions stay on the TV, in the owner's backups and in a git-ignored
  folder on the PC, since they say what the household watches.
- **Imported lists keep their source.** hagezi's LG list (GPL-3.0),
  Perflyst's SmartTV list (MIT) and Glasshouse's host names (MIT) live in
  files of their own, with source and license, apart from the names Lumen
  measured. They feed the carve-outs and the address rules.
- **One resolver (G3; P4 answered).** Quad9, `dns.quad9.net` (9.9.9.9,
  which passes no client subnet), over DNS over TLS: rustls, ring and
  webpki-roots on a plain socket, no async runtime, no second provider.
  Guard forwards in clear through the router only while the TV's clock is
  implausible after a power cut and while Quad9 can't be reached, and it
  logs and shows that it did; only allowed names can leave that way. Until
  G3, the owner sets the Nest's DNS to Quad9 for the whole house.
- **The TV stays on the home network** and enforces everything itself.
  The guest network would cut phones off from casting, and the Nest can't
  filter one device.
- **Few open doors (P9 and P10 answered).** Stateless `INPUT` rules reject
  3000 and 3001, the DLNA ports (1103, 1537, 1551, 1830, 1833, 1973),
  AirPlay (7000) and HomeKit; a rule for a port inside the ephemeral range
  matches `--syn` only. DIAL (36866, 18181), Netflix (9080), SSDP (1900),
  mDNS and SSH stay open. One rule at a time, each followed by a casting
  check.
- **One address rule:** an `OUTPUT` reject for LG's own 156.147.0.0/16,
  once a capture shows the store doesn't need it. Never AWS or a CDN.
- **Two more programs held down (S1).** After a read-only look at their
  values: LG's ACR consent flags off (`acrOnAllowed`, `acrAdAllowed`,
  `additional1Allowed`) with `acr2` held down from boot, and UEI's
  `iconnectivity` held down by a do-nothing stand-in mounted over it in
  RAM.

**Cost.** A new app breaks until its learning window, and an app that
moves to a new host breaks until the owner allows it; the store-safe check
and the denied list are how that shows. Quad9 sees every name the TV looks
up. During a clock bootstrap or an outage, allowed names leave in clear.
With AirPlay, DLNA and 3000/3001 closed, Apple devices, DLNA senders and
LG's phone apps can't reach the TV. TLS adds an estimated 1 to 2 MB to
Guard, to be measured.

Supersedes decision 5's custom hosts list (its store-safe rule stands) and
privacy.md's blocklist wording for G2.

## 12. Only the owner loosens the TV

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).

**Context.** The family uses the TV, Lumen Remote pairs phones (remote.md,
R4), and Homebrew Channel can fetch and update apps by itself.

**Decision.**

- **The Privacy panel on the TV is read-only.** It shows what is blocked,
  what was denied and what each hook did. Allowing a name, starting a
  learning window or turning a layer off happens only from a device with
  owner rights.
- **Paired devices start remote-only:** power, volume, inputs, apps, keys
  and picture presets. Owner rights come only from the PC, with its SSH
  key, and the PC revokes any device.
- **Software arrives only from the PC.** Every IPK (YouTube AdFree,
  plx-native, Homebrew Channel's updates, Lumen itself) is fetched and
  hash-checked on the PC, then pushed. Homebrew Channel's repository and
  update hosts stay off the allowlist. The Content Store keeps updating
  Netflix and Prime Video, as the contract says.
- **Content too.** Videos the owner adds to the screensaver come from the
  PC or the LAN; the TV streams nothing for it.

**Cost.** Changing privacy, or updating an app, needs the PC or a phone
the PC promoted. Homebrew Channel's own update notices stop.

## 13. No Glasshouse

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).
Supersedes decisions 4 and 6, and closes P2 and D7.

**Context.** Decision 4 made Glasshouse the system layer, and decision 6
pinned it. Since then Guard has taken the blocking (record 11), route D
the Home key, and Lumen Remote the control. Glasshouse would add a root
Node.js service with a web dashboard whose phone-setup link listens on the
LAN even when the dashboard is bound to localhost.

**Decision.** Glasshouse is never installed. The stack is LG's platform
plus Lumen, with Homebrew Channel for root. OLED care is LG's own settings,
shown in the Control Center; remote keys come from Lumen Remote.
Glasshouse's research (LG's hosts, the ACR flags, the UEI stand-in, key
filters on webOS 4) stays a cited source.

**Cost.** Lumen builds what it would have taken: the view of LG's settings
in the Control Center and any OLED-care scheduling. `profiles/glasshouse`
and `runbooks/phase1-tv.md` become history.

## 14. Rust on the TV, and one root daemon

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).

**Context.** The shell, Guard and `lumen-ctl` are Rust. The boot hooks
(privacy S1, Guard, `lumen-home`) and the icon copy are POSIX `sh`, and
`lumen-home` polls the app manager with sleeps. The owner wants more of
Lumen in Rust wherever that makes the TV faster or easier to control, and
accepts more work and review for it. Research of 2026-11-05: the bus hub
judges a client by its executable and its role files, not its uid, so root
gets no luna access by being root; a hand-written luna client would be 800
to 1,500 lines.

**Decision.**

- **Everything Lumen runs on the TV is Rust,** except the key filter (ES5,
  since it runs inside LG's compositor) and each boot hook's door: a few
  lines of POSIX `sh` that check the kill switch, time-box the start and
  run a binary.
- **`lumend`, one root daemon, takes the hooks' logic:** it holds LG's
  senders down, closes LG Home after boot, copies the icons, watches
  Lumen's file modes, starts the shell and restarts it (record 15), and
  turns the screen off after the screensaver (record 16). It reacts to the
  app manager's events instead of polling.
- **Guard stays its own process,** so DNS fails open by itself whatever
  happens to `lumend`. `lumen-ctl` stays the SSH forced command.
- **Only Guard reaches the internet.** `lumend` and the shell open no
  network socket: a lint on Rust's network types in their crates refuses
  it, and a port survey on the TV checks it. `lumen-remote` (remote.md, R4)
  stays the one LAN listener: unprivileged, paired, off by default.
- **Dependencies.** Lumen hand-writes a protocol only when it is small,
  and takes a crate only when it replaces more code than Lumen would
  write, listed with its version and reason, with a license check in
  `make check`.
- **LG's luna library stays.** A hand-written client gains no permission,
  so the shell and `lumend` keep loading `libluna-service2` at run time.
  `lumend` gets a luna role of its own, outbound only with no public
  methods, written under `/var` by its install and removed by the undo.
- **The shell and `lumend` talk over a socket of their own,** each
  checking the other's uid through the kernel (`SO_PEERCRED`): the shell's
  jail uid on one side, root on the other. A small fixed message set,
  Lumen's own. The socket lives in the shell's own jail, created from the
  host side, never in `/tmp`, which every jail shares and where root
  writes nothing (CONTRIBUTING.md). If the jail can't reach it there, the
  fallback comes back to the owner with the rule it would change.

**Cost.** `lumend` is a new root process and needs the same security
review as `lumen-ctl`. The `sh` doors stay readable on the TV, but the
logic behind them doesn't, so `lumend`'s log has to say what it did.
Rewriting working scripts costs time before it pays back.

## 15. The shell owns its window and stays resident

**Status.** Accepted by the owner on 2026-11-05 (record 10's session). The
SDL path stays until the Rust layer wins on the B9.

**Context.** On 2026-10-31 a Home press with the shell not running took
1,173 ms to its first frame (SDL 264 ms, GL 358, renderer 175, assets 258,
apps 75, the frame 43). Bringing back a shell that was running took 2 to
170 ms. The target is 150 ms. Research of 2026-11-05:

* With Lumen in front, a Home press becomes an app-manager relaunch,
  delivered to whoever holds Lumen's lifecycle registration. Today that is
  LG's SDL inside Lumen, which only raises the window.
* Only a registered app (lifecycle version 2) survives a keep-alive pause;
  any other is killed.
* LG's EGL accepts damage regions without advertising them. In plx-native's
  test on webOS 4.10.2, a 6.25% region cut GPU work by 92.8%.
* ES 3 includes the ETC2 and ASTC texture formats.
* No UI toolkit has been measured on this GPU class.

**Decision.**

- **Resident.** The shell starts at boot, hidden behind the last input,
  and stays alive behind apps. While hidden it frees the Top Shelf art and
  keeps the icon atlas.
- **Its own platform layer, in Rust.** The shell talks to the compositor
  itself:
  - the TV's `libwayland-client` and EGL, loaded at run time;
  - LG's protocol differences mapped at run time, as SDL-webOS does;
  - its own version-2 lifecycle registration, through libluna.

  It is built behind a switch beside the SDL path. SDL leaves only when
  the Rust layer matches or beats it on the B9 (start-up, key to frame,
  memory) and a Home press with Lumen in front reaches the shell.
- **Staying alive takes four layers,** each tried by hand first:
  - the registration;
  - Lumen in the app manager's keep-alive list (one persistent configd
    value, read after the app manager restarts; the undo removes it);
  - `lumend` starting the shell hidden after boot's first app, and again
    if the memory manager closes it;
  - a small footprint while hidden.
- **The GPU plan,** gated by one measurement on the B9 (upload times, and
  the gradients with the dither on):
  - an ES 3.0 context;
  - heroes as ASTC 4x4, from ARM's `astcenc` at build time, about 1.2 MB
    each instead of 4.9 (ETC2 if ASTC bands);
  - compiled shaders and the glyph atlas cached on disk;
  - partial redraw, its damage found by comparing a frame's draw list with
    the last one, and the whole screen redrawn after any return to the
    front.

  The simulator keeps PNGs.
- **No toolkit:** the hand-written renderer stays.
- **One overlay trial.** The platform layer tries one of the compositor's
  overlay window types. If a developer-mode app is refused, the gear
  inside apps stays LG's quick settings for good.

**Cost.** 1,500 to 2,500 lines of Rust against LG's undocumented variant
of the protocol, and two window paths until SDL leaves. The keep-alive
value is the first persistent change to LG's app manager. The resident
shell holds 20 to 40 MB on a set with about 335 MB free. ASTC may band on
the gradients. The simulator and the TV draw the heroes from different
files.

Supersedes decision 1's "SDL2 and GLES2" (Rust and OpenGL ES stay).

## 16. The screensaver lives in the shell

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).

**Context.** The plan in `ambient/README.md` mounted a QML file over LG's
screensaver. Research of 2026-11-05:

* The TV's power service asks its subscribers before it starts the
  screensaver, and an answer of `ack:false` holds it off (Kodi does this).
* plx-native plays HDR10 and Dolby Vision profiles 5 and 8 on the video
  plane, under a Rust UI, on webOS 4.10.2.
* Nobody has tested a seamless loop.

**Decision.**

- **With Lumen in front,** Lumen answers the screensaver request with
  `ack:false` and plays its own HDR loops on the video plane, moving and
  dimming for the panel's sake. It uses libplayerAPIs and ACB, loaded at
  run time, which is plx-native's route.
- **Screen off.** After 2 hours of screensaver with no key pressed, LG's
  Screen Off (94 ms through `lumen-ctl`). Any key wakes it. LG's auto
  power-off is left alone.
- **With another app in front,** LG's screensaver runs as today. Taking
  over from other apps, with a key going back to the app, is a later
  trial. If Lumen dies, LG's screensaver comes back by itself.
- **A seamless-loop spike comes first.** The content is the studio's loops
  and the owner's own videos, from the PC or the LAN.
- **The QML mount is dropped.**

**Cost.** The screensaver depends on the shell running. Lumen owns OLED
care for its own screensaver. Playing video is new code (pipelines, HDR
metadata) in the shell's process.

## 17. Video and art are CC BY-SA 4.0

**Status.** Accepted by the owner on 2026-11-05 (record 10's session).
Supersedes decision 2's last sentence.

**Context.** Decision 2 says "Original video content is CC BY-SA 4.0".
Lane E's heroes and pictograms are drawn by code from the same scenes, and
that sentence doesn't cover them. `shell/assets/art/CREDITS.md` had a
placeholder for the author.

**Decision.** Original video and art are CC BY-SA 4.0, and the code that
makes them stays GPL-3.0-or-later. The author is credited as SunnySide007,
the owner's GitHub handle. No real name appears in the public repository.

**Cost.** None beyond keeping the credits current.
