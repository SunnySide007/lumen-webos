# Depth: how low Lumen can go on the B9, and what to build next (proposed)

Written on 2026-10-30 for the owner's review, after a read of every branch,
the uncommitted work and the upstream projects. It answers four questions:
where the work stands, how deep the B9 lets Lumen reach, what already exists
so nothing is built twice, and what to build next, in which order and by
whom. Nothing here has run on the TV; every TV step waits for the owner
([safety.md](../safety.md)).

## Hand-off of 2026-10-31

Since 2026-10-31 every session on the project, the project manager and each
lane, is a Claude Opus 5.5 session at extra-high effort: the owner's choice,
quality first and as low in the B9's stack as it allows. The Codex and Gemini
lanes of the first set are gone; A, B, C and D ran as Claude sessions and are
merged. The briefs for what comes next (lanes F to K, below) and the project
manager's opening prompt are in
[tools/out/handoff-20261031/](../../tools/out/handoff-20261031/README.md)
(git-ignored). In flight at the hand-off: lane A's second round, the Guard
boot hook rebuilt around a `resolv.conf` bind mount (item 6); it landed,
was merged, and passed by hand on the TV the same afternoon. Lanes F
(item 8) and G (item 13) started on 2026-10-31 at about 15:20 UTC and
were reviewed and merged the same day, F in three rounds (the last moved
every boot hook's log out of `/tmp`, which the app jails share and where
`fs.protected_symlinks` is 0 on the B9). S1's and Guard's hooks were
reinstalled with those logs at 17:05 UTC and checked after a reboot.

## Where it stands

| Where | What | State |
|---|---|---|
| `main` (pushed; 9632884 at the hand-off) | Phase 0 and 1 tooling, the TV check skill, the look-and-feel spec; privacy: [privacy.md](privacy.md), the surveys, stage S1 as a boot hook (installed 2026-10-28), `tools/backup.sh`; lanes A to D (Guard G1, frame-stats, the remote survey, the remote front-end) | The owner's Netflix, YouTube and casting check passed after the reboot of 2026-10-31 16:10 UTC, with S1 and Guard running |
| `claude/shell-timing` (c35e7e4, 20 commits over `main`) | The real shell: `lumen-core`, B2 icons and Inter text, B3 luna launcher, the timing and key-diagnostics lines, hardware float, the style module, kerning, LG's icons through a root copy, stage 3's vsync pacer and glyph warming. 3a89955 is the build on the TV today | Owed: the `--no-ff` merge into `main` and its pull requests; the slow-motion video (targets 3 and 5) |
| `refactor/loop-pass` (61cf2fc, 5 commits over `claude/shell-timing`) | The home loop behind events, screen and clock adapters, tested through fakes (decision 8, proposed) | Owed: one `lumen-tv-check` against 3a89955's numbers, then the merge |
| `claude/studio-9e791a` (c407f65, 12 commits) | The HDR studio: aurora and fireflies loops, the HDR switch check (passed on the B9 2026-10-20) | Owed: the 60 s loops on the TV; Dolby Vision and more scenes when the owner says so |
| `claude/tv-remote-plan-cb573f` (30d67ba), merged into `main` on 2026-10-31 | [remote.md](remote.md) (R0 to R6), decision 9, the "Remote" research | Approved by the owner on 2026-10-31 as drafted; lane D and R0's run are open |
| The TV | Root, SSH by key, plx-native 0.8.0, Lumen 3a89955 with LG's icons copied and set as Home (route D, since 2026-10-31 05:50 UTC), the S1 boot hook, IPv6 off, Lumen Guard G1 as a boot hook (log only, since 2026-10-31 16:10 UTC), both hooks logging to `/var/lib/lumen/log` (since 17:10 UTC), Glasshouse not installed | As left on 2026-10-31 17:12 UTC |

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
| **Names** | Homebrew Channel's hosts bind mount (firmware hosts); Lumen Guard G1, log only, a boot hook since 2026-10-31 | Lumen Guard: lookups through a `resolv.conf` bind mount, and a nat `DNAT` for programs with their own resolver (Netflix asks 8.8.8.8), with suffix rules and a log that survives power-offs (G1 to G3) | connman's own forwards: its sockets are bound to `wlan0`, so no rule redirects them; they go to the router unlogged |
| **Addresses and the LAN** | One `INPUT` rule | `OUTPUT` rules for hard-coded endpoints; `INPUT` rules for ports nobody uses (3000/3001, 7000, 35254) | Connection state, owner and port-range matches: the kernel has none, so rules are stateless, one address or port each. No IPv6 netfilter, so IPv6 stays off |
| **Boot** | One Homebrew Channel `init.d` hook, about 37 s into the boot | More hooks, each with the kill switch and a time box | Anything before Homebrew Channel starts: the boot logo, the first 30 s |
| **Firmware** | Updates blocked (`snu`, `su`, `su-ssl`, `su-dev` at 127.0.0.1; `nsu.lge.com` missing) | `nsu.lge.com` through Guard; the 156.147.69.32:8080 fallback through an `OUTPUT` rule, once seen | Flashing anything (decision 3). Security fixes: the set never gets LG's, so the LAN surface is kept small instead |

