# Lumen

**A clean, fast, tvOS-inspired home screen for rooted LG webOS TVs.** No ads, no
promotions, no telemetry: just your apps, your inputs, and screensavers that
use what an OLED panel can do.

> **Status: Phase 0, pre-alpha.** The build pipeline works and `hello-gles`
> proves the native graphics path. There is no home screen yet. See
> [docs/roadmap.md](docs/roadmap.md).

Lumen is an open-source proof of concept developed on an LG OLED B9 (2019,
webOS 4.5). It is not affiliated with or endorsed by LG Electronics or Apple.

## What it is

Two layers, both installed as ordinary apps and boot scripts. Nothing is
flashed, and the TV's system partitions are never written.

```
┌──────────────────────────── Lumen (this repo, GPL-3.0+) ────────────────────────────┐
│ shell/     native tvOS-style home (Rust + SDL2 + GLES2)   takeover/  Home-key filter │
│ ambient/   HDR video screensaver                          studio/    HDR content     │
│ profiles/  privacy settings for Glasshouse                tools/     dev and TV tools│
└──────────────────────────────────────────────────────────────────────────────────────┘
┌────────────── System layer: existing projects, installed and configured ─────────────┐
│ Homebrew Channel (root, boot hooks, SSH)   Glasshouse (ad and telemetry blocking,    │
│                                            services, OLED care)   plx-native (Plex)  │
└──────────────────────────────────────────────────────────────────────────────────────┘
          LG webOS: Settings, picture and sound engine, Netflix, Prime Video: untouched
```

* **The home screen** is native code drawing straight on the GPU, because the
  TV's web engine (Chromium 53) can't hold 60 fps with the effects a modern
  design needs. [docs/architecture.md](docs/architecture.md) explains the
  layers and how they stay out of each other's way.
* **LG's picture and sound settings stay LG's.** Lumen replaces the home screen,
  not the TV. Quick toggles write through LG's own settings service.
* **Privacy and debloating** come from [Glasshouse](https://github.com/rorygallagher2024/glasshouse),
  configured so the LG Content Store keeps working and Netflix and Prime Video
  keep updating.
* **HDR screensavers** are HDR10 and Dolby Vision video loops made for OLED,
  since the TV's graphics layer is always SDR.

## Is it safe?

Lumen follows [docs/safety.md](docs/safety.md): runtime overlays only, every
change reversible, a kill switch, and nothing persistent until it has been
tested by hand. It still runs custom code as root on an embedded system. Use it
at your own risk, and don't update the TV's firmware while rooted: an update
can remove root.

## Building

You need Docker. Everything else comes in the build container.

```sh
tools/dev.sh make check    # formatting, lints and tests
tools/dev.sh make ipk      # cross-compile for the TV and package dist/*.ipk
```

On Windows, run these from Git Bash. To see the shell on your PC, in a window,
without a TV:

```powershell
pwsh tools/sim.ps1
```

[docs/setup.md](docs/setup.md) covers SSH access to the TV, installing, and the
read-only device survey.

## Credits

Lumen stands on [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel),
[Glasshouse](https://github.com/rorygallagher2024/glasshouse),
[plx-native](https://github.com/GLinnik21/plx-native) and the
[webosbrew](https://www.webosbrew.org/) community. See [THIRD-PARTY.md](THIRD-PARTY.md).

## License

[GPL-3.0-or-later](LICENSE). Original screensaver video content will be
licensed separately under CC BY-SA 4.0.
