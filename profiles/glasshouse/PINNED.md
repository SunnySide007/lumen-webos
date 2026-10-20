# Pinned versions

The exact builds Lumen tested on the B9, with where they came from and their
SHA-256. Install only these; a new version goes through the Phase 1 checks
again (`tools/store-safe-check.sh`, the manual app checklist, a cold boot)
before its line here changes.

Checked on 2026-10-20 by downloading each IPK to `dist/vendor/` (git ignores
it) and comparing `sha256sum` with the hash GitHub publishes for the release
asset (the `digest` field of the release API).

| Component | Version | Released | IPK | SHA-256 | Published hash |
|---|---|---|---|---|---|
| Glasshouse | 0.118.0 | 2026-10-19 | [io.github.rorygallagher2024.lg-webos-dashboard_0.118.0_all.ipk](https://github.com/rorygallagher2024/glasshouse/releases/download/v0.118.0/io.github.rorygallagher2024.lg-webos-dashboard_0.118.0_all.ipk) (969,146 bytes) | `287f1a5ca89061188aceb91a4d1c2e2e79ae6cb9a08bcb77fdf1bd741fdf8e2a` | GitHub release API digest: matches |
| plx-native | 0.8.0 | 2026-10-07 | [com.beb.plxnative_0.8.0_arm.ipk](https://github.com/GLinnik21/plx-native/releases/download/v0.8.0/com.beb.plxnative_0.8.0_arm.ipk) (19,603,158 bytes) | `150737a0395276f10f283192f9dc97193bf8c5e61ed58745037eee2a7195d3e2` | GitHub release API digest and the release's own `ipk.sha256` asset: both match |

App ids: Glasshouse is `io.github.rorygallagher2024.lg-webos-dashboard`;
plx-native is `com.beb.plxnative`.

## Re-check before installing

```sh
mkdir -p dist/vendor && cd dist/vendor
curl -fLO https://github.com/rorygallagher2024/glasshouse/releases/download/v0.118.0/io.github.rorygallagher2024.lg-webos-dashboard_0.118.0_all.ipk
curl -fLO https://github.com/GLinnik21/plx-native/releases/download/v0.8.0/com.beb.plxnative_0.8.0_arm.ipk
sha256sum *.ipk
```

The two hashes must equal the table's. On Windows, `curl -O` has been seen
saving `com_beb.plxnative_0.8.0_arm.ipk`; rename it before checking.

## Capture tools

`tools/capture/build.sh` pins its sources the same way: libpcap 1.10.7
(`68fa62cf…6a03`) and tcpdump 4.99.7 (`39675dfa…eec1`) from tcpdump.org,
whose PGP signatures checked out against The Tcpdump Group's key
`1F16 6A57 42AB B9E0 249A 8D30 E089 DEF1 D9C1 5D0D` on 2026-10-20.