### Home: three routes

The roadmap's `takeover/` planned a key filter plus a foreground watcher.
The community has since settled on something lower, and it works on webOS 4
according to Hearth and OpenHome's users:

| Route | How | Home press to Lumen | Costs and risks | Undo |
|---|---|---|---|---|
| **D. LG's own switch** (found in the look of 2026-10-31) | `defaultApps.home` in `settingsservice` (category `general`) set to Lumen's id, the way LG's `home_launcher.sh` selects its full-screen home; no file, mount or restart | The compositor's own `launch(..., {"launchType":"homeKey"})`, the same call LG's bar gets | Proven on 2026-10-31: the service took Lumen's id and a short Home press launched Lumen with `launchType: homeKey` (b9.md, "Home"). A long Home press does nothing until Lumen handles it; LG Home stays installed and is opened from Lumen or by setting the key back. LG Home may still be preloaded on snapshot boots (close it as LG's script does) | One `setSystemSettings` back to `com.webos.app.home` |
| **A. Lumen as Home** | A RAM-only bind mount over `/usr/palm/applications/com.webos.app.home/appinfo.json` (or the folder) whose `main` is Lumen's binary, then the app manager restarted once at boot. The compositor's own Home handler then launches Lumen; `bootd` opens it at power-on; Back from an app returns to it | The compositor's launch of Home, the same path LG's bar uses. Lumen's own half is 11 ms (p50) after a focus gain | The boot animation stops playing (theboringtv). Hearth notes the TV's aspect-ratio setting goes dark while the stand-in is mounted; cause unknown. Whether SAM runs a native Home jailed, as root, or neither is unread. LG's bar is gone until undone, so the escape hatch becomes holding Home (a key filter of a few lines) and Homebrew's failsafe | Remove the hook, reboot: the mount lives in RAM |
| **B. Stand-in and watcher** | A tiny black stand-in app bind-mounted over LG Home; a root watcher opens Lumen whenever the stand-in comes forward (Hearth's "script and SAM", OpenHome's `homestub.sh`) | A flash of black, then Lumen: tens of ms more than A | The watcher is one more process; the same aspect-ratio note | The same |
| **C. Key filter** (the plan until now) | An ES5 handler in `com.webos.surfacemanager.keyFilters`, before `appLaunch.js`, that launches Lumen on a short Home press and lets a long press through | A luna launch of a backgrounded app: measured 170 ms from "restored" to first frame once, 2 to 13 ms when still in front | LG's Home still runs and still calls home (its launcher and beacon names). Glasshouse documents the mechanism on webOS 4; nothing else has shipped it | Delete the configd entry, reboot |

**Recommendation, after the look of 2026-10-31
([b9.md, "Home"](../device/b9.md#home)):** route D, proven by hand on
2026-10-31 (a backup, the one setting, Home opened Lumen). Lumen stays a jailed developer-mode app on
every route, so the icon root copy stays. C is the fallback. The look read,
and changed nothing:

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

Done on 2026-10-31: b9.md's "Home" section has the five answers; the route
is D, proven the same day.

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
owner at the set with the project manager's session, **lane** means a Claude
Opus 5.5 session started from it with a brief
([tools/out/handoff-20261031/](../../tools/out/handoff-20261031/README.md),
git-ignored), **here** means the project manager off the TV.

| # | Item | Who | Needs | Done when |
|---|---|---|---|---|
| 1 | Commit the privacy work on `main` (Gate 2) after the owner's Netflix and casting check | TV, then here | The owner's word | `make check` green; one commit series on `main` |
| 2 | **Lumen Guard G1**: the Rust forwarder, log only, with its hook and tooling, off the TV | lane A (done 2026-10-31, merged) | 1 | Built and merged: unit, integration, bats and Python tests, make check green. Next: the by-hand run on the TV (`tools/guard.sh push`, `guard-apply.sh`), G1 |
| 3 | `tools/frame-stats.sh` summarises the stage 3 lines (`vsyncs skipped`, `motion paced`, `warm`, `dropped`) | lane B (done 2026-10-31, merged 208ce07) | Nothing | Done: bats cases on real fixtures from `tools/out/archive/tv-check-20261023T024724Z` |
| 4 | **Remote R0** as a read-only survey script, like `--privacy` | lane C (done 2026-10-31, merged ff81bfe) | Nothing | Done: `tools/recon.sh --remote` ran on the B9 on 2026-10-31; answers in b9.md, "Remote" |
| 5 | The Home look (done 2026-10-31: route D), route D proven by hand, YouTube AdFree, the owed post-boot check (done 2026-10-31: Netflix, YouTube and casting work with S1 and Guard), the loop-pass TV check | TV | The owner | b9.md "Home" section; the route chosen; YouTube plays without ads or the IPK is removed; decision 8 accepted or not |
| 6 | Guard G1 on the TV by hand, then as a hook; a week of log | TV | 2, 5 | privacy.md G1's "done when". First by-hand run 2026-10-31: the redirect design failed open as built (connman's sockets are bound to wlan0; b9.md, "Resolver"); lane A rebuilt the hook around a `resolv.conf` bind mount, and **the rebuilt hook passed by hand the same afternoon**: 940 kB resident, Netflix's own resolver (8.8.8.8) caught by the DNAT rules, kill switch, Wi-Fi drop and hung-Guard tests all passed. P11 decided (Guard's file names connman second) and tried by hand; **installed as a boot hook at 16:10 UTC and checked after a reboot**; reinstalled with its log off `/tmp` at 17:07 UTC and checked after another reboot. The week of log runs to 2026-11-07, which opens lane H |
| 7 | `--no-ff` merge of `claude/shell-timing` (and `refactor/loop-pass` once checked) and `claude/studio-9e791a` into `main`; the pull requests; the push | here | 5, the owner's call | CI green; `v0.3.0` waits for the takeover. A trial merge of `claude/shell-timing` on 2026-10-31 conflicts only in `Makefile`, `THIRD-PARTY.md`, `shell/Cargo.toml` and `shell/Cargo.lock` |
| 8 | **`takeover/`** on route D: `tools/takeover.sh` with its undo, the boot hook that closes LG Home after boot as LG's own script does, `uninstall-all.sh` restoring `defaultApps.home` before it removes the app, an LG Home tile, and the long-Home key filter designed and fake-tested but delivered inert | lane F, then TV | 5 (done); 7 for the tile only | Home opens Lumen in under 150 ms on the B9; every escape hatch works; `undo` restores LG. **Built and merged 2026-10-31** (c7c0034; `takeover/README.md`): the setting with its undo, the `lumen-home` hook, the inert filter with its trial plan, uninstall's order; files reach the TV inside a generated script, checked in root's stage. Not yet run on the TV; the tile waits for 7 |
| 9 | **Lumen Remote front-end** against a mock `lumen-ctl` (R3's web UI, the shared front-end) | lane D (done 2026-10-31, merged) | Nothing: remote.md approved 2026-10-31 | Built and merged: `remote/ui` and `remote/mock-ctl`, 73 KB gzipped, tests green in the lane; not in make check yet (D9) |
| 10 | **Original Top Shelf and tile art** for Lumen's own tiles and the inputs, made by code from the studio's scenes and the tile palette; no image model | lane E | 7 (the studio merge) | PNGs that fit the look-and-feel tokens and pass their own luminance and banding tests, CC BY-SA 4.0, no brand marks |
| 11 | Phase 4 on the merged shell: the Control Center over `settingsservice` with the Privacy panel (lane I); focus light and tilt, the Top Shelf hero with 10's art, frosted glass B, the dither (lane J); editing | lanes I and J, one at a time on the shell | 7, 10 | look-and-feel.md's three checks |
| 12 | Guard G2 with the address rules (lane H, after a week of G1's log), Guard G3 (lane K, after P4), senders G4 (TV, one job per check) | lanes H and K, TV | 6 | privacy.md |
| 13 | **`lumen-ctl`** (Remote R2): the root control binary behind an SSH forced command, two bus implementations behind one trait, `tools/remote.sh`, a PC driver | lane G, then TV | 4 (done) | remote.md R2's "done when" on the B9. **Built and merged 2026-10-31** (2df8600; remote.md R2, "Built on"): the crate, the command and library buses, `tools/remote.sh` with a staged, hash-checked install and its `authorized_keys` line, `tools/remote/ctl.py`. Not yet run on the TV; its first TV steps are `remote.sh install`, `probe` and `ctl.py check-door` (the client's environment must not reach the forced command) |

Lanes F (parts 1, 2 and 4) and G can start now; E after the studio merge;
F's tile, I and J after the shell merge; H after a week of Guard's log; K
after P4. Two or three lanes at a time, never two on the shell's files at
once; the README says how each comes back.

### What this changes in the roadmap

* Phase 1's rows become privacy.md's stages; the 24 h capture and the
  Glasshouse install are no longer prerequisites.
* Phase 3's takeover is decided by the Home look, not fixed to the key
  filter.
* Phase 4 gains the art lane and OpenHome's settings map as inputs.

## Decisions for the owner

| | Question | Recommendation |
|---|---|---|
| D1 | Commit the privacy work on `main` now (your post-boot check first, or trust the 2026-10-28 log and check later) | Done 2026-10-30: commits 354d989 to 655518b, pushed |
| D2 | Home: route D proven on 2026-10-31. Keep the setting on, or set LG's Home back until Lumen handles a long Home press | Kept on, the owner's choice the same day: Lumen is the B9's Home. Undo: one setSystemSettings back to com.webos.app.home |
| D3 | YouTube AdFree through Homebrew Channel | Yes, after the look says which copy SAM picks; one IPK, reversible |
| D4 | Lanes: A Guard (Astra high), B frame-stats (Sol high), C remote survey (Sol high), D remote front-end (Argon high, after remote.md), E art (Gemini image generation) | Superseded 2026-10-31: A to D ran as Claude sessions and are merged; from now on every lane is a Claude Opus 5.5 session at extra-high effort (the owner's choice), and E is remade without an image model |
| D5 | remote.md: approve as drafted on 2026-10-28, with your D1 to D13 answers | Approved 2026-10-31, merged into `main` |
| D6 | plx-native: pin a 0.10.0 nightly for a look at the HDR and remote changes, or stay on 0.8.0 | Stay until a tagged 0.9 or 0.10; nightlies change daily |
| D7 | Glasshouse: still install at all | Decide after G1's week, as P2 says; if yes, for OLED care and remote keys only |
| D8 | Art: the generated images' rights follow the tool's terms; Lumen ships them CC BY-SA 4.0 | Falls away 2026-10-31: lane E makes the art by code from Lumen's own scenes, original work with no third party's terms |
| D9 | `make check` for `remote/`: add `npm ci`, lint, tests and the build to the container's check (Node 22 is already in the image), which fetches from the npm registry at check time and adds about a minute | Yes, with a cached npm volume in `tools/dev.sh`; Playwright stays out of it |
