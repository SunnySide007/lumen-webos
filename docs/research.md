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

## Play: emulators, controllers and the phone

Researched on 2026-10-20 for the proposed Play track. Nothing here has run on
the B9 yet, so each point is marked high, medium or low confidence.

### Emulators

* **RetroArch already exists for webOS.** It's in the Homebrew Channel repo as
  [com.retroarch.webos](https://repo.webosbrew.org/apps/com.retroarch.webos/),
  1.22.2 (updated 2025-11-21), no root needed, from
  [webosbrew/RetroArch](https://github.com/webosbrew/RetroArch) (GPL-3.0). High.
  * **Builds:** its [release notes](https://github.com/webosbrew/RetroArch/releases)
    ship a GLES 2 build for webOS up to 4.4.2 and a GLES 3 build for 4.10.0 and
    later. The GLES 3 build is recommended, because some cores need GLES 3. The
    B9 is 4.10.2 and reports OpenGL ES 3.2 (b9.md; the "GLES 2 only" above is
    plx-native's claim), so the GLES 3 build applies. Medium: an older release
    gives "4.5+" as the threshold instead.
  * **Cores and games:** cores come from RetroArch's Online Updater. ROMs load
    from `/tmp/usb/...` on rooted sets or over SFTP, and controllers are paired
    with webOS itself (v1.17.0 notes). Medium.
* **Other ports:** ScummVM (a [2026.3.0 port](https://lists.scummvm.org/pipermail/scummvm-devel/2026-August/013079.html),
  tested on webOS 6.5.3 only), Chocolate Doom, OpenTTD and 3D Pinball
  ([repo](https://repo.webosbrew.org/apps/)). Medium.
* **What should run:** four Cortex-A53 cores with a 32-bit userland are a
  Raspberry Pi 3's CPU class, and the Mali-T820 MP2 is the stronger GPU. On a
  Pi 3, RetroPie runs everything up to PS1 well, N64 runs a quarter to half of
  its library, and Dreamcast and PSP run only a few games
  ([RetroPie forum](https://retropie.org.uk/forum/post/214946?page=2),
  [N64, Dreamcast and PSP](https://retropie.org.uk/forum/post/63899)). Expect
  NES, SNES, Genesis, Game Boy, GBA and PS1, with N64 partial and PSP and
  Dreamcast unlikely. Low until measured: the B9 doesn't expose its clock
  speeds or thermal zones (b9.md), and it had only ~335 MB of RAM free at the
  survey, so only a run on the set will tell.
* **Storage:** `/var` has 272 MB free (b9.md), so ROMs and saves belong on a
  USB drive.

### Controllers

* **LG doesn't promise Bluetooth gamepads on 2019 sets.** Its gaming page
  lists the DualShock 4, Xbox One S, Xbox Series X and Shield controllers as
  supported from 2021 models
  ([LG](https://www.lg.com/ae/support/product-help/CT20076005-20152528678516LST)).
  Its 2019 (webOS 4.5) pairing path covers only the LG Wireless Keyboard, and
  the "Connect Bluetooth controller" menu starts with webOS 5.0
  ([LG](https://www.lg.com/us/support/help-library/lg-tv-how-to-connect-a-keyboard-mouse-or-gamepad-to-your-lg-tv--20152890341179)).
  High that it's unsupported; unknown whether the stack can do it anyway.
* **Root route:** webOS's `com.webos.service.bluetooth2` has discovery and pair
  calls that work with `luna-send`
  ([webOS OSE](https://webosose.org/docs/reference/ls2-api/com-webos-service-bluetooth2),
  [an example](https://forum.webosose.org/t/using-the-new-bluetooth-audio-profiles/738)).
  On the B9 that service answers. It is LG's own stack on a MediaTek MT7668,
  with no BlueZ, and it has a `/hid` profile. **Measured 2026-10-21:** an Xbox
  Series pad pairs as a BLE device, but HID never connects and a GATT link
  drops at once, so this route fails for that pad (b9.md). A classic-Bluetooth
  pad, such as a DualShock 4, may still connect through `/hid`. Untried.
* **Xbox pads use Bluetooth LE now.** The Series pads do, and Microsoft's 2021
  firmware moved the One S pad (model 1708) to BLE and changed its product ID
  from 0x02fd to 0x0b20
  ([kernel patch](https://lkml.iu.edu/hypermail/linux/kernel/2210.2/07369.html),
  [Windows Central](https://www.windowscentral.com/xbox-controller-firmware-update-brings-xbox-series-x-s-features)).
  A 2019 Bluetooth stack is less likely to handle BLE HID than classic HID.
  Medium.
* **The TV's SDL is 2.0.5 (2016)**, older than these pads, and SDL learned
  their IDs (0x0b13, 0x0b20) later
  ([SDL commit](https://git.axiodl.com/encounter/SDL/commit/907943a2366bc1f633da5ccc805a77f78ceee475)).
  Lumen would add its own mapping with `SDL_GameControllerAddMapping`. Medium.
* **A USB cable isn't enough for a Series pad, measured.** On the B9 (firmware
  05.50.00) the built-in `xpad` driver bound an Xbox Series controller
  (`045e:0b12`) as "Generic X-Box pad", but a 15-second read gave no input
  while every control was used, and nothing had grabbed the node (b9.md,
  2026-10-21). Its Xbox button kept blinking, so the start-up handshake never
  completed; the kernel's `xpad` is Linux 4.4, older than the pad. Ways round
  it, all untried:
  * Lumen drives the pad itself over USB (`usbfs`) and passes its input on
    through `uinput`. Both need root.
  * Pair it over Bluetooth, where its input would arrive as HID, without
    `xpad`.
  * Use a pad this kernel already knows: Xbox 360-protocol pads (many
    third-party pads have that mode), or a DualShock 4 by cable through the
    built-in `hid-sony`.

  LG also lists USB gamepads (Xbox 360 wired, Logitech F310/F510/F710) for
  every smart TV since 2012.

### The Android phone

* **It can't be a stream host.** Sunshine (Moonlight's host) and punktfunk's
  host run on PCs, and no Android host turned up
  ([Moonlight for Android](https://f-droid.org/de/packages/com.limelight/)).
  The TV-side clients exist: [Moonlight](https://repo.webosbrew.org/apps/com.limelight.webos/)
  1.6.36 and [punktfunk](https://repo.webosbrew.org/apps/io.unom.punktfunk.client-webos/)
  0.24.0 (webOS 4+), both without root, so a PC host added later is one
  install. High for the clients; medium that no Android host exists.
* **Backup options, best first:**
  1. **A USB-C to HDMI cable** into a spare HDMI port, if the phone has
     DisplayPort Alt Mode (check its spec sheet). It adds the least lag, and the
     shell already shows inputs as tiles. Medium: it depends on the phone.
  2. **Screen Share (Miracast)**, built into the B9: wireless, but estimates put
     it at 20–100 ms of extra lag, which is too slow for fast games
     ([LG](https://www.lg.com/us/support/help-library/using-miracast-bluray--1426625460231)).
     Low: no measurements.
  3. **The phone as a controller over Wi-Fi.** RetroArch's Remote RetroPad
     sends one RetroArch's input to another over UDP, port 55400 by default
     ([libretro docs](https://docs.libretro.com/library/remote_retropad/)), so
     RetroArch on the phone could act as a pad. Medium: the wire format isn't
     documented, and anything listening on the LAN needs pairing. Making a
     virtual pad for every app would need `/dev/uinput`, which is root-only on
     the B9.
  4. **The phone as a library:** ROMs and saves copied over SFTP, which the TV
     already serves.

### OLED care

* **The risk is uneven wear, not the bars.** Black pillarbox bars don't emit
  light, so long 4:3 sessions age the middle of the panel faster than the
  edges, and static HUDs wear their own spots. Owners report no burn-in from
  mixed 4:3 play on 2018 LG OLEDs, with a caution about one game for hours
  every day ([shmups forum](https://shmups.system11.org/viewtopic.php?p=1425360),
  [What Hi-Fi](https://www.whathifi.com/advice/oled-burn-in-should-you-be-worried-about-it-and-how-can-you-prevent-it)).
  Low: anecdotes only.
* **What Lumen can add:** a dim, slowly moving fill in the bars, dimming and
  pausing a game left idle, and handing over to the HDR screensaver after
  that. LG's own Screen Shift and Logo Luminance stay on.

### What it means for the plan

Adopt RetroArch's GLES 3 build instead of writing a frontend. Lumen's part is
the Games row, controller mapping, game mode, OLED care and the phone path.
The controller is still the biggest risk. The Series pad binds by cable but
sends nothing, and over Bluetooth it pairs but never connects (measured
2026-10-21). P1 is parked until there's more research and the user-space USB
helper. Meanwhile RetroArch can be measured with the Magic Remote, and the
phone-as-a-pad route matters more.

## Remote: control channels, Google Home and the desktop app

Researched on 2026-10-28 for the proposed Remote spec
([specs/remote.md](specs/remote.md)). Nothing here has run on the B9 yet
except the privacy surveys of the same day, so each point is marked high,
medium or low confidence.

### What the B9 already offers

* **The survey's facts** (`docs/device/privacy-20261028-061746.txt`):
  * tcp 3000 and 3001 are `xinetd` → `nsl` → `com.webos.service.secondscreen.gateway`
    (LG's second-screen API, "SSAP").
  * tcp 36866 and 18181 are DIAL, through the same `nsl`.
  * LG's network settings read `wolwowlOnOff: "true"` (wake on LAN and
    wireless LAN), `allowMobileDeviceAccess: true` and `enableIpControl: "off"`.
  * The TV is on Wi-Fi (`wlan0`); `eth0` isn't used.

  High.
* **SSAP** (LG's second-screen API) is a WebSocket API on 3000 (plain) and 3001
  (TLS). A client registers once, the TV shows a prompt, and the client keeps
  the key it gets back. It reaches power off, volume, inputs, apps, toasts and
  a pointer socket for remote buttons. Home Assistant's
  [webOS TV integration](https://www.home-assistant.io/integrations/webostv/)
  uses it, needs "LG Connect Apps" on, and can't turn the TV on: it needs a
  Wake-on-LAN automation, which "usually only works" on the same network. High.
* **SSAP's security record:**
  * **The 2024 bugs.** Bitdefender's
    [2024 report](https://www.bitdefender.com/blog/labs/vulnerabilities-identified-in-lg-webos)
    found that the gateway's PIN could be skipped (CVE-2023-6317), followed by
    root command injection. Both the bypass and one of the injections
    (CVE-2023-6319, `getAudioMetadata`) were confirmed on webOS 4.9.7, on a
    2019 set (LG43UM7000PLA), so the B9's generation. LG patched them on
    2024-03-22. High.
  * **The October 2026 chain.** It gets root from the LAN through port 3001
    with no PIN ([SSD advisory](https://ssd-disclosure.com/lg-webos-sync-demo-preauth-rce/),
    [The Hacker News](https://thehackernews.com/2026/10/lg-smart-tvs-vulnerable-to-pre-auth.html)).
    It was tested only on webOS 7.5.7, and no workaround was given but the
    update. High for 7.5; unknown for 4.5.
  * **The B9 can't be patched either way.** Whether firmware 05.50.00 has
    the 2024 fixes is unknown: no source names it.
    [dejavuln-autoroot](https://github.com/throwaway96/dejavuln-autoroot),
    which rooted this TV, works on unpatched webOS 3.5 and later. And updates
    stay blocked (safety rule 8), so no fix will arrive. Medium.
* **Remote keys:** LG offers apps no API for sending remote keys
  ([LG developer forum](https://forum.webostv.developer.lge.com/t/send-key-messages-by-web-apps/397)).
  As root, the routes are SSAP's pointer socket, LG's input services over
  luna, or a kernel `uinput` device (whether the B9's kernel has it is
  unchecked). Low until tried on the TV.
* **SSH:** Homebrew Channel's dropbear already listens on 22, key only.
  Dropbear's `authorized_keys` takes `command="…"` (the client's command is
  ignored and handed over in `SSH_ORIGINAL_COMMAND`), `no-pty`,
  `no-port-forwarding` and, in newer releases, `restrict`
  ([dropbear(8)](https://man.archlinux.org/man/dropbear.8)). Which release the
  TV runs is unchecked. High for dropbear; unknown for the TV's build.
* **DIAL** only starts DIAL apps (YouTube, Netflix) with a payload. It can't
  change the volume or the input. High.

### Google Home without LG's cloud

* **Google TV Streamer over HDMI-CEC.** With a Chromecast or a Google TV
  Streamer on the TV and CEC on, "Hey Google, turn on/off the TV" works
  ([Google](https://support.google.com/chromecast/answer/7498991)). Google
  describes it for its speakers and displays; whether the Pixel's assistant
  does the same is unchecked. Owners report the Streamer turning the TV back
  on about 10 s after it's turned off
  ([Google Nest Community](https://www.googlenestcommunity.com/t5/Streaming/Google-Streamer-4k-When-turning-TV-off-device-will-turn-TV-back-on/td-p/736320)),
  and CEC volume is uneven across reports. Medium.
* **Matter hubs:** Google's list includes the Nest Wifi Pro and the Google TV
  Streamer (4K), both in this house. Its 32 supported Matter device types
  include "Speaker" (on/off and level), but no TV or video player, and device
  types not listed aren't supported
  ([Google Home developers](https://developers.home.google.com/matter/supported-devices)).
  High.
* **Home Assistant as a Matter bridge.**
  [home-assistant-matter-hub](https://github.com/RiDDiX/home-assistant-matter-hub)
  is the maintained fork; the original was archived in January 2026. It maps
  `media_player` to a Matter Speaker or a Basic Video Player. Google would see
  only the Speaker: on, off and volume level. One user reports only on/off
  for TVs with it. Medium.
* **Home Assistant's Google Assistant integration**
  ([docs](https://www.home-assistant.io/integrations/google_assistant/)):
  * **What it reaches:** a `media_player` gets on/off, volume, source (inputs,
    and the LG apps in the source list) and playback.
  * **Self-hosted:** a cloud-to-cloud project in Google's Home Developer
    Console, with Home Assistant reachable from the internet "with a hostname
    and SSL certificate". Its test mode is reported to expire after about 30
    days.
  * **Local fulfillment:** Google's devices can send commands straight to
    Home Assistant, but still use the internet for credentials and as a
    fallback.

  High.
* **Nabu Casa's Home Assistant Cloud** does the internet side for a
  subscription ([pricing](https://www.nabucasa.com/pricing/): 6.50 a month or
  65 a year in the listed currency). Commands pass through its relay. It is
  reported to be renamed "Home Assistant Link" from Home Assistant 2026.12.
  Medium.
* **Whichever route:** Google's servers process the voice. LG ThinQ, the
  usual way to link an LG TV, is blocked by decision 5.

### The desktop app

* **Tauri 2** uses Windows' own WebView2, so its installer stays small. Its
  backend is Rust, like the shell.
  * **Tray:** an icon with click events
    ([tray](https://v2.tauri.app/learn/system-tray/)), plus a positioner
    plugin that places a window relative to the tray icon
    ([positioner](https://v2.tauri.app/plugin/positioner/)).
  * **Glass:** a transparent window with Acrylic (Windows 10 and 11) or Mica
    (11 only) behind it
    ([window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/),
    [window-vibrancy](https://github.com/tauri-apps/window-vibrancy)).
    Acrylic is slow while the window is dragged.

  High.
* **Size and speed:** blog comparisons give Tauri installers of a few MB
  against 80–200 MB for Electron, and a faster start. On Windows, memory is
  about the same, since both run Chromium
  ([tauri#5889](https://github.com/tauri-apps/tauri/issues/5889)). Low until
  measured.
* **WebView2's data:**
  * **Optional data** follows Windows' Diagnostic data switch.
  * **Required data** is collected "regardless of" that switch.
  * **What an app can turn off:** SmartScreen
    (`IsReputationCheckingRequired = false`), and crash dumps can stay local
    (`IsCustomCrashReportingEnabled`)
    ([Microsoft](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy)).

  So an app on WebView2 can promise that it sends nothing itself, but not that
  Windows' component sends nothing. High.
* **Electron** ships its own Chromium, so its updates are the app's job.
  Electron's crash reporter does nothing until the app starts it
  ([docs](https://www.electronjs.org/docs/latest/api/crash-reporter)). Whether
  its Chromium makes other background calls is unchecked. Medium.
* **Other stacks:** WinUI 3 or WPF (native look, Composition animations, C#)
  and Wails (Go on WebView2) give no web animations, or bring in a third
  language.
