<!--
Title: Conventional Commits, for example `feat(shell): …` or `fix(tools): …`.
One logical change per pull request. See CONTRIBUTING.md.
-->

## What changed

<!-- What it does and why, in a few lines. -->

## How it was tested

<!-- What you ran, what you checked by hand, and what you saw. -->

TV: <!-- model and firmware, for example "LG OLED B9, webOS 4.5, 05.xx.xx", or "none" if nothing here runs on the TV -->

- [ ] `tools/dev.sh make check` passes
- [ ] Performance claims, if any, cite a measurement on the TV above

## Runs on the TV

<!-- Delete this section if nothing in the change runs on the TV. -->

Per docs/safety.md:

- [ ] Writes no system partition: only run-time bind mounts, Lumen's own configd
      values, boot scripts in `/var/lib/webosbrew/init.d` and Lumen's own files
- [ ] Reversible: ships with its undo, and backs up whatever it overlays
- [ ] Boot hooks check the kill switch (`/var/luna/preferences/lumen_disable`)
      first, background their work and are time-boxed
- [ ] Applied by hand and checked on the TV before anything was made persistent
