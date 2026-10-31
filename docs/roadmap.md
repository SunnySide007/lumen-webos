# Roadmap

Each phase ships something usable on its own and ends with a check on the real
TV. The detailed plan for a phase is written when the previous one finishes,
from what it measured.

| Phase | Delivers | Done when |
|---|---|---|
| **0. Recon and dev environment** | Repo, build container, simulator, `hello-gles`, read-only device survey | A recon report for the B9 is in `docs/device/`, and `hello-gles` shows its GPU and draws at 60 fps on the TV |
| **1. System layer** | Glasshouse and plx-native installed and configured; `profiles/glasshouse` from a 24 h DNS capture; `store-safe-check.sh`; `uninstall-all.sh` | Blocked hosts resolve to 0.0.0.0; Netflix, Prime Video, plx-native, the Content Store and DRM playback still work; costs (voice, ThinQ) confirmed |
| **2. Ambient** | HDR QML screensaver and the first original HDR10 loop | The TV switches to HDR on the loop; it starts from LG's idle timer and from the shell; it reverts cleanly |
| **3. Shell MVP** | Native home with apps, inputs and launching; Home key takeover; plx-native pinned | Home to shell in under 150 ms; 60 fps navigation; every app launches; every escape hatch works alongside Glasshouse; the snappy targets in [look and feel](specs/look-and-feel.md) pass |
| **4. Polish** | Top Shelf hero, glass dock, parallax icons, Control Center, app editing, more loops, Dolby Vision | The reference screens pass the owner's review on the TV, and the frame budget holds with each feature on screen ([look and feel](specs/look-and-feel.md)); toggles match LG Settings |
| **5. Release** | CI-built IPKs, docs, screenshots, Homebrew Channel submission; upstream pull requests (B9 profile and HDR saver to Glasshouse, deep links to plx-native) | A clean install on a freshly reset set, following only the docs |
| **P. Play** (parallel, proposed) | RetroArch measured on the B9, one controller working, a Games row, OLED care, the phone path ([spec](specs/play.md), decision 7) | A pad plays a game started from the shell at full speed on the B9; every step reverts |
| **R. Remote** (parallel, approved 2026-10-31) | `lumen-ctl` on the TV behind an SSH forced command, Lumen Remote for Windows, the phone's web UI served by the TV, Google Home through the Streamer's CEC ([spec](specs/remote.md), decision 9) | The PC and the Pixel control power, volume, inputs, apps, the remote's keys and picture presets; LG's gateway ports closed; nothing opened to the internet; every step reverts |

Status on 2026-10-30, how deep the B9 lets Lumen reach, the upstream prior
art and the next steps with hand-off lanes: [specs/depth.md](specs/depth.md)
(proposed; the lanes were handed to Opus 5.5 sessions on 2026-10-31). On
2026-10-31 the shell and the HDR studio merged into `main`; the shell has
been the B9's Home since that morning (route D), so Phase 3 is under way.

## Phase 0 checklist

- [x] Repository, license, contribution and safety docs
- [x] Build container: webosbrew toolchain, Rust 1.99 (armv7), LG's packager, linters
- [x] `hello-gles`: SDL2 at run time, GLES2, frame stats, remote keys, screenshots
- [x] PC simulator (`tools/sim.ps1`), colour-accurate to the TV's framebuffer
- [x] TV tools (`tools/tv.sh`) and read-only survey with redaction (`tools/recon.sh`)
- [x] SSH key on the TV and `.env` (owner)
- [x] Recon report for the B9 → `docs/device/b9.md`
- [x] `hello-gles` on the B9: Mali-T820, a steady 60 fps; BACK is 482, and HOME
  never reaches the app (the compositor takes it). Its BACK handling needs the
  TV's SDL key-event layout, recorded in `docs/device/b9.md`.

## Phase 1 checklist

Off the TV (Stage A):

- [x] On-TV capture: `tools/capture.sh` and a static tcpdump build
  (`make capture-tools`)
- [x] Capture summary: `tools/capture/summarize.py`
- [x] `profiles/glasshouse`: allowlist, firmware blocks, candidate list,
  recipe, pinned versions
- [x] `tools/store-safe-check.sh`, `tools/uninstall-all.sh`,
  `tools/glasshouse-custom.sh`, `tools/tv.sh push` and `tunnel`

Proposed on 2026-10-27, awaiting the owner: [specs/privacy.md](specs/privacy.md)
replaces the 24 h capture with a lookup log that restarts at boot (Lumen
Guard), and adds layers below the hosts file. Its read-only survey,
`tools/recon.sh --privacy`, ran on 2026-10-28: the B9 has an IPv4 firewall
in its kernel, and none for IPv6.

On the TV ([runbook](runbooks/phase1-tv.md)):

- [x] plx-native 0.8.0 installed from the verified IPK; signed in, plays from Plex
- [ ] 24 h baseline capture, fetched and summarised. Both tries stopped
  early: the first (from 19:01 PDT 2026-10-20) after about 4 h, the second
  (from 18:48 PDT 2026-10-21) after about 1 h 8 min. Stage C needs a new one
- [ ] `b9-aggressive.hosts` and `allowlist.txt` refined from the capture (owner reviewed)
- [ ] Glasshouse 0.118.0 installed (owner confirmed), dashboard on 127.0.0.1 only
- [ ] Stage 1, ads & telemetry tier: store-safe check and app checklist pass
- [ ] Stage 2, privacy toggles and LG services: check and checklist pass
- [ ] Stage 3, TV software updates blocked: firmware hosts resolve to 0.0.0.0
- [ ] Stage 4, custom list uploaded: check and checklist pass
- [ ] Cold boot: everything reapplied; check and checklist pass again
- [ ] Costs confirmed: voice search, ThinQ and LG Channels fail; nothing else does
- [ ] Second 24 h capture with blocking on: no allowlisted name blocked
- [ ] `uninstall-all.sh` dry run reviewed; its Lumen-only part run for real
- [ ] Decision 6 accepted; b9.md notes (resolver, tools on the TV)

## Play checklist (proposed)

The owner approved [specs/play.md](specs/play.md) on 2026-10-21; P0 and P1
run first, side by side.

- [x] Research with sources (`docs/research.md`, "Play")
- [x] Input, Bluetooth, USB and network survey of the B9 (`docs/device/b9.md`)
- [x] P0: `lumen-tv-check` skill: on main on 2026-10-22 (8ec43a9); it
  measured hello-gles on 2026-10-21 and the home screen's B2/B3 frame time
  on 2026-10-22 (c500289)
- [ ] P1: one controller working end to end. Parked 2026-10-21: the Series pad
  sends nothing by cable and won't connect over Bluetooth; it comes back with
  more research and the USB helper ([spec](specs/play.md#p1-one-controller-working-end-to-end))
- [ ] P2: the shell reads the pad
- [ ] P3: RetroArch on the B9, measured per system
- [ ] P4: Games row
- [ ] P5: OLED care (game mode only if the owner wants it)
- [ ] P6: the phone path
- [ ] P7: owner-only features the owner picks
