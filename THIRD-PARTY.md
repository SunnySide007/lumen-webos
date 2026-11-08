# Third-party work

Lumen builds on, and learns from, these projects. Code copied or adapted from
any of them is marked at the point of use with its origin and license.

| Project | Used for | License |
|---|---|---|
| [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel) | Root access, boot hooks (`/var/lib/webosbrew/init.d`), SSH, failsafe mode | MIT |
| [Glasshouse](https://github.com/rorygallagher2024/glasshouse) | Not installed (decision 13). Its research on webOS 4 key filters, home takeover, ACR and blocklists informs `takeover/` and `profiles/`; the names of its "ads & telemetry" tier are imported for Lumen Guard in `profiles/guard/imported/glasshouse.txt`, with its notice beside them (decision 11) | MIT |
| [hagezi/dns-blocklists](https://github.com/hagezi/dns-blocklists) | Its LG webOS tracker list, unmodified, as one of Lumen Guard's imported block lists (`profiles/guard/imported/hagezi-lgwebos.txt`) | GPL-3.0 |
| [Perflyst/PiHoleBlocklist](https://github.com/Perflyst/PiHoleBlocklist) | Its SmartTV list, unmodified, as one of Lumen Guard's imported block lists (`profiles/guard/imported/perflyst-smarttv.txt`, its licence beside it) | MIT |
| [plx-native](https://github.com/GLinnik21/plx-native) | The Plex client Lumen pins. Its measured GPU budgets, Liquid Glass notes and build approach inform `shell/`. The `getauxval` shims in `shell/.../compat.rs` and `lumen-guard`'s `sys.rs` follow its `src/compat/getauxval.c`. The PlxNative name and brand are excluded from its license (see its TRADEMARKS.md) and are not used here | GPL-3.0-or-later |
| [webosbrew/custom-screensaver](https://github.com/webosbrew/custom-screensaver) and [aabytt/custom-screensaver-aerial](https://github.com/aabytt/custom-screensaver-aerial) | The QML screensaver bind mount that `ambient/` will build on | MIT |
| [webosbrew/SDL-webOS](https://github.com/webosbrew/SDL-webOS) | The webOS remote's scancodes (`SDL_SCANCODE_WEBOS_*`) and SDL hints | zlib |
| [openlgtv/buildroot-nc4](https://github.com/openlgtv/buildroot-nc4) | The cross-compiling toolchain in `docker/Dockerfile` | GPL-2.0 (toolchain) |
| [webos-galaxy-launcher](https://github.com/guntiss/webos-galaxy-launcher) | Reference for installing an `.ipk` over SSH (`tools/tv.sh install`) | not stated |
| [SDL](https://github.com/libsdl-org/SDL) | Window, GL context and input; loaded at run time | zlib |
| [libloading](https://github.com/nagisa/rust_libloading) | Loading SDL2 at run time; `lumen-ctl` and `lumend` load libluna-service2 the same way | ISC |
| [png](https://github.com/image-rs/image-png) | Decoding app icons (`shell/.../icons.rs`) | MIT or Apache-2.0 |
| [fontdue](https://github.com/mooman219/fontdue) | Rasterising text (`shell/.../text.rs`) | MIT, Apache-2.0 or Zlib |
| [serde_json](https://github.com/serde-rs/json) | Reading the TV's app and input lists from luna (`shell/.../luna.rs`); `lumen-ctl`'s JSON lines and luna answers; `lumend`'s luna calls, replies and the TV's permission files | MIT or Apache-2.0 |
| [libc](https://github.com/rust-lang/libc) | The C calls `std` lacks in Lumen Guard (the DSCP mark, `poll`, SIGTERM, the local date) and in `lumen-ctl` (signals, `_exit`, `getauxval`); in `lumend`, signals, `mount`, `kill`, inotify, `SO_PEERCRED` and the link trial's uid change | MIT or Apache-2.0 |
| [Inter](https://github.com/rsms/inter) 4.1 | The shell's typeface: `shell/assets/fonts/Inter-SemiBold.ttf`, unmodified, built into the binary, with its licence beside it (`Inter-LICENSE.txt`); its kerning, read out of the font's GPOS table into `Inter-SemiBold.kern` by `tools/font/kern_table.py` and built in too | SIL Open Font License 1.1 |
| [Inter](https://github.com/rsms/inter), through [Fontsource](https://github.com/fontsource/font-files) | The typeface of the Remote web front-end, bundled in `remote/ui` (variable, Latin subset) | OFL-1.1 |
| [Tauri](https://github.com/tauri-apps/tauri) (`tauri`, `tauri-build`) | Lumen Remote for Windows (`remote/app`): the tray icon, its menu, the popup and the window around `remote/ui`, in Microsoft's WebView2, which Windows 11 ships and the app doesn't bundle | MIT or Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | `remote/app`'s command arguments and the messages it sends the page | MIT or Apache-2.0 |

Trademarks (LG, webOS, Apple, tvOS, Netflix, Prime Video, Plex) belong to their
owners. "Inspired by tvOS" describes a design direction only: Lumen ships no
Apple fonts, icons, artwork or video.
