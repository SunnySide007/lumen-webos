//! The shell's own platform layer (decision 15; docs/specs/depth.md, item
//! 17): its window, GL context, events and lifecycle without LG's SDL.
//!
//! * `wl`: the TV's `libwayland-client` and LG's webOS extensions, loaded at
//!   run time, with every request and event found by name in the TV's own
//!   interface tables, so LG's opcodes are mapped at run time.
//! * `egl`: the TV's EGL and GLES libraries, loaded at run time; an ES 2 or
//!   ES 3 context on the Wayland surface.
//! * `wayland` (Linux only): the window, the remote's keys and pointer, and
//!   the compositor's states, as the home loop's events and its screen's
//!   window.
//! * `lifecycle`: the shell's own registration with the app manager, so a
//!   relaunch (a Home press with Lumen in front) reaches the shell.
//! * `launch`: what the launch arguments say: the platform, the lifecycle
//!   interface SAM chose, a hidden start, the trials.
//! * `keys`: the remote's evdev codes as the SDL scancodes the core reads,
//!   and which clock the compositor stamps them on.
//! * `probe`: the GPU plan's gate, measured in an ES 3 context.
//!
//! SDL stays the default (`Platform::Sdl`). The Wayland path is chosen per
//! run with `{"platform":"wayland"}` in the launch parameters, `--platform
//! wayland`, `LUMEN_PLATFORM=wayland` or a `platform` file beside the
//! executable (`make ipk PLATFORM=wayland`), and falls back to SDL if the
//! compositor or a library is missing. It is the TV's, built on Linux only.

// The Wayland path is the TV's, so its parts build on Linux only (the
// container's checks and tests run there); elsewhere the switch logs that it
// is missing and SDL runs.
#[cfg(target_os = "linux")]
pub mod egl;
#[cfg(target_os = "linux")]
pub mod keys;
pub mod launch;
// Its thread runs beside the Wayland path only; elsewhere `Method` alone is
// read, from the launch argument.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod lifecycle;
pub mod probe;
#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(target_os = "linux")]
pub mod wl;

/// Which window and event layer the shell runs on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Platform {
    /// LG's SDL 2.0.5 on the TV, the official SDL2 on a PC.
    #[default]
    Sdl,
    /// The shell's own: libwayland-client, EGL and its own registration.
    Wayland,
}

impl Platform {
    /// `sdl` or `wayland`, any case, spaces around ignored.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "sdl" => Some(Self::Sdl),
            "wayland" => Some(Self::Wayland),
            _ => None,
        }
    }

    /// Its name in the log and the `start:` line's first phase.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sdl => "sdl",
            Self::Wayland => "wayland",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_are_named_either_way() {
        assert_eq!(Platform::from_name("wayland"), Some(Platform::Wayland));
        assert_eq!(Platform::from_name(" SDL "), Some(Platform::Sdl));
        assert_eq!(Platform::from_name("x11"), None);
        assert_eq!(Platform::default(), Platform::Sdl);
        for p in [Platform::Sdl, Platform::Wayland] {
            assert_eq!(Platform::from_name(p.name()), Some(p));
        }
    }
}
