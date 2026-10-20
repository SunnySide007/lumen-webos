# profiles/glasshouse: aggressive, store-safe privacy (Phase 1)

How to configure [Glasshouse](https://github.com/rorygallagher2024/glasshouse)
for a quiet TV that still updates Netflix and Prime Video. Status: the files
are ready; the block list waits for the 24-hour capture of the B9.

| File | What it is |
|---|---|
| [aggressive.md](aggressive.md) | The settings recipe, in four stages, with the cost of each and the manual checklist |
| [allowlist.txt](allowlist.txt) | Names that must never be blocked (suffix rules, `=` for exact), each with its reason and source |
| [required-blocks.txt](required-blocks.txt) | LG's firmware update hosts, which must be blocked |
| [b9-aggressive.hosts](b9-aggressive.hosts) | Lumen's extra hosts for Glasshouse's custom list. Research candidates only, all commented out until the capture confirms them |
| [PINNED.md](PINNED.md) | The tested Glasshouse (0.118.0) and plx-native (0.8.0) builds, with SHA-256 |

**The recipe, in short:**

* blocker tier "ads & telemetry"
* every privacy toggle off
* LG's diagnostics, telemetry, ACR and voice services off
* unused preloaded apps off
* TV software updates blocked
* the custom list, in add mode
* the dashboard limited to the TV (`"host": "127.0.0.1"`, reached over
  `tools/tv.sh tunnel`); it has no login, so never port-forward it

**Never blocked:** the LG Content Store (`lgtvsdp.com` and its country hosts
on webOS 4), Widevine provisioning (`googleapis.com`), Netflix and Amazon
(including `*.amazonaws.com`; their licence servers are on those domains),
Plex, Spotify and YouTube. Glasshouse's "everything" tier
is **not** used, because on webOS 4 it blocks `lgtvsdp.com`, which is the
Content Store.

**Tools:**

* `tools/capture.sh` and `tools/capture/summarize.py`: the on-TV capture and
  its host table, which mark each name as allow, block or candidate.
* `tools/store-safe-check.sh`: proves the allowlist and the required blocks
  after every change.
* `tools/glasshouse-custom.sh`: uploads the custom list through the tunnel.
* `tools/uninstall-all.sh --glasshouse`: Glasshouse's documented undo.

The order of work is in
[docs/runbooks/phase1-tv.md](../../docs/runbooks/phase1-tv.md).

**Costs, stated where the profile is applied:** voice search, the ThinQ app
and LG Channels stop working.
