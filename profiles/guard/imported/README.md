# Imported block lists

Lumen Guard reads every `*.txt` file here as a block list (decision 11):
a plain name blocks that name only, `*.name` blocks the name and every name
under it, and a hosts line's sinkhole address in front is skipped. Their
names count as carve-outs: where one is more specific than an allowed suffix
in `../allowlist.txt` it stays blocked, and an exact allowlist rule beats
them all. Under default deny the rest add nothing but a reason in the log
review (`tools/guard.sh log`), since a name no rule allows is blocked anyway.

Each file is kept as its source published it, with its license, apart from
the names Lumen measured.

| File | Source | Version | License |
|---|---|---|---|
| `hagezi-lgwebos.txt` | [hagezi/dns-blocklists](https://github.com/hagezi/dns-blocklists), `wildcard/native.lgwebos.txt`, commit `acded31b209c342f06c38af538281c7ed306202e` (2026-11-07) | 2026.1106.2258.55, 315 entries; SHA-256 `dddddf78…aac7` | GPL-3.0, as Lumen ([LICENSE](../../../LICENSE)) |
| `perflyst-smarttv.txt` | [Perflyst/PiHoleBlocklist](https://github.com/Perflyst/PiHoleBlocklist), `SmartTV.txt`, commit `bb8392bec4a08188a36035268d52a4a1c1087bde` (2026-09-09) | 13July2023v1; SHA-256 `d891473d…9d4a` | MIT, [LICENSE.perflyst](LICENSE.perflyst) |
| `glasshouse.txt` | [Glasshouse](https://github.com/rorygallagher2024/glasshouse), `server/lib/privacy.js`, commit `ca170fa31ee3edcaf403aa71f8fc988870e50b07` (2026-11-08): the names of its "ads & telemetry" tier, written out by Lumen | 2026-11-08 | MIT, [LICENSE.glasshouse](LICENSE.glasshouse) |

All three were fetched on 2026-11-08. Level1Techs' list has no license, so it
is only cited (docs/research.md).

## What they block under Lumen's allowed suffixes

Checked by `rules.rs`'s and `test_guard_rules.py`'s tests of the shipped
profile:

* hagezi: `*.rdx2.lgtvsdp.com` and `*.rdx2.nextlgsdp.com`, LG's report
  uploads beside the store.
* Perflyst: `us.nextlgsdp.com` and the other country names under
  `nextlgsdp.com` (not used by the B9 in G1's week),
  `device-metrics-us.amazon.com`, four `*.s3.amazonaws.com` and
  `*.elb.amazonaws.com` names, three Android TV names under
  `googleapis.com`, and `cert-test.sandbox.google.com`.
* Perflyst also lists `customerevents.netflix.com`, `ichnaea.netflix.com`
  and `tv-static.scdn.co`. The allowlist keeps those three by exact rules:
  Netflix's logging stays allowed until the owner tries blocking it (P5),
  and Spotify's TV files stay until a trial says otherwise.
* Perflyst notes that blocking `ngfts.lge.com` loses the store's
  thumbnails. Guard blocks it by default all the same (no rule allows it),
  which the trial checks.
* Glasshouse: the same report and beacon names, by region.

None blocks a name Guard must resolve (`MUST_RESOLVE`), and Guard refuses to
block if one ever did. `python3 -I tools/guard/rules.py NAME...` says which
rule decides a name.

## Refreshing

Fetch each file again from its source into an empty folder, read the diff,
and replace the file here whole; update the table (commit, version, SHA-256).
Then `tools/dev.sh make check` (the profile tests) and
`tools/store-safe-check.sh --hosts-file /dev/null --stage 5` must pass before
`tools/guard.sh push`.
