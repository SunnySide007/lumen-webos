# Roadmap

Each phase ships something usable on its own and ends with a check on the real
TV. The detailed plan for a phase is written when the previous one finishes,
from what it measured.

| Phase | Delivers | Done when |
|---|---|---|
| **0. Recon and dev environment** | Repo, build container, simulator, `hello-gles`, read-only device survey | A recon report for the B9 is in `docs/device/`, and `hello-gles` shows its GPU and draws at 60 fps on the TV |
| **1. System layer** | Glasshouse and plx-native installed and configured; `profiles/glasshouse` from a 24 h DNS capture; `store-safe-check.sh`; `uninstall-all.sh` | Blocked hosts resolve to 0.0.0.0; Netflix, Prime Video, plx-native, the Content Store and DRM playback still work; costs (voice, ThinQ) confirmed |
| **2. Ambient** | HDR QML screensaver and the first original HDR10 loop | The TV switches to HDR on the loop; it starts from LG's idle timer and from the shell; it reverts cleanly |
| **3. Shell MVP** | Native home with apps, inputs and launching; Home key takeover; plx-native pinned | Home to shell in under 150 ms; 60 fps navigation; every app launches; every escape hatch works alongside Glasshouse |
| **4. Polish** | Top Shelf hero, glass dock, parallax icons, Control Center, app editing, more loops, Dolby Vision | Frame budget holds on the TV; toggles match LG Settings |
| **5. Release** | CI-built IPKs, docs, screenshots, Homebrew Channel submission; upstream pull requests (B9 profile and HDR saver to Glasshouse, deep links to plx-native) | A clean install on a freshly reset set, following only the docs |

## Phase 0 checklist

- [x] Repository, license, contribution and safety docs
- [x] Build container: webosbrew toolchain, Rust 1.99 (armv7), LG's packager, linters
- [x] `hello-gles`: SDL2 at run time, GLES2, frame stats, remote keys, screenshots
- [x] PC simulator (`tools/sim.ps1`), colour-accurate to the TV's framebuffer
- [x] TV tools (`tools/tv.sh`) and read-only survey with redaction (`tools/recon.sh`)
- [ ] SSH key on the TV and `.env` (owner)
- [ ] Recon report for the B9 → `docs/device/b9.md`
- [ ] `hello-gles` on the B9: renderer, frame times, BACK and HOME scancodes
