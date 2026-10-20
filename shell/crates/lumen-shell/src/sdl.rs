//! SDL2, loaded at run time.
//!
//! The shell does not link against SDL. The TV ships its own SDL2 (LG's, or a
//! copy bundled in the app's `lib/`), and the PC simulator uses an official
//! release, so the library is opened with `dlopen`/`LoadLibrary` and only the
//! handful of functions present since SDL 2.0.1 are resolved. That keeps the
//! cross-compile free of TV libraries and the binary tolerant of old SDLs.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;

use libloading::Library;

pub const INIT_VIDEO: u32 = 0x0000_0020;
pub const INIT_EVENTS: u32 = 0x0000_4000;

pub const WINDOW_FULLSCREEN: u32 = 0x0000_0001;
pub const WINDOW_OPENGL: u32 = 0x0000_0002;
pub const WINDOW_SHOWN: u32 = 0x0000_0004;
pub const WINDOW_RESIZABLE: u32 = 0x0000_0020;
pub const WINDOWPOS_CENTERED: c_int = 0x2FFF_0000;

// SDL_GLattr values.
pub const GL_RED_SIZE: c_int = 0;
pub const GL_GREEN_SIZE: c_int = 1;
pub const GL_BLUE_SIZE: c_int = 2;
pub const GL_DOUBLEBUFFER: c_int = 5;
pub const GL_CONTEXT_MAJOR_VERSION: c_int = 17;
pub const GL_CONTEXT_MINOR_VERSION: c_int = 18;
pub const GL_CONTEXT_PROFILE_MASK: c_int = 21;
pub const GL_CONTEXT_PROFILE_ES: c_int = 0x0004;

const EVENT_QUIT: u32 = 0x100;
const EVENT_KEYDOWN: u32 = 0x300;

/// Scancodes the shell cares about. The webOS ones come from webosbrew's SDL
/// fork (`SDL_SCANCODE_WEBOS_*`), which matches what LG's SDL reports.
pub mod scancode {
    pub const ESCAPE: i32 = 41;
    pub const AC_BACK: i32 = 270;
    pub const WEBOS_HOME: i32 = 384;
    pub const WEBOS_BACK: i32 = 482;
}

/// Raw `SDL_Event`: a 56-byte union in SDL2, given room to spare.
#[repr(C, align(8))]
pub struct RawEvent(pub [u8; 64]);

