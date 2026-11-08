# Lumen

Native tvOS-style home screen for rooted LG webOS TVs: Rust + SDL2 +
GLES2 in `shell/`. Dev set: LG OLED B9, webOS 4.5. Phase 3 (the shell
is the B9's Home), with the privacy and remote tracks alongside; see
docs/roadmap.md and docs/specs/depth.md.

## Commands

Builds and checks run in the Docker build container (Git Bash on Windows):

- `tools/dev.sh make check`: fmt, clippy for the PC and TV targets
  (`-D warnings`), cargo test, and the Python and bats tool tests.
  Must pass before a pull request.
- `tools/dev.sh make ipk`: cross-compile for `armv7-unknown-linux-gnueabi`
  and package `dist/*.ipk`.
- `pwsh tools/sim.ps1`: the shell in a window on Windows. Layout only;
  frame rates come from the TV.

The HDR loop pipeline (studio/README.md) has its own container,
`tools/studio.sh`. `make check` skips its numpy tests; run them with
`tools/studio.sh python3 -I -m unittest discover -s tools/tests -p 'test_studio*.py'`.

The TV scripts (`tools/tv.sh`, `recon.sh`, `backup.sh`, `bench.sh`,
`guard.sh`, `remote.sh`, `lumend.sh`, `takeover.sh`) run on the host and
act on the TV over SSH as root. What runs without asking follows the tiers
in docs/safety.md, "Working without asking" (the owner's OK of
2026-11-08). Anything outside them is asked first.

## Rules

- Anything that runs on the TV follows docs/safety.md: no writes to system
  partitions, every change reversible, nothing persistent until tested by
  hand.
- Code style, commits and licensing: CONTRIBUTING.md. Every `unsafe` block
  has a `// SAFETY:` comment; TV shell scripts are POSIX sh (busybox);
  webOS key-filter JS is ES5; LF line endings.
- Performance claims cite a measurement on a real TV, with model and
  firmware.
- No telemetry.
