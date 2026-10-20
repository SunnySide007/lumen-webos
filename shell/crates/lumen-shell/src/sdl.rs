//! SDL2, loaded at run time.
//!
//! The shell does not link against SDL. The TV ships its own SDL2 (LG's, or a
//! copy bundled in the app's `lib/`), and the PC simulator uses an official
//! release, so the library is opened with `dlopen`/`LoadLibrary` and only the
//! handful of functions present since SDL 2.0.1 are resolved. That keeps the
//! cross-compile free of TV libraries and the binary tolerant of old SDLs.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::time::Duration;

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
const EVENT_APP_WILL_ENTER_BACKGROUND: u32 = 0x103;
const EVENT_APP_DID_ENTER_BACKGROUND: u32 = 0x104;
const EVENT_APP_DID_ENTER_FOREGROUND: u32 = 0x106;
const EVENT_WINDOW: u32 = 0x200;
const EVENT_KEYDOWN: u32 = 0x300;
const EVENT_KEYUP: u32 = 0x301;

/// `SDL_WindowEventID` values the shell reacts to.
pub mod window {
    pub const SHOWN: u8 = 1;
    pub const HIDDEN: u8 = 2;
    pub const EXPOSED: u8 = 3;
    pub const RESIZED: u8 = 5;
    pub const SIZE_CHANGED: u8 = 6;
    pub const MINIMIZED: u8 = 7;
    pub const RESTORED: u8 = 9;
    pub const FOCUS_GAINED: u8 = 12;
    pub const FOCUS_LOST: u8 = 13;
}

/// Raw `SDL_Event`: a 56-byte union in upstream SDL2. LG's SDL lays some
/// events out differently, so the buffer has twice that, to spare.
#[repr(C, align(8))]
pub struct RawEvent(pub [u8; 128]);

impl RawEvent {
    pub const fn zeroed() -> Self {
        Self([0; 128])
    }
}

/// Where `SDL_KeyboardEvent` keeps its fields.
///
/// Upstream SDL2: type, timestamp, windowID (u32 each), then state, repeat,
/// two bytes of padding, and `SDL_Keysym { scancode, sym, mod, unused }`. LG's
/// SDL on the B9 (2.0.5, `/usr/lib/libSDL2-2.0.so.0`) has an unidentified u32
/// at bytes 12..16, which moves everything after it 4 bytes later. Measured on
/// an OLED55B9PUA, webOS 4.10.2-31, firmware 05.50.00.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLayout {
    Upstream,
    Lg,
}

impl KeyLayout {
    /// Offset of `state`; `repeat` follows it and the keysym starts 4 later.
    const fn state_offset(self) -> usize {
        match self {
            Self::Upstream => 12,
            Self::Lg => 16,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "upstream" => Some(Self::Upstream),
            "lg" => Some(Self::Lg),
            _ => None,
        }
    }
}

/// Where the SDL2 library was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibOrigin {
    /// `LUMEN_SDL2`.
    Explicit,
    /// A copy beside the executable.
    Bundled,
    /// The system's, by its bare name.
    System,
}