impl RawEvent {
    pub const fn zeroed() -> Self {
        Self([0; 64])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Quit,
    KeyDown {
        scancode: i32,
        keycode: i32,
        repeat: bool,
    },
    Other(u32),
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Decodes the parts of an `SDL_Event` the shell uses. Offsets follow SDL2's
/// `SDL_KeyboardEvent`: type, timestamp, windowID (u32 each), state, repeat,
/// two bytes of padding, then `SDL_Keysym { scancode, sym, mod, unused }`.
pub fn parse_event(raw: &RawEvent) -> Event {
    let bytes = &raw.0;
    match read_u32(bytes, 0) {
        EVENT_QUIT => Event::Quit,
        EVENT_KEYDOWN => Event::KeyDown {
            scancode: read_u32(bytes, 16) as i32,
            keycode: read_u32(bytes, 20) as i32,
            repeat: bytes[13] != 0,
        },
        other => Event::Other(other),
    }
}

/// True for the keys that should leave a full-screen app: the remote's BACK
/// (webOS or Android-style) and Escape on a PC keyboard.
pub fn is_back(scancode: i32) -> bool {
    matches!(
        scancode,
        scancode::WEBOS_BACK | scancode::AC_BACK | scancode::ESCAPE
    )
}

/// A readable name for the keys the shell knows, for the overlay and the log.
pub fn key_name(scancode: i32) -> Option<&'static str> {
    match scancode {
        scancode::WEBOS_BACK | scancode::AC_BACK => Some("BACK"),
        scancode::WEBOS_HOME => Some("HOME"),
        scancode::ESCAPE => Some("ESC"),
        _ => None,
    }
}

#[repr(C)]
#[derive(Default)]
struct Version {
    major: u8,
    minor: u8,
    patch: u8,
}

pub type Window = *mut c_void;
pub type GlContext = *mut c_void;

pub struct Sdl {
    init: unsafe extern "C" fn(u32) -> c_int,
    quit: unsafe extern "C" fn(),
    set_hint: unsafe extern "C" fn(*const c_char, *const c_char) -> c_int,
    get_error: unsafe extern "C" fn() -> *const c_char,
    get_version: unsafe extern "C" fn(*mut Version),
    create_window: unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> Window,
    destroy_window: unsafe extern "C" fn(Window),
    get_window_size: unsafe extern "C" fn(Window, *mut c_int, *mut c_int),
    gl_set_attribute: unsafe extern "C" fn(c_int, c_int) -> c_int,
    gl_create_context: unsafe extern "C" fn(Window) -> GlContext,
    gl_delete_context: unsafe extern "C" fn(GlContext),
    gl_make_current: unsafe extern "C" fn(Window, GlContext) -> c_int,
    gl_set_swap_interval: unsafe extern "C" fn(c_int) -> c_int,
    gl_get_proc_address: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    gl_swap_window: unsafe extern "C" fn(Window),
    gl_get_drawable_size: Option<unsafe extern "C" fn(Window, *mut c_int, *mut c_int)>,
    poll_event: unsafe extern "C" fn(*mut RawEvent) -> c_int,
    /// Held so every function pointer above stays valid; dropped last.
    _lib: Library,
}

/// Where to look for SDL2, in order: an explicit `LUMEN_SDL2`, a copy beside
/// the executable (an app-bundled build on the TV), then the system's.
fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("LUMEN_SDL2") {
        paths.push(PathBuf::from(explicit));
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let names: &[&str] = if cfg!(windows) {
        &["SDL2.dll"]
    } else if cfg!(target_os = "macos") {
        &["libSDL2-2.0.0.dylib", "libSDL2.dylib"]
    } else {
        &["libSDL2-2.0.so.0", "libSDL2.so"]
    };
    if let Some(dir) = &exe_dir {
        for name in names {
            paths.push(dir.join("lib").join(name));
            paths.push(dir.join(name));
        }
    }
    paths.extend(names.iter().map(PathBuf::from));
    paths
}

/// Copies a symbol out of `lib` as a function pointer.
///
/// # Safety
/// `T` must be the exact `extern "C"` function-pointer type of `name`.
unsafe fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: forwarded from the caller: T matches the symbol's real type.
    unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| {
        let shown = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
        format!("SDL2 has no {shown}: {e}")
    })
}

impl Sdl {
    pub fn load() -> Result<Self, String> {
        let mut tried = Vec::new();
        for path in candidates() {
            // SAFETY: loading SDL2 runs only its own initialisers, which have
            // no preconditions.
            match unsafe { Library::new(&path) } {
                Ok(lib) => {
                    crate::info!("SDL2 loaded from {}", path.display());
                    return Self::bind(lib);
                }
                Err(e) => tried.push(format!("{}: {e}", path.display())),
            }
        }
        Err(format!("SDL2 not found; tried:\n  {}", tried.join("\n  ")))
    }

