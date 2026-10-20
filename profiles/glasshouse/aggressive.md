# The aggressive, store-safe recipe

How to set up Glasshouse 0.118.0 ([PINNED.md](PINNED.md)) on the B9 for
aggressive privacy that keeps the Content Store, DRM, Netflix, Prime Video,
Plex, Spotify and casting working. The commands, expected output and
rollback for each step are in
[docs/runbooks/phase1-tv.md](../../docs/runbooks/phase1-tv.md), Stage C.

Apply the stages in order, one at a time. After **each** one:

1. `tools/store-safe-check.sh --stage N` passes, with N the stage just
   applied (the default, 4, also expects the firmware and custom blocks).
2. The manual checklist below passes.
3. Write down what changed. If anything fails, undo that stage (its
   "Undo" line) before going on.

Dashboard labels below are from Glasshouse's documentation; if 0.118.0 words
a control differently, follow the meaning and note the label in the runbook.

## Before the first stage

- The capture summary has been read, and `b9-aggressive.hosts` and
  `allowlist.txt` reflect it.
- **Dashboard on localhost only.** Glasshouse's dashboard (port 8080) has no
  password. Set `"host": "127.0.0.1"` in `/var/lib/tvweb/config.json` and
  reach it over `tools/tv.sh tunnel`. Never port-forward it.
- **Leave alone:** tile hiding (issue #366: black picture, no sound), remote
  key remapping, screensavers, and the `tvdataexchanger` and `alwaysready`
  services (never stop them).

## Stage 1: the "ads & telemetry" blocker tier

Turn on host blocking at the **ads & telemetry** tier. Not "everything": on
webOS 4 that tier blocks `lgtvsdp.com`, which is the Content Store.

- Cost: none expected. LG's home-screen ad tiles go blank.
- Undo: set the blocker tier back to off.

## Stage 2: privacy toggles and LG services

- Every privacy toggle off: viewing information (Live Plus/ACR), voice
  information, interest-based ads, and the LG terms that are optional.
- In the services list, turn off LG's diagnostics, telemetry, ACR and voice
  services. Keep the default for anything the dashboard marks as needed for
  the store, DRM or the picture. Never stop `tvdataexchanger` or
  `alwaysready`.
- Unused preloaded apps off, but only ones the owner never opens.
- Cost: **voice search stops working**, and LG's recommendations go quiet.
- Undo: the same toggles back on; Glasshouse's restore actions for services.

## Stage 3: block TV software updates

Turn on **Block TV software updates**. Then check that every name in
[required-blocks.txt](required-blocks.txt) resolves to `0.0.0.0` on the TV:
`tools/store-safe-check.sh` does this, and fails until they all do. The
Phase 0 survey found none of them blocked, despite Homebrew Channel's flag.

- Cost: no LG firmware updates. Intended: an update can remove root.
- Undo: turn the switch off. Never do this while the TV is rooted unless you
  mean to update.

## Stage 4: the custom list

Upload [b9-aggressive.hosts](b9-aggressive.hosts) in **add** mode, after its
confirmed entries are uncommented: `tools/glasshouse-custom.sh` (dry run
first), or the dashboard's custom-list upload.

- Cost: **the ThinQ app and LG Channels stop working**, plus whatever the
  confirmed entries' categories say.
- Undo: remove the custom entries in the dashboard (or upload an empty list
  in replace mode).

## After the last stage

Cold boot: unplug the TV for 30 seconds (the remote's power button is
standby with Quick Start+). Then re-run `tools/store-safe-check.sh` and the
checklist, to prove Glasshouse reapplies everything at boot.

## Manual checklist

All must pass, unless marked as an expected loss:

- [ ] Netflix plays a title (Widevine DRM).
- [ ] Prime Video plays a title (DRM).
- [ ] plx-native plays from the owner's Plex server.
- [ ] Spotify Connect from the Pixel plays on the TV.
- [ ] YouTube cast from the Pixel plays.
- [ ] Netflix cast from the Pixel plays.
- [ ] The Content Store opens, and "check for app updates" completes.
- [ ] Expected loss: voice search fails.
- [ ] Expected loss: the ThinQ app fails (after stage 4).
- [ ] Expected loss: LG Channels fails (after stage 4).
