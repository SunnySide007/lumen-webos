# Research notes

What was learned before writing code (October 2026), and where it came from.
Measurements quoted here are other projects'; Lumen's own go in
`docs/device/` once taken on the B9.

## The TV

* **LG OLED B9 (2019):** webOS 4.5 (internally 4.10.x) on LG's α7 Gen 2 SoC.
  This is the same class as the LG 49SM9000 that plx-native profiles: a
  Mali-T820 MP2 GPU running **OpenGL ES 2 only**. The C9 beside it is α9 Gen 2
  (Mali-G71 MP2, Cortex-A73, ~2.2 GB RAM), per Glasshouse's C9 profile.
* **Engines:** Chromium 53 for web apps, **node 0.12**, a Qt 5 / QML compositor
  (`surface-manager`), and upstart instead of systemd.
* **Firmware:** 05.40.09 (December 2023) added an optional full-screen,
  webOS 6-style home to 2019 TVs, alongside the classic bottom bar
  ([FlatpanelsHD](https://www.flatpanelshd.com/news.php?subaction=showfull&id=1701855760)).
* **Root:** [dejavuln-autoroot](https://github.com/throwaway96/dejavuln-autoroot)
  installs [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel).
  That provides a root `exec` service, `init.d` boot hooks, SSH, failsafe mode
  and an update block.
* **Updates:** a firmware update can remove root and patch the exploit.

## No flashing

System partitions are signed, read-only squashfs. Mods are runtime bind mounts,
configd values and boot scripts, so uninstalling and rebooting restores stock.
There is no realistic custom-firmware route for these TVs, and none is needed.

## Graphics limits (from plx-native's measurements)

[plx-native](https://github.com/GLinnik21/plx-native)'s
[glass-hardware-budget.md](https://github.com/GLinnik21/plx-native/blob/main/docs/glass-hardware-budget.md)
and [liquid-glass.md](https://github.com/GLinnik21/plx-native/blob/main/docs/liquid-glass.md),
measured on a 49SM9000 (webOS 4.5, Mali-T820 MP2):

* **The frame budget:** 16.7 ms a frame, of which a moving poster grid already
  takes ~8.9 ms.
* **Backdrop-blur glass:** charged for the rectangle being blurred. It holds
  60 fps only under ~300,000 px, steps 60 → 45 → 36 → 30 above that, and a
  full-screen blur runs at ~24 fps. Glass pieces far apart cost as much as a
  full-screen blur.
* **Glass over video:** impossible. Video is on a hardware plane GL cannot read.
* **Idle cost:** a settled screen that stops repainting idles at ~2% of a core.
* **No HDR from GL:** the UI plane is 8-bit SDR, so HDR is only possible as video.

## Prior art

| Project | Relevance |
|---|---|
| [Glasshouse](https://github.com/rorygallagher2024/glasshouse) (MIT) | System layer: blocker, privacy, services, key filters, OLED care, home takeover (webOS 9/10 only), QML screensavers; verified on webOS 4 (B8, C9) |
| [plx-native](https://github.com/GLinnik21/plx-native) (GPL-3.0+) | Native Plex client; proof that Rust + SDL2 + GLES2 at 60 fps with glass works on this hardware |
| [webosbrew/custom-screensaver](https://github.com/webosbrew/custom-screensaver), [aabytt fork](https://github.com/aabytt/custom-screensaver-aerial) (MIT) | Screensaver bind mount over `com.webos.app.screensaver/qml/main.qml`; the fork plays 4K HDR and Dolby Vision aerials through QML `Video` |
| [webos-launch-home](https://github.com/gprot42/webos-launch-home) (MIT) | Web-app home replacement reported working on webOS 4 through a root watcher |
| [webos-galaxy-launcher](https://github.com/guntiss/webos-galaxy-launcher) | Home via `setDefaultApp` plus a foreground watcher (webOS 25); SSH `.ipk` install routine |
| [LG Input Hook](https://repo.webosbrew.org/apps/org.webosbrew.inputhook/) | Key remapping by injection; fallback for the Home key |

## Netflix, Prime Video and blocking

* **Root and DRM:** rooting does not affect DRM.
* **Store host:** on webOS 4 the Content Store runs on `lgtvsdp.com` (and its
  country hosts). Netflix and Prime Video update through it, so it must stay
  reachable.
* **Glasshouse's tiers:** its "everything" tier blocks that host; the
  "ads & telemetry" tier does not.
* **Never block AWS:** Prime Video runs on AWS, so `*.amazonaws.com` must never
  be wildcard-blocked.
* **Community blocklist:** [Level1Techs](https://forum.level1techs.com/t/lg-tv-block-mini-how-to/255178)
  maps LG's telemetry, ad, ACR and AI hosts. It was written for webOS 25, so the
  B9 gets its own 24-hour DNS capture before its list is written.

## Design reference

[tvOS 26](https://www.apple.com/newsroom/2025/06/apple-tv-brings-a-beautiful-redesign-and-enhanced-home-entertainment-experience)
introduced Liquid Glass:
* translucent, refracting Control Center platters and dock
* glass-edged poster art
* a full-bleed Top Shelf above the app row

Lumen borrows the direction, within the GPU limits above, and ships no Apple
assets. SF Pro may not be used off Apple platforms, so Lumen uses Inter.