    fn bind(lib: Library) -> Result<Self, String> {
        // SAFETY: each type below is the SDL2 2.0.x C prototype of the named
        // function, as declared in SDL.h.
        unsafe {
            Ok(Self {
                init: sym(&lib, b"SDL_Init\0")?,
                quit: sym(&lib, b"SDL_Quit\0")?,
                set_hint: sym(&lib, b"SDL_SetHint\0")?,
                get_error: sym(&lib, b"SDL_GetError\0")?,
                get_version: sym(&lib, b"SDL_GetVersion\0")?,
                create_window: sym(&lib, b"SDL_CreateWindow\0")?,
                destroy_window: sym(&lib, b"SDL_DestroyWindow\0")?,
                get_window_size: sym(&lib, b"SDL_GetWindowSize\0")?,
                gl_set_attribute: sym(&lib, b"SDL_GL_SetAttribute\0")?,
                gl_create_context: sym(&lib, b"SDL_GL_CreateContext\0")?,
                gl_delete_context: sym(&lib, b"SDL_GL_DeleteContext\0")?,
                gl_make_current: sym(&lib, b"SDL_GL_MakeCurrent\0")?,
                gl_set_swap_interval: sym(&lib, b"SDL_GL_SetSwapInterval\0")?,
                gl_get_proc_address: sym(&lib, b"SDL_GL_GetProcAddress\0")?,
                gl_swap_window: sym(&lib, b"SDL_GL_SwapWindow\0")?,
                gl_get_drawable_size: sym(&lib, b"SDL_GL_GetDrawableSize\0").ok(),
                poll_event: sym(&lib, b"SDL_PollEvent\0")?,
                _lib: lib,
            })
        }
    }

