# Lumen

**A clean, fast, tvOS-inspired home screen for one rooted LG OLED B9.** No ads,
no promotions, no telemetry: just your apps, your inputs, and screensavers that
use what an OLED panel can do.

> **Status: pre-alpha, Phase 3.** On the developer's B9 the native home screen
> is the TV's Home: apps and inputs with LG's own icons, launching, and motion
> paced to 60 Hz with a rare skipped frame. LG's reporting jobs are held down
> from boot, Lumen Guard logs the TV's DNS lookups, and an original HDR10 loop
> switches the panel into HDR. Measured on an OLED55B9PUA, webOS 4.10.2-31,
> firmware 05.50.00. The design accepted on 2026-11-05 (decisions 10 to 17)
> takes it lower: a resident shell on its own Rust platform layer, a root
> daemon, default-deny DNS. See [docs/roadmap.md](docs/roadmap.md).

Lumen is an open-source project for one TV: the developer's LG OLED B9
(2019, webOS 4.5). On any other set it refuses to start (decision 10). It is
not affiliated with or endorsed by LG Electronics or Apple, and it is not
[ammar027/lumen-webos](https://github.com/ammar027/lumen-webos), a web
launcher of the same name for webOS 6 and later.

## What it is

A TV that is just a TV: the HDMI inputs, Netflix, Prime Video, YouTube and
Plex, a screensaver made for the panel, and LG's picture and sound settings.
Nothing else. Lumen is installed as ordinary apps and boot scripts on top of
Homebrew Channel's root. Nothing is flashed, and the TV's system partitions
are never written.

```
┌──────────────────────────── Lumen (this repo, GPL-3.0+) ────────────────────────────┐
│ shell/     native tvOS-style home and screensaver, Rust   takeover/  Home key        │
│ lumend     root daemon, Rust (planned)                    Guard      DNS, Rust       │
│ lumen-ctl  control over SSH, Rust                         remote/    Lumen Remote    │
│ profiles/  privacy hooks and Guard's rules                studio/    HDR content     │
└──────────────────────────────────────────────────────────────────────────────────────┘
          Homebrew Channel (root, boot hooks, SSH)   plx-native (Plex)
          LG webOS: Settings, picture and sound engine, Netflix, Prime Video: untouched
```

* **The home screen** is native code drawing straight on the GPU, because the
  TV's web engine (Chromium 53) can't hold 60 fps with the effects a modern
  design needs. [docs/architecture.md](docs/architecture.md) explains the
  layers and how they stay out of each other's way.
* **LG's picture and sound settings stay LG's.** Lumen replaces the home screen,
  not the TV. Quick toggles write through LG's own settings service.
* **Privacy** comes in layers below the hosts file
  ([docs/specs/privacy.md](docs/specs/privacy.md)): LG's reporting jobs held
  down from boot, LAN ports closed, and Lumen Guard, a small DNS forwarder
  that logs every lookup it sees. Next it blocks by default, so only the
  names the TV's apps need resolve, and sends lookups to Quad9 over TLS.
  The LG Content Store keeps working, and Netflix and Prime Video keep
  updating. Only the owner's PC or phone can loosen it.
* **HDR screensavers** are HDR10 and Dolby Vision video loops made for OLED,
  played by the shell on the TV's video plane, since its graphics layer is
  always SDR.

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
[plx-native](https://github.com/GLinnik21/plx-native) and the
[webosbrew](https://www.webosbrew.org/) community, and learns from
[Glasshouse](https://github.com/rorygallagher2024/glasshouse)'s research. See
[THIRD-PARTY.md](THIRD-PARTY.md).

## License

[GPL-3.0-or-later](LICENSE). Original video and art are CC BY-SA 4.0
(decision 17).