/// LG's layout for the system SDL on the TV, upstream's for anything else.
/// `LUMEN_SDL_KEY_LAYOUT=upstream|lg` overrides the choice.
pub fn choose_layout(origin: LibOrigin, on_tv: bool, choice: Option<&str>) -> KeyLayout {
    if let Some(layout) = choice.and_then(KeyLayout::from_name) {
        return layout;
    }
    if on_tv && origin == LibOrigin::System {
        KeyLayout::Lg
    } else {
        KeyLayout::Upstream
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
    KeyUp {
        scancode: i32,
    },
    /// `SDL_WINDOWEVENT`, with its `SDL_WindowEventID`.
    Window(u8),
    /// The app is going to, or has gone to, the background.
    Background,
    /// The app is back in front.
    Foreground,
    Other(u32),
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Decodes the parts of an `SDL_Event` the shell uses. Window events are read
/// at upstream's offsets (the `event` byte at 12) until a TV log shows LG
/// moved those too; app-lifecycle events carry nothing beyond their type.
pub fn parse_event(raw: &RawEvent, layout: KeyLayout) -> Event {
    let bytes = &raw.0;
    let state = layout.state_offset();
    match read_u32(bytes, 0) {
        EVENT_QUIT => Event::Quit,
        EVENT_KEYDOWN => Event::KeyDown {
            scancode: read_u32(bytes, state + 4) as i32,
            keycode: read_u32(bytes, state + 8) as i32,
            repeat: bytes[state + 1] != 0,
        },
        EVENT_KEYUP => Event::KeyUp {
            scancode: read_u32(bytes, state + 4) as i32,
        },
        EVENT_WINDOW => Event::Window(window_id(bytes, layout)),
        EVENT_APP_WILL_ENTER_BACKGROUND | EVENT_APP_DID_ENTER_BACKGROUND => Event::Background,
        EVENT_APP_DID_ENTER_FOREGROUND => Event::Foreground,
        other => Event::Other(other),
    }
}

/// A window event's `SDL_WindowEventID`, at byte 12 upstream. Under LG's
/// layout byte 12 might instead be the low byte of LG's extra field; 0 is
/// never a real ID (`SDL_WINDOWEVENT_NONE`), so then byte 16, where a 4-byte
/// shift would put it, is read instead.
fn window_id(bytes: &[u8], layout: KeyLayout) -> u8 {
    match (layout, bytes[12]) {
        (KeyLayout::Lg, 0) => bytes[16],
        (_, id) => id,
    }
}

/// A key event's `state` byte: 1 (pressed) on a key-down read at the right
/// offsets.
fn key_state(raw: &RawEvent, layout: KeyLayout) -> u8 {
    raw.0[layout.state_offset()]
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(if i % 4 == 0 { '|' } else { ' ' });
        }
        out.push_str(&format!("{b:02x}"));
    }
    out
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

/// Which raw events go into the log, so a TV run shows what LG's SDL sends
/// without flooding the log: the first presses and releases (not SDL's
/// repeats), and the first event of each other kind, where a window event's
/// kind includes its ID bytes.
const RAW_KEY_EVENTS: u32 = 8;
const RAW_OTHER_KINDS: usize = 24;
/// Bytes of each raw event logged: enough for any key or window event.
const RAW_BYTES: usize = 48;

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
    wait_event_timeout: unsafe extern "C" fn(*mut RawEvent, c_int) -> c_int,
    key_layout: KeyLayout,
    raw_keys_logged: Cell<u32>,
    raw_kinds_logged: RefCell<Vec<u64>>,
    layout_warned: Cell<bool>,
    /// Held so every function pointer above stays valid; dropped last.
    _lib: Library,
}

