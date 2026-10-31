# Depth: how low Lumen can go on the B9, and what to build next (proposed)

Written on 2026-10-30 for the owner's review, after a read of every branch,
the uncommitted work and the upstream projects. It answers four questions:
where the work stands, how deep the B9 lets Lumen reach, what already exists
so nothing is built twice, and what to build next, in which order and by
whom. Nothing here has run on the TV; every TV step waits for the owner
([safety.md](../safety.md)).

## Where it stands

| Where | What | State |
|---|---|---|
| `main` (ce6b25d, 29 commits ahead of `origin/main`, unpushed) | Phase 0 and 1 tooling, the TV check skill, the look-and-feel spec | Clean apart from the privacy work below |
| `main`, uncommitted | Privacy: [privacy.md](privacy.md), the read-only survey (`tools/recon.sh --privacy`, two runs on 2026-10-28), stage S1 by hand and as a boot hook (`profiles/privacy/lumen-privacy`, installed 2026-10-28 07:00 UTC, verified after a reboot), `tools/backup.sh`, `tools/privacy-hook.sh`, their bats tests | Owed: the owner's short Netflix and casting check after that reboot, then the commit |
| `claude/shell-timing` (c35e7e4, 20 commits over `main`) | The real shell: `lumen-core`, B2 icons and Inter text, B3 luna launcher, the timing and key-diagnostics lines, hardware float, the style module, kerning, LG's icons through a root copy, stage 3's vsync pacer and glyph warming. 3a89955 is the build on the TV today | Owed: the `--no-ff` merge into `main` and its pull requests; the slow-motion video (targets 3 and 5) |
| `refactor/loop-pass` (61cf2fc, 5 commits over `claude/shell-timing`) | The home loop behind events, screen and clock adapters, tested through fakes (decision 8, proposed) | Owed: one `lumen-tv-check` against 3a89955's numbers, then the merge |
| `claude/studio-9e791a` (c407f65, 12 commits) | The HDR studio: aurora and fireflies loops, the HDR switch check (passed on the B9 2026-10-20) | Owed: the 60 s loops on the TV; Dolby Vision and more scenes when the owner says so |
| `claude/tv-remote-plan-cb573f` (worktree `studio-9e791a`), uncommitted | [remote.md](remote.md) (R0 to R6), decision 9, the "Remote" research | Awaiting the owner's approval before anything is built |
| The TV | Root, SSH by key, plx-native 0.8.0, Lumen 3a89955 with LG's icons copied, the S1 boot hook, IPv6 off, Glasshouse not installed | As left on 2026-10-28 |

Phase 1 as the roadmap words it (Glasshouse, a 24 h capture) is being
replaced by privacy.md's stages: S1 is done, G1 (Lumen Guard, log only) is
next, and Glasshouse is decided after a week of G1's log (P2).

## How low the B9 lets us go

From the pixels down to the kernel, what Lumen reaches today, what it can
reach, and what stays out. "Measured" means the owner's OLED55B9PUA,
webOS 4.10.2-31, firmware 05.50.00 ([b9.md](../device/b9.md)).

