# profiles/glasshouse: aggressive, store-safe privacy (Phase 1, not started)

How to configure [Glasshouse](https://github.com/rorygallagher2024/glasshouse)
for a quiet TV that still updates Netflix and Prime Video.

**Planned contents:**

* `aggressive.md`, the settings recipe:
  * blocker tier "ads & telemetry"
  * every privacy toggle off
  * LG's diagnostics, telemetry, ACR and voice services off
  * unused preloaded apps off
  * TV software updates blocked
  * the dashboard limited to the TV or LAN (it has no login, so never port-forward it)
* `b9-aggressive.hosts`, extra hosts for Glasshouse's custom blocking (ThinQ,
  voice search, AI and recommendations), built from a 24-hour DNS capture of
  the TV.
* A pinned, tested Glasshouse version.

**Never blocked:**

* the LG Content Store (`lgtvsdp.com` and its country hosts on webOS 4)
* Widevine and PlayReady license servers
* Netflix and Amazon, including `*.amazonaws.com`
* Plex

Glasshouse's "everything" tier is **not** used, because on webOS 4 it blocks
`lgtvsdp.com`, which is the Content Store.

`tools/store-safe-check.sh` will prove the allowlist after every change.

**Costs, stated where the profile is applied:** voice search, the ThinQ app
and LG Channels stop working.