/// Where to look for SDL2, in order: an explicit `LUMEN_SDL2`, a copy beside
/// the executable (an app-bundled build on the TV), then the system's.
fn candidates() -> Vec<(PathBuf, LibOrigin)> {
    let mut paths = Vec::new();
    if let Some(explicit) = std::env::var_os("LUMEN_SDL2") {
        paths.push((PathBuf::from(explicit), LibOrigin::Explicit));
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
            paths.push((dir.join("lib").join(name), LibOrigin::Bundled));
            paths.push((dir.join(name), LibOrigin::Bundled));
        }
    }
    paths.extend(names.iter().map(|n| (PathBuf::from(n), LibOrigin::System)));
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
        for (path, origin) in candidates() {
            // SAFETY: loading SDL2 runs only its own initialisers, which have
            // no preconditions.
            match unsafe { Library::new(&path) } {
                Ok(lib) => {
                    let choice = std::env::var("LUMEN_SDL_KEY_LAYOUT").ok();
                    if let Some(bad) = choice
                        .as_deref()
                        .filter(|c| KeyLayout::from_name(c).is_none())
                    {
                        crate::info!("ignoring LUMEN_SDL_KEY_LAYOUT={bad:?}: use upstream or lg");
                    }
                    let layout = choose_layout(origin, crate::app::on_tv(), choice.as_deref());
                    crate::info!(
                        "SDL2 loaded from {} ({origin:?}); key events in the {layout:?} layout",
                        path.display()
                    );
                    return Self::bind(lib, layout);
                }
                Err(e) => tried.push(format!("{}: {e}", path.display())),
            }
        }
        Err(format!("SDL2 not found; tried:\n  {}", tried.join("\n  ")))
    }

    fn bind(lib: Library, key_layout: KeyLayout) -> Result<Self, String> {
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
                wait_event_timeout: sym(&lib, b"SDL_WaitEventTimeout\0")?,
                key_layout,
                raw_keys_logged: Cell::new(0),
                raw_kinds_logged: RefCell::new(Vec::new()),
                layout_warned: Cell::new(false),
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
        // SAFETY: `raw` is large and aligned enough for any SDL2 event, with
        // room to spare for LG's layout.
        (unsafe { (self.poll_event)(&mut raw) } != 0).then(|| self.decode(&raw))
    }

    /// Waits for an event for up to `timeout`, or for as long as it takes with
    /// `None`. SDL 2.0.5 (the B9's) waits by checking every 10 ms; newer SDLs
    /// sleep until an event arrives.
    pub fn wait_event(&self, timeout: Option<Duration>) -> Option<Event> {
        let ms = timeout.map_or(-1, |t| {
            // Rounded up to whole milliseconds. SDL may still wake a little
            // early (it counts whole ticks) or, on 2.0.5, up to 10 ms late;
            // either way the loop just looks at its deadlines again.
            let ms = t.as_micros().div_ceil(1000);
            c_int::try_from(ms).unwrap_or(c_int::MAX)
        });
        let mut raw = RawEvent::zeroed();
        // SAFETY: as in `poll_event`; any timeout value is valid.
        (unsafe { (self.wait_event_timeout)(&mut raw, ms) } != 0).then(|| self.decode(&raw))
    }

    /// Parses `raw`, logging the first events of each kind byte for byte and
    /// warning once if key events look misread.
    fn decode(&self, raw: &RawEvent) -> Event {
        let event = parse_event(raw, self.key_layout);
        let ty = read_u32(&raw.0, 0);
        let log = match event {
            Event::KeyDown { repeat: true, .. } => false,
            Event::KeyDown { .. } | Event::KeyUp { .. } => {
                let n = self.raw_keys_logged.get();
                self.raw_keys_logged.set(n.saturating_add(1));
                n < RAW_KEY_EVENTS
            }
            _ => {
                let mut kind = u64::from(ty) << 16;
                if ty == EVENT_WINDOW {
                    kind |= u64::from(raw.0[12]) << 8 | u64::from(raw.0[16]);
                }
                let mut seen = self.raw_kinds_logged.borrow_mut();
                let new = seen.len() < RAW_OTHER_KINDS && !seen.contains(&kind);
                if new {
                    seen.push(kind);
                }
                new
            }
        };
        if log {
            crate::info!("raw event {ty:#x}: {}", hex(&raw.0[..RAW_BYTES]));
        }
        if matches!(event, Event::KeyDown { .. })
            && key_state(raw, self.key_layout) != 1
            && !self.layout_warned.replace(true)
        {
            crate::info!(
                "a key-down's state byte reads {} in the {:?} layout, not 1: key events are \
                 probably misread; try LUMEN_SDL_KEY_LAYOUT",
                key_state(raw, self.key_layout),
                self.key_layout
            );
        }
        event
    }

    pub fn quit(&self) {
        // SAFETY: SDL_Quit is safe to call after (or without) SDL_Init.
        unsafe { (self.quit)() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_with(event_type: u32, fill: impl FnOnce(&mut [u8; 128])) -> RawEvent {
        let mut raw = RawEvent::zeroed();
        raw.0[..4].copy_from_slice(&event_type.to_ne_bytes());
        fill(&mut raw.0);
        raw
    }

    /// A key event as LG's SDL on the B9 sends it: an unknown u32 at 12, state
    /// at 16, repeat at 17, scancode at 20, keycode at 24.
    fn lg_key(event_type: u32, state: u8, repeat: u8, scancode: i32) -> RawEvent {
        raw_with(event_type, |b| {
            b[12..16].copy_from_slice(&0x0000_00ABu32.to_ne_bytes());
            b[16] = state;
            b[17] = repeat;
            b[20..24].copy_from_slice(&scancode.to_ne_bytes());
            b[24..28].copy_from_slice(&0x4000_01E2_i32.to_ne_bytes());
        })
    }

    #[test]
    fn parses_quit() {
        let raw = raw_with(EVENT_QUIT, |_| {});
        assert_eq!(parse_event(&raw, KeyLayout::Upstream), Event::Quit);
        assert_eq!(parse_event(&raw, KeyLayout::Lg), Event::Quit);
    }

    #[test]
    fn parses_key_down_at_upstream_offsets() {
        let raw = raw_with(EVENT_KEYDOWN, |b| {
            b[12] = 1;
            b[13] = 1;
            b[16..20].copy_from_slice(&482i32.to_ne_bytes());
            b[20..24].copy_from_slice(&0x4000_01E2_i32.to_ne_bytes());
        });
        assert_eq!(
            parse_event(&raw, KeyLayout::Upstream),
            Event::KeyDown {
                scancode: 482,
                keycode: 0x4000_01E2,
                repeat: true
            }
        );
        assert_eq!(key_state(&raw, KeyLayout::Upstream), 1);
    }

    #[test]
    fn parses_the_b9s_key_events_at_lgs_offsets() {
        let down = lg_key(EVENT_KEYDOWN, 1, 0, 482);
        assert_eq!(
            parse_event(&down, KeyLayout::Lg),
            Event::KeyDown {
                scancode: 482,
                keycode: 0x4000_01E2,
                repeat: false
            }
        );
        assert_eq!(key_state(&down, KeyLayout::Lg), 1);
        let held = lg_key(EVENT_KEYDOWN, 1, 1, 79);
        assert!(matches!(
            parse_event(&held, KeyLayout::Lg),
            Event::KeyDown {
                scancode: 79,
                repeat: true,
                ..
            }
        ));
        let up = lg_key(EVENT_KEYUP, 0, 0, 40);
        assert_eq!(
            parse_event(&up, KeyLayout::Lg),
            Event::KeyUp { scancode: 40 }
        );
    }

    #[test]
    fn upstream_offsets_misread_the_b9_as_hello_gles_did() {
        // hello-gles logged "scancode 1" for every key and "scancode 257" for
        // a repeat: LG's state and repeat bytes, read as the scancode.
        let down = lg_key(EVENT_KEYDOWN, 1, 0, 482);
        let held = lg_key(EVENT_KEYDOWN, 1, 1, 482);
        for (raw, wrong) in [(&down, 1), (&held, 257)] {
            match parse_event(raw, KeyLayout::Upstream) {
                Event::KeyDown { scancode, .. } => assert_eq!(scancode, wrong),
                other => panic!("not a key-down: {other:?}"),
            }
        }
        // Read that way, the state byte is the unknown field's low byte, which
        // is how the shell notices the mismatch and says so in the log.
        assert_ne!(key_state(&down, KeyLayout::Upstream), 1);
    }

    #[test]
    fn parses_window_and_lifecycle_events() {
        let hidden = raw_with(EVENT_WINDOW, |b| b[12] = window::HIDDEN);
        assert_eq!(
            parse_event(&hidden, KeyLayout::Lg),
            Event::Window(window::HIDDEN)
        );
        for (ty, want) in [
            (EVENT_APP_WILL_ENTER_BACKGROUND, Event::Background),
            (EVENT_APP_DID_ENTER_BACKGROUND, Event::Background),
            (EVENT_APP_DID_ENTER_FOREGROUND, Event::Foreground),
        ] {
            assert_eq!(parse_event(&raw_with(ty, |_| {}), KeyLayout::Lg), want);
        }
    }

    #[test]
    fn lgs_layout_finds_a_shifted_window_event_id() {
        // Shifted like LG's key events: the ID at 16, a zero at 12.
        let shifted = raw_with(EVENT_WINDOW, |b| b[16] = window::FOCUS_LOST);
        assert_eq!(
            parse_event(&shifted, KeyLayout::Lg),
            Event::Window(window::FOCUS_LOST)
        );
        assert_eq!(parse_event(&shifted, KeyLayout::Upstream), Event::Window(0));
    }

    #[test]
    fn unknown_events_pass_through() {
        let raw = raw_with(0x400, |_| {});
        assert_eq!(parse_event(&raw, KeyLayout::Upstream), Event::Other(0x400));
    }

    #[test]
    fn lg_layout_only_for_the_tvs_system_sdl_unless_overridden() {
        use LibOrigin::*;
        assert_eq!(choose_layout(System, true, None), KeyLayout::Lg);
        assert_eq!(choose_layout(Bundled, true, None), KeyLayout::Upstream);
        assert_eq!(choose_layout(Explicit, true, None), KeyLayout::Upstream);
        assert_eq!(choose_layout(System, false, None), KeyLayout::Upstream);
        assert_eq!(
            choose_layout(System, true, Some("upstream")),
            KeyLayout::Upstream
        );
        assert_eq!(choose_layout(Bundled, false, Some(" LG ")), KeyLayout::Lg);
        assert_eq!(choose_layout(System, true, Some("nonsense")), KeyLayout::Lg);
    }

    #[test]
    fn hex_groups_bytes_in_fours() {
        assert_eq!(hex(&[0, 1, 2, 3, 0xab, 0xff]), "00 01 02 03|ab ff");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn raw_event_has_room_beyond_sdl_event() {
        assert!(std::mem::size_of::<RawEvent>() >= 2 * 56);
        assert_eq!(std::mem::align_of::<RawEvent>(), 8);
    }
}