| Layer | Today | Possible | Out of reach |
|---|---|---|---|
| **Pixels** | One batched GLES draw a frame through LG's SDL 2.0.5, 60 fps moving, 0.2% of a core settled, hardware float (start 524 to 688 ms) | EGL and Wayland directly, skipping SDL: saves the SDL window set-up (139 to 199 ms of start-up) and the key-layout guesswork. Worth it only if SDL blocks something; it hasn't yet | HDR from the UI plane: it is 8-bit SDR. Glass over video: video is on a plane GL can't read |
| **Video** | plx-native plays through `libplayerAPIs` (`StarfishMediaAPIs::Feed`, resolved with `dlsym`), the same decoder LG's apps use, Dolby Vision profiles 5 and 8 direct | The same library would let the shell play its own loops or a Top Shelf trailer on the hardware plane. Not planned: the screensaver already gets HDR through QML `Video` | Drawing over that plane |
| **Input** | SDL key events at LG's offsets; clicks as OK; the pointer hidden with `SDL_webOSCursorVisibility`; evdev read-only recordings | `uinput` and `uhid` exist (root, 0600): a root helper can present a USB pad the kernel's `xpad` can't finish (Play P1 route 3) | HOME inside an app: the compositor takes it first (below) |
| **Apps and launching** | luna from the app's jail through `libluna-service2` (list, launch, foreground watch) | `com.webos.service.config`, `settingsservice`, `tvpower`, `audio`: the Control Center and the remote's verbs. `ls-monitor -i` lists any service's methods, read-only | Nothing yet |
| **Home** | Lumen is an ordinary app; Home opens LG's native `com.webos.app.home` (`/usr/bin/com.webos.app.home`, `appinfo.json` type `native`, recon 2026-10-20) through the compositor's `appLaunch.js` key filter (`handleTvhotkeyAppLaunch`) | Three routes, below. The deepest makes Lumen *be* `com.webos.app.home` | Changing what `bootd` expects: it has that app id built in |
| **Senders** | Four LG reporting jobs held down from boot (S1), one LAN port closed | Any upstart job, one per check (G4); on-demand luna services left alone | Nothing: `initctl` reaches every job |
| **Names** | Homebrew Channel's hosts bind mount (firmware hosts) | Lumen Guard: every lookup through a nat `DNAT` to a loopback forwarder, with suffix rules and a log that survives power-offs (G1 to G3) | Nothing: netfilter's nat table is built in |
| **Addresses and the LAN** | One `INPUT` rule | `OUTPUT` rules for hard-coded endpoints; `INPUT` rules for ports nobody uses (3000/3001, 7000, 35254) | Connection state, owner and port-range matches: the kernel has none, so rules are stateless, one address or port each. No IPv6 netfilter, so IPv6 stays off |
| **Boot** | One Homebrew Channel `init.d` hook, about 37 s into the boot | More hooks, each with the kill switch and a time box | Anything before Homebrew Channel starts: the boot logo, the first 30 s |
| **Firmware** | Updates blocked (`snu`, `su`, `su-ssl`, `su-dev` at 127.0.0.1; `nsu.lge.com` missing) | `nsu.lge.com` through Guard; the 156.147.69.32:8080 fallback through an `OUTPUT` rule, once seen | Flashing anything (decision 3). Security fixes: the set never gets LG's, so the LAN surface is kept small instead |

### Home: three routes

The roadmap's `takeover/` planned a key filter plus a foreground watcher.
The community has since settled on something lower, and it works on webOS 4
according to Hearth and OpenHome's users:

| Route | How | Home press to Lumen | Costs and risks | Undo |
|---|---|---|---|---|
| **A. Lumen as Home** | A RAM-only bind mount over `/usr/palm/applications/com.webos.app.home/appinfo.json` (or the folder) whose `main` is Lumen's binary, then the app manager restarted once at boot. The compositor's own Home handler then launches Lumen; `bootd` opens it at power-on; Back from an app returns to it | The compositor's launch of Home, the same path LG's bar uses. Lumen's own half is 11 ms (p50) after a focus gain | The boot animation stops playing (theboringtv). Hearth notes the TV's aspect-ratio setting goes dark while the stand-in is mounted; cause unknown. Whether SAM runs a native Home jailed, as root, or neither is unread. LG's bar is gone until undone, so the escape hatch becomes holding Home (a key filter of a few lines) and Homebrew's failsafe | Remove the hook, reboot: the mount lives in RAM |
| **B. Stand-in and watcher** | A tiny black stand-in app bind-mounted over LG Home; a root watcher opens Lumen whenever the stand-in comes forward (Hearth's "script and SAM", OpenHome's `homestub.sh`) | A flash of black, then Lumen: tens of ms more than A | The watcher is one more process; the same aspect-ratio note | The same |
| **C. Key filter** (the plan until now) | An ES5 handler in `com.webos.surfacemanager.keyFilters`, before `appLaunch.js`, that launches Lumen on a short Home press and lets a long press through | A luna launch of a backgrounded app: measured 170 ms from "restored" to first frame once, 2 to 13 ms when still in front | LG's Home still runs and still calls home (its launcher and beacon names). Glasshouse documents the mechanism on webOS 4; nothing else has shipped it | Delete the configd entry, reboot |

