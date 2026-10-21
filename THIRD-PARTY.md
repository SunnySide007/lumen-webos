# Third-party work

Lumen builds on, and learns from, these projects. Code copied or adapted from
any of them is marked at the point of use with its origin and license.

| Project | Used for | License |
|---|---|---|
| [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel) | Root access, boot hooks (`/var/lib/webosbrew/init.d`), SSH, failsafe mode | MIT |
| [Glasshouse](https://github.com/rorygallagher2024/glasshouse) | The privacy and debloat layer, installed as-is. Its research on webOS 4 key filters, home takeover and blocklists informs `takeover/` and `profiles/` | MIT |
| [plx-native](https://github.com/GLinnik21/plx-native) | The Plex client Lumen pins. Its measured GPU budgets, Liquid Glass notes and build approach inform `shell/`. The `getauxval` shim in `shell/.../compat.rs` follows its `src/compat/getauxval.c`. The PlxNative name and brand are excluded from its license (see its TRADEMARKS.md) and are not used here | GPL-3.0-or-later |
| [webosbrew/custom-screensaver](https://github.com/webosbrew/custom-screensaver) and [aabytt/custom-screensaver-aerial](https://github.com/aabytt/custom-screensaver-aerial) | The QML screensaver bind mount that `ambient/` will build on | MIT |
| [webosbrew/SDL-webOS](https://github.com/webosbrew/SDL-webOS) | The webOS remote's scancodes (`SDL_SCANCODE_WEBOS_*`) and SDL hints | zlib |
| [openlgtv/buildroot-nc4](https://github.com/openlgtv/buildroot-nc4) | The cross-compiling toolchain in `docker/Dockerfile` | GPL-2.0 (toolchain) |
| [webos-galaxy-launcher](https://github.com/guntiss/webos-galaxy-launcher) | Reference for installing an `.ipk` over SSH (`tools/tv.sh install`) | not stated |
| [SDL](https://github.com/libsdl-org/SDL) | Window, GL context and input; loaded at run time | zlib |
| [libloading](https://github.com/nagisa/rust_libloading) | Loading SDL2 at run time | ISC |
| [png](https://github.com/image-rs/image-png) | Decoding app icons (`shell/.../icons.rs`) | MIT or Apache-2.0 |
| [fontdue](https://github.com/mooman219/fontdue) | Rasterising text (`shell/.../text.rs`) | MIT, Apache-2.0 or Zlib |
| [Inter](https://github.com/rsms/inter) 4.1 | The shell's typeface: `shell/assets/fonts/Inter-SemiBold.ttf`, unmodified, built into the binary, with its licence beside it (`Inter-LICENSE.txt`) | SIL Open Font License 1.1 |

Trademarks (LG, webOS, Apple, tvOS, Netflix, Prime Video, Plex) belong to their
owners. "Inspired by tvOS" describes a design direction only: Lumen ships no
Apple fonts, icons, artwork or video.
