# Contributing

Thanks for helping. Lumen runs as root on people's televisions, so the bar is
correctness and reversibility first, features second.

## Ground rules

1. **Read [docs/safety.md](docs/safety.md) before touching anything that runs
   on the TV.** No writes to system partitions, every mod reversible, nothing
   persistent until it has been tested by hand.
2. **No telemetry.** Lumen sends nothing anywhere unless the user turns a
   feature on that needs it (streamed aerials, Plex).
3. **Measure on hardware.** Performance claims cite a measurement on a real set,
   with the model and firmware. The simulator is for layout, not for frame rates.
4. **Keep LG's settings LG's.** Lumen reads and writes TV settings only through
   LG's own services, never by editing their files.

## Development loop

Once per clone, turn on the pre-push hook, which runs `make check` before every
push (CI runs only on pull requests and release tags):

```sh
git config core.hooksPath .githooks
```

```sh
tools/dev.sh make check     # must pass before a pull request
tools/dev.sh make ipk       # build dist/*.ipk
pwsh tools/sim.ps1          # run the shell in a window on Windows
tools/tv.sh install dist/<file>.ipk && tools/tv.sh launch io.github.sunnyside007.lumen.shell
tools/tv.sh log             # what the shell logged on the TV
```

## Code style

* Rust: `cargo fmt` and `cargo clippy -D warnings`, both for the PC and for the
  TV target. Every `unsafe` block carries a `// SAFETY:` comment.
* Shell that runs on the TV: POSIX `sh` (the TV has busybox), `shellcheck`-clean.
* Root writes nothing into `/tmp` on the TV, which every app jail shares
  (and where nothing stops a link an app put there): logs and staged files
  go under `/var/lib/lumen`. Files pushed to the TV as root are staged under
  `/var/lib/lumen/stage/` and checked against the PC's SHA-256 while staged,
  before they go anywhere they can run. Boot hooks log to
  `/var/lib/lumen/log/<hook>.log`, keeping the run before as `.log.1`, and
  log nothing when that folder is not root's alone.
* JavaScript that runs on webOS 4 (key filters): ES5 only, since node 0.12 and
  Chromium 53 parse nothing newer.
* Everything that ships to the TV uses LF line endings (`.gitattributes`
  enforces this).

## Commits and pull requests

Conventional Commits (`feat(shell): …`, `fix(tools): …`). One logical change per
pull request, with how it was tested, and on which TV, in the description.

Work lands in batches: commit on a branch, open one pull request per milestone
(CI runs once), merge, and tag `v0.<phase>.0` when a roadmap phase is done.

## Licensing

By contributing you agree your work is licensed under GPL-3.0-or-later. Files
meant for upstream projects with other licenses (for example a screensaver for
Glasshouse) say so in their header.