**Recommendation:** a read-only look first (below), then route A if SAM
will run Lumen's binary as Home, else B. Either one also answers privacy
layer 2 (LG Home out of the way) and removes the need for the icon root
copy, since Home can read `/usr/palm/applications` itself. C stays as the
fallback. The look reads, and changes nothing:

1. `/usr/palm/applications/com.webos.app.home/appinfo.json` whole, and
   `com.webos.app.homeconnect`'s.
2. `/usr/lib/qt5/qml/KeyFilters/appLaunch.js` and `systemUi.js`: what a
   Home press, a long Home press and Back do.
3. How SAM launches a `native` app: its upstart job (`initctl list | grep
   sam`), `/etc/jail_*.conf` and `/etc/palm/jail*`, and `ps -o user,pid,cmd`
   for the running Home.
4. `bootd`'s configuration (`/etc/bootd*`, `/var/luna/preferences/bootd*`),
   and the settings `homeAutoLaunch` and the last input, which theboringtv
   changes.
5. Whether `restart sam` (or LG's name for it) is a plain upstart job, and
   what else is stopped with it.

Done when b9.md has a "Home" section with those five answers and a chosen
route.

## What already exists (so nothing is built twice)

Checked on 2026-10-30. Where a project replaces one of Lumen's parts, the
verdict says so.

| Project | What it is now | For Lumen |
|---|---|---|
| [Glasshouse](https://github.com/rorygallagher2024/glasshouse) 0.135.0 (five releases on 28 and 29 October) | Privacy, blocker, services, remote keys, OLED care, Home Assistant, screen off; Glasshouse Home now experimental on webOS 5 and 6, still not 4 | The pin at 0.118.0 is 17 releases old. Its blocker is the hosts file, which Guard outgrows (privacy.md's table). Decide after G1's week of log (P2); if it comes in, for OLED care and remote keys, not for blocking or Home |
| [plx-native](https://github.com/GLinnik21/plx-native) 0.10.0 nightlies (daily, latest 2026-10-30) | Native Plex: `libplayerAPIs`, DV 5 and 8, Atmos, in-app FLAC and Opus, hi-res resampling, HUD plugins; Magic Remote scrolling changed | Keep 0.8.0 until a nightly is pinned and checked (the HDR and remote changes are worth a look). Servers come from the plex.tv account: no sign-in-free local mode exists, so `plex.tv` and `plex.direct` stay allowlisted and playback itself stays on the LAN. No deep-link parameters documented: the roadmap's upstream PR still applies |
| [YouTube AdFree](https://github.com/webosbrew/youtube-webos) (Homebrew Channel) | The stock YouTube web app with ads blocked, SponsorBlock, no Shorts, highest quality; installs through Homebrew Channel; wants the official app gone first | The one stock app whose ads Lumen can't otherwise touch. Try it: one IPK, reversible. webOS 4.5 isn't confirmed by its page; C9 owners report it working. Its app id equals the stock one, so the look checks which copy SAM picks |
| [Hearth](https://github.com/rudra-avatar/Hearth) (MIT), [OpenHome](https://github.com/devic021/openhome-webos) (GPL-3.0), [theboringtv](https://github.com/theboringhumane/theboringtv-lgwebos), [Mi pantalla](https://github.com/frxnnk/webos-liquid-glass-launcher) | Web-app home replacements for rooted sets. Hearth: webOS 4.x reported, three takeover methods. OpenHome: "webOS 4 and up", default-home setting plus `homestub.sh`, 100 hidden LG settings in 13 tabs. Mi pantalla: "liquid glass" in CSS on webOS 5, 59.7 fps claimed on a 2020 LCD | None is native or GLES; none runs on Chromium 53 with glass. Their takeover scripts are the prior art for routes A and B, and OpenHome's settings tabs are a map of `settingsservice` keys for the Control Center. Lumen's niche, a native 60 fps home on a 2019 OLED, stands |
| [Homebrew repo](https://repo.webosbrew.org/): Jellyfin, Kodi, Moonlight, RetroArch, WireGuard VPN, HyperHDR and PicCap, two Aerial screensavers, Immich, VNCServer, Open Service Menu, LG App Update Blocker, Bluetooth Disabler | 48 apps, no launcher | Jellyfin is the account-free local player if Plex ever grates. WireGuard on the TV is an option for Remote R6, after the bridge box. PicCap and HyperHDR mean bias lighting is one install away. Nothing else overlaps |
| [Level1Techs](https://forum.level1techs.com/t/lg-tv-block-mini-how-to/255178), [hagezi](https://github.com/hagezi/dns-blocklists/issues/5622), [Krebs 2026-07-21](https://krebsonsecurity.com/tag/webos/) | Router-side blocking that survives a reset; block outbound 53 from the TV's subnet (some firmware hard-codes 8.8.8.8); `snu.lge.com` is the update check; LG store apps found acting as residential proxies | Guard's `DNAT` covers hard-coded resolvers on the TV itself. The router rule is the owner's call on the Nest Wifi (it can't log, but can it block a port? unverified). The proxy story is one more reason for `OUTPUT` rules and a small LAN surface |

## Stock apps, kept private

What decision 5 (aggressive, store-safe) means app by app, with what each
layer costs:

| App | Keeps working through | Costs | Private because |
|---|---|---|---|
| Netflix, Prime Video | The Content Store (`lgtvsdp.com`), DRM (`googleapis.com`), their own hosts, AWS never blocked | Netflix's logging names (`nrdp.logs`, `ichnaea`) stay allowed under the suffix rule (P5: one trial, if wanted) | LG's beacon and launcher calls stop with Home out of the way (route A or B); senders are down (S1) |
| YouTube | YouTube AdFree instead of the stock app | Developer-mode copy; updates come from Homebrew Channel, not LG | Ads and Shorts gone; Google's own telemetry is YouTube's, not LG's |
| Plex | plx-native on the LAN; `plex.tv` for sign-in and the server list | Account required | Streams never leave the house; the server is the owner's |
| LG Channels, voice search, ThinQ | Nothing: they are the cost of the tier (decision 5) | | |
| HDMI inputs | Lumen's tiles | None | ACR stays off (LG's switch, measured off); Guard's log covers the HDMI hours the 4 h capture missed |

## What to build next

Ordered by what unblocks what. Each item names who runs it: **TV** means the
owner at the set with this session, **lane** means a hand-off to Codex or
Gemini ([tools/out/handoff-20261030/](../../tools/out/handoff-20261030/README.md),
git-ignored), **here** means this session off the TV.

| # | Item | Who | Needs | Done when |
|---|---|---|---|---|
| 1 | Commit the privacy work on `main` (Gate 2) after the owner's Netflix and casting check | TV, then here | The owner's word | `make check` green; one commit series on `main` |
| 2 | **Lumen Guard G1**: the Rust forwarder, log only, with its hook and tooling, off the TV | lane A (Codex, GPT-6 Astra) | 1 | Unit and bats tests; a PC run forwards and logs real lookups; `tools/dev.sh make check` green |
| 3 | `tools/frame-stats.sh` summarises the stage 3 lines (`vsyncs skipped`, `motion paced`, `warm`, `dropped`) | lane B (Codex, GPT-6.1 Sol) | Nothing | bats cases on real fixtures from `tools/out/archive/tv-check-20261023T024724Z` |
| 4 | **Remote R0** as a read-only survey script, like `--privacy` | lane C (Codex, GPT-6.1 Sol) | Nothing | `tools/recon.sh --remote` with read-only guards in `recon.bats`; the run itself is a TV step |
| 5 | The Home look (five reads above), YouTube AdFree, the owed post-boot check, the loop-pass TV check | TV | The owner | b9.md "Home" section; the route chosen; YouTube plays without ads or the IPK is removed; decision 8 accepted or not |
| 6 | Guard G1 on the TV by hand, then as a hook; a week of log | TV | 2, 5 | privacy.md G1's "done when" |
| 7 | `--no-ff` merge of `claude/shell-timing` (and `refactor/loop-pass` once checked) into `main`; the pull requests; the push | here | 5, the owner's call | CI green; `v0.3.0` waits for the takeover |
| 8 | **`takeover/`** on the chosen route: hook, stand-in or appinfo, hold-Home escape, watchdog, `uninstall-all.sh` coverage | lane (Codex, Astra) after 5, then TV | 5, 7 | Home opens Lumen in under 150 ms on the B9; every escape hatch works; a reboot without the hook restores LG |
| 9 | **Lumen Remote front-end** against a mock `lumen-ctl` (R3's web UI, the shared front-end) | lane D (Gemini 4 Argon) | The owner's approval of remote.md | A bundled TypeScript UI with the verbs of remote.md, no CDN, a demo recording |
| 10 | **Original Top Shelf and tile art** for Lumen's own tiles and the inputs | lane E (Gemini, image generation) | Nothing | PNGs that fit the look-and-feel tokens, CC BY-SA 4.0, no brand marks |
| 11 | Phase 4 on the merged shell: focus light and tilt, the Top Shelf hero with 10's art, frosted glass B, the Control Center (with OpenHome's key map as a reference), editing; the dither | here and lanes, after 7 | 7, 10 | look-and-feel.md's three checks |
| 12 | Guard G2 and G3, senders G4, the Privacy panel G5 | here, TV | 6 | privacy.md |

Lanes A, B and C can start the moment 1 is committed; D waits for remote.md;
E has no dependency. Three Codex lanes and one Gemini lane at a time is the
owner's limit, and the README says how each comes back.

### What this changes in the roadmap

* Phase 1's rows become privacy.md's stages; the 24 h capture and the
  Glasshouse install are no longer prerequisites.
* Phase 3's takeover is decided by the Home look, not fixed to the key
  filter.
* Phase 4 gains the art lane and OpenHome's settings map as inputs.

## Decisions for the owner

| | Question | Recommendation |
|---|---|---|
| D1 | Commit the privacy work on `main` now (your post-boot check first, or trust the 2026-10-28 log and check later) | Commit now: lanes branch from it |
| D2 | Home: run the five-read look next time at the TV | Yes; then route A if SAM runs a native Home from a developer path, else B |
| D3 | YouTube AdFree through Homebrew Channel | Yes, after the look says which copy SAM picks; one IPK, reversible |
| D4 | Lanes: A Guard (Astra high), B frame-stats (Sol high), C remote survey (Sol high), D remote front-end (Argon high, after remote.md), E art (Gemini image generation) | Start A, B and C after D1; E any time; D when remote.md is approved |
| D5 | remote.md: approve as drafted on 2026-10-28, with your D1 to D13 answers | Approve; it unlocks lane D and item 4's run |
| D6 | plx-native: pin a 0.10.0 nightly for a look at the HDR and remote changes, or stay on 0.8.0 | Stay until a tagged 0.9 or 0.10; nightlies change daily |
| D7 | Glasshouse: still install at all | Decide after G1's week, as P2 says; if yes, for OLED care and remote keys only |
| D8 | Art: the generated images' rights follow the tool's terms; Lumen ships them CC BY-SA 4.0 | Confirm you accept that before lane E runs |
