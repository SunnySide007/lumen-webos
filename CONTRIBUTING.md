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

```sh
tools/dev.sh make check     # must pass before a pull request
tools/dev.sh make ipk       # build dist/*.ipk
pwsh tools/sim.ps1          # run the shell in a window on Windows
tools/tv.sh install dist/<file>.ipk && tools/tv.sh launch com.lumen.shell
tools/tv.sh log             # what the shell logged on the TV
```

## Code style

* Rust: `cargo fmt` and `cargo clippy -D warnings`, both for the PC and for the
  TV target. Every `unsafe` block carries a `// SAFETY:` comment.
* Shell that runs on the TV: POSIX `sh` (the TV has busybox), `shellcheck`-clean.
* JavaScript that runs on webOS 4 (key filters): ES5 only, since node 0.12 and
  Chromium 53 parse nothing newer.
* Everything that ships to the TV uses LF line endings (`.gitattributes`
  enforces this).

## Commits and pull requests

Conventional Commits (`feat(shell): …`, `fix(tools): …`). One logical change per
pull request, with how it was tested, and on which TV, in the description.

## Licensing

By contributing you agree your work is licensed under GPL-3.0-or-later. Files
meant for upstream projects with other licenses (for example a screensaver for
Glasshouse) say so in their header.