    pub fn error(&self) -> String {
        // SAFETY: SDL_GetError always returns a valid, NUL-terminated string.
        unsafe { CStr::from_ptr((self.get_error)()) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn version(&self) -> String {
        let mut v = Version::default();
        // SAFETY: `v` is a valid SDL_version for SDL to fill in.
        unsafe { (self.get_version)(&mut v) };
        format!("{}.{}.{}", v.major, v.minor, v.patch)
    }

    /// Sets a hint. Hints an SDL build does not know are stored and ignored,
    /// so webOS-only hints are harmless on the PC.
    pub fn hint(&self, name: &str, value: &str) {
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        // SAFETY: both arguments are valid C strings for the duration of the call.
        unsafe { (self.set_hint)(name.as_ptr(), value.as_ptr()) };
    }

    pub fn init(&self, flags: u32) -> Result<(), String> {
        // SAFETY: SDL_Init may be called with any subsystem flags.
        if unsafe { (self.init)(flags) } != 0 {
            return Err(format!("SDL_Init: {}", self.error()));
        }
        Ok(())
    }

    pub fn set_gl_attribute(&self, attr: c_int, value: c_int) {
        // SAFETY: plain value setter; SDL validates `attr`.
        unsafe { (self.gl_set_attribute)(attr, value) };
    }

    pub fn create_window(
        &self,
        title: &str,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> Result<Window, String> {
        let title = CString::new(title).map_err(|e| e.to_string())?;
        // SAFETY: the title is a valid C string; SDL copies it.
        let window = unsafe {
            (self.create_window)(
                title.as_ptr(),
                WINDOWPOS_CENTERED,
                WINDOWPOS_CENTERED,
                w,
                h,
                flags,
            )
        };
        if window.is_null() {
            return Err(format!("SDL_CreateWindow: {}", self.error()));
        }
        Ok(window)
    }

    /// # Safety
    /// `window` must come from `create_window` and not be destroyed yet.
    pub unsafe fn destroy_window(&self, window: Window) {
        // SAFETY: forwarded from the caller.
        unsafe { (self.destroy_window)(window) };
    }

    /// # Safety
    /// `window` must be a live window created with `WINDOW_OPENGL`.
    pub unsafe fn gl_create_context(&self, window: Window) -> Result<GlContext, String> {
        // SAFETY: forwarded from the caller.
        let ctx = unsafe { (self.gl_create_context)(window) };
        if ctx.is_null() {
            return Err(format!("SDL_GL_CreateContext: {}", self.error()));
        }
        // SAFETY: both handles are live; the context was just made for this window.
        if unsafe { (self.gl_make_current)(window, ctx) } != 0 {
            // SAFETY: the context is live and owned here.
            unsafe { (self.gl_delete_context)(ctx) };
            return Err(format!("SDL_GL_MakeCurrent: {}", self.error()));
        }
        Ok(ctx)
    }

    /// # Safety
    /// `ctx` must come from `gl_create_context` and not be deleted yet.
    pub unsafe fn gl_delete_context(&self, ctx: GlContext) {
        // SAFETY: forwarded from the caller.
        unsafe { (self.gl_delete_context)(ctx) };
    }

    /// Asks for vsync. Returns false when the platform refuses it.
    pub fn gl_set_swap_interval(&self, interval: c_int) -> bool {
        // SAFETY: valid with a current context, which callers have; otherwise
        // SDL returns an error rather than misbehaving.
        unsafe { (self.gl_set_swap_interval)(interval) == 0 }
    }

    pub fn gl_proc_address(&self, name: &CStr) -> *const c_void {
        // SAFETY: `name` is a valid C string; a missing function yields null.
        unsafe { (self.gl_get_proc_address)(name.as_ptr()) }.cast_const()
    }

    /// # Safety
    /// `window` must be live and have the current GL context.
    pub unsafe fn gl_swap_window(&self, window: Window) {
        // SAFETY: forwarded from the caller.
        unsafe { (self.gl_swap_window)(window) };
    }

    /// Size of the GL drawable in pixels, falling back to the window size on
    /// SDL builds older than 2.0.1.
    ///
    /// # Safety
    /// `window` must be live.
    pub unsafe fn drawable_size(&self, window: Window) -> (i32, i32) {
        let (mut w, mut h) = (0, 0);
        let query = self.gl_get_drawable_size.unwrap_or(self.get_window_size);
        // SAFETY: the window is live (caller) and both out-pointers are valid.
        unsafe { query(window, &mut w, &mut h) };
        (w, h)
    }

    pub fn poll_event(&self) -> Option<Event> {
        let mut raw = RawEvent::zeroed();
        // SAFETY: `raw` is large and aligned enough for any SDL2 event.
        (unsafe { (self.poll_event)(&mut raw) } != 0).then(|| parse_event(&raw))
    }

    pub fn quit(&self) {
        // SAFETY: SDL_Quit is safe to call after (or without) SDL_Init.
        unsafe { (self.quit)() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_with(event_type: u32, fill: impl FnOnce(&mut [u8; 64])) -> RawEvent {
        let mut raw = RawEvent::zeroed();
        raw.0[..4].copy_from_slice(&event_type.to_ne_bytes());
        fill(&mut raw.0);
        raw
    }

    #[test]
    fn parses_quit() {
        assert_eq!(parse_event(&raw_with(EVENT_QUIT, |_| {})), Event::Quit);
    }

    #[test]
    fn parses_key_down_at_sdl2_offsets() {
        let raw = raw_with(EVENT_KEYDOWN, |b| {
            b[13] = 1;
            b[16..20].copy_from_slice(&scancode::WEBOS_BACK.to_ne_bytes());
            b[20..24].copy_from_slice(&0x4000_01E2_i32.to_ne_bytes());
        });
        assert_eq!(
            parse_event(&raw),
            Event::KeyDown {
                scancode: scancode::WEBOS_BACK,
                keycode: 0x4000_01E2,
                repeat: true
            }
        );
    }

    #[test]
    fn unknown_events_pass_through() {
        assert_eq!(parse_event(&raw_with(0x200, |_| {})), Event::Other(0x200));
    }

    #[test]
    fn back_keys() {
        assert!(is_back(scancode::WEBOS_BACK));
        assert!(is_back(scancode::AC_BACK));
        assert!(is_back(scancode::ESCAPE));
        assert!(!is_back(scancode::WEBOS_HOME));
    }

    #[test]
    fn names_the_remote_keys() {
        assert_eq!(key_name(scancode::WEBOS_BACK), Some("BACK"));
        assert_eq!(key_name(scancode::WEBOS_HOME), Some("HOME"));
        assert_eq!(key_name(4), None);
    }

    #[test]
    fn raw_event_is_big_enough_for_sdl_event() {
        assert!(std::mem::size_of::<RawEvent>() >= 56);
        assert_eq!(std::mem::align_of::<RawEvent>(), 8);
    }
}
