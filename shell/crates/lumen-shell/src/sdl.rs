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
use std::time::{Duration, Instant};

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
const EVENT_MOUSEMOTION: u32 = 0x400;
const EVENT_MOUSEBUTTONDOWN: u32 = 0x401;
const EVENT_MOUSEBUTTONUP: u32 = 0x402;
const EVENT_MOUSEWHEEL: u32 = 0x403;

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
    /// A button of the Magic Remote's pointer (or a mouse) went down or up at
    /// `x`,`y`; `button` 1 is the left one, which is OK while the pointer
    /// shows (docs/specs/shell-mvp.md, "Input diagnostics").
    MouseButton {
        button: u8,
        down: bool,
        x: i32,
        y: i32,
    },
    /// The pointer moved. Where to is not used, only that it did.
    MouseMotion,
    /// The Magic Remote's wheel turned (the B9 sends 0x403 while the pointer
    /// is up). Input, nothing more.
    MouseWheel,
    Other(u32),
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Decodes the parts of an `SDL_Event` the shell uses. Window events are read
/// at upstream's offsets (the `event` byte at 12): the B9's do not move them
/// (B1 run, 2026-10-20; shell-mvp.md, "Measured on the B9", 2). App-lifecycle
/// events carry nothing beyond their type.
/// Mouse events are at upstream's offsets in both layouts: the B9's own
/// 0x400, 0x401 and 0x402 events (2026-10-22) have `which` at 12, the button
/// at 16, its state at 17 and `x`,`y` at 20 and 24, as `SDL_MouseButtonEvent`
/// declares them.
pub fn parse_event(raw: &RawEvent, layout: KeyLayout) -> Event {
    let bytes = &raw.0;
    let state = layout.state_offset();
    match read_u32(bytes, 0) {
        ty @ (EVENT_MOUSEBUTTONDOWN | EVENT_MOUSEBUTTONUP) => Event::MouseButton {
            button: bytes[16],
            down: ty == EVENT_MOUSEBUTTONDOWN,
            x: read_u32(bytes, 20) as i32,
            y: read_u32(bytes, 24) as i32,
        },
        EVENT_MOUSEMOTION => Event::MouseMotion,
        EVENT_MOUSEWHEEL => Event::MouseWheel,
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

/// A window event's `SDL_WindowEventID`, at byte 12 in both layouts, as the
/// B9 sends it. The byte-16 fallback for a 0 under LG's layout dates from
/// before that was known; it never runs, since 0 (`SDL_WINDOWEVENT_NONE`) is
/// never sent.
fn window_id(bytes: &[u8], layout: KeyLayout) -> u8 {
    match (layout, bytes[12]) {
        (KeyLayout::Lg, 0) => bytes[16],
        (_, id) => id,
    }
}

/// An event's timestamp: milliseconds on SDL's clock (`SDL_GetTicks`), at
/// byte 4 in both layouts, since LG's extra key field comes after it.
fn event_time(raw: &RawEvent) -> u32 {
    read_u32(&raw.0, 4)
}

/// An event queued longer than this is taken to be stamped on some other clock.
const MAX_EVENT_AGE_MS: u32 = 10_000;

/// How long ago SDL stamped an event. `event_ms` and `now_ms` are both read
/// from SDL's millisecond clock, which wraps after 49.7 days. `None` if the
/// stamp cannot be on that clock: never set (0), from the future (it wraps to
/// weeks), or older than `MAX_EVENT_AGE_MS`.
fn event_age(event_ms: u32, now_ms: u32) -> Option<Duration> {
    if event_ms == 0 {
        return None;
    }
    let age = now_ms.wrapping_sub(event_ms);
    (age <= MAX_EVENT_AGE_MS).then(|| Duration::from_millis(u64::from(age)))
}

/// The moment an event was stamped, given when it was `read` and SDL's clock
/// then; `None` if the stamp is not on SDL's clock (`event_age`).
fn event_instant(read: Instant, event_ms: u32, now_ms: u32) -> Option<Instant> {
    event_age(event_ms, now_ms).map(|age| read.checked_sub(age).unwrap_or(read))
}

/// The key pressed last, with times on SDL's clock.
#[derive(Debug, Clone, Copy)]
struct Held {
    scancode: i32,
    pressed: u32,
    first_repeat: u32,
    last: u32,
    repeats: u32,
    /// Whether its repeats go into the log (`REPEAT_KEYS_LOGGED`).
    logged: bool,
}

/// Times SDL's repeats of the key pressed last, for the log.
#[derive(Debug, Default)]
struct RepeatTimes {
    held: Option<Held>,
    /// Keys that have repeated so far.
    keys_repeated: u32,
}

impl RepeatTimes {
    /// Notes a key event stamped `ms` and returns the line to log, if any:
    /// each of the first `REPEATS_LOGGED` repeats of a logged key, and a
    /// summary at its release. Taps, and repeats of a key other than the one
    /// pressed last, log nothing.
    fn note(&mut self, event: Event, ms: u32) -> Option<String> {
        match event {
            Event::KeyDown {
                scancode,
                repeat: false,
                ..
            } => {
                self.held = Some(Held {
                    scancode,
                    pressed: ms,
                    first_repeat: ms,
                    last: ms,
                    repeats: 0,
                    logged: false,
                });
                None
            }
            Event::KeyDown {
                scancode,
                repeat: true,
                ..
            } => {
                let held = self.held.as_mut().filter(|h| h.scancode == scancode)?;
                if held.repeats == 0 {
                    held.first_repeat = ms;
                    held.logged = self.keys_repeated < REPEAT_KEYS_LOGGED;
                    self.keys_repeated = self.keys_repeated.saturating_add(1);
                }
                held.repeats = held.repeats.saturating_add(1);
                let gap = ms.wrapping_sub(held.last);
                held.last = ms;
                (held.logged && held.repeats <= REPEATS_LOGGED).then(|| {
                    format!(
                        "SDL repeat {} of scancode {scancode}: {} ms after the press, \
                         {gap} ms after the last",
                        held.repeats,
                        ms.wrapping_sub(held.pressed)
                    )
                })
            }
            Event::KeyUp { scancode } => {
                let held = self.held.take_if(|h| h.scancode == scancode)?;
                if !held.logged {
                    return None;
                }
                let mut line = format!(
                    "scancode {scancode} released {} ms after the press: {} SDL repeats, \
                     the first {} ms after the press",
                    ms.wrapping_sub(held.pressed),
                    held.repeats,
                    held.first_repeat.wrapping_sub(held.pressed)
                );
                if held.repeats > 1 {
                    let every = held.last.wrapping_sub(held.first_repeat) / (held.repeats - 1);
                    line.push_str(&format!(", then every {every} ms on average"));
                }
                Some(line)
            }
            _ => None,
        }
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
/// SDL's own key repeats are logged with their times for the first keys held
/// long enough to repeat, so a TV run shows how fast the remote repeats: the
/// first few of each key, then at its release how many came and how far
/// apart. The shell's repeats do not depend on them.
const REPEAT_KEYS_LOGGED: u32 = 8;
const REPEATS_LOGGED: u32 = 5;

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
    get_ticks: unsafe extern "C" fn() -> u32,
    /// webOS only: shows or hides the Magic Remote pointer. webosbrew's SDL
    /// fork has it; whether LG's 2.0.5 on the B9 does, the log says.
    webos_cursor_visibility: Option<unsafe extern "C" fn(c_int)>,
    key_layout: KeyLayout,
    raw_keys_logged: Cell<u32>,
    raw_kinds_logged: RefCell<Vec<u64>>,
    layout_warned: Cell<bool>,
    clock_warned: Cell<bool>,
    repeat_times: RefCell<RepeatTimes>,
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
                get_ticks: sym(&lib, b"SDL_GetTicks\0")?,
                // Declared without a return value: webosbrew's header makes it
                // `void`, and on every ABI here a caller may ignore one anyway.
                webos_cursor_visibility: sym(&lib, b"SDL_webOSCursorVisibility\0").ok(),
                key_layout,
                raw_keys_logged: Cell::new(0),
                raw_kinds_logged: RefCell::new(Vec::new()),
                layout_warned: Cell::new(false),
                clock_warned: Cell::new(false),
                repeat_times: RefCell::new(RepeatTimes::default()),
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

    /// The next event, if one is queued, with the time SDL stamped it.
    pub fn poll_event(&self) -> Option<(Event, Instant)> {
        let mut raw = RawEvent::zeroed();
        // SAFETY: `raw` is large and aligned enough for any SDL2 event, with
        // room to spare for LG's layout.
        (unsafe { (self.poll_event)(&mut raw) } != 0).then(|| self.decode(&raw))
    }

    /// Waits for an event for up to `timeout`, or for as long as it takes with
    /// `None`. SDL 2.0.5 (the B9's) waits by checking every 10 ms; newer SDLs
    /// sleep until an event arrives.
    pub fn wait_event(&self, timeout: Option<Duration>) -> Option<(Event, Instant)> {
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

    /// When SDL stamped `raw`, as an `Instant`. A stamp that is not on SDL's
    /// clock (warned once) gives the time the shell read the event instead,
    /// which leaves out the time it sat in the queue.
    fn stamped_at(&self, raw: &RawEvent) -> Instant {
        let read = Instant::now();
        // SAFETY: SDL_GetTicks takes no arguments and only reads SDL's clock;
        // the pointer stays valid while `_lib`, which `self` owns, is loaded.
        let now_ms = unsafe { (self.get_ticks)() };
        let stamp = event_time(raw);
        match event_instant(read, stamp, now_ms) {
            Some(at) => at,
            None => {
                if !self.clock_warned.replace(true) {
                    crate::info!(
                        "an event stamped {stamp} ms when SDL's clock reads {now_ms} ms: key \
                         times count from when the shell reads each event"
                    );
                }
                read
            }
        }
    }

    /// Parses `raw`, logging the first events of each kind byte for byte,
    /// timing SDL's first key repeats, and warning once if key events look
    /// misread. Returns the event with the time SDL stamped it.
    fn decode(&self, raw: &RawEvent) -> (Event, Instant) {
        let at = self.stamped_at(raw);
        let event = parse_event(raw, self.key_layout);
        if let Some(line) = self.repeat_times.borrow_mut().note(event, event_time(raw)) {
            crate::info!("{line}");
        }
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
        (event, at)
    }

    /// Whether this SDL can hide the Magic Remote pointer.
    pub fn can_hide_pointer(&self) -> bool {
        self.webos_cursor_visibility.is_some()
    }

    /// The `pointer:` line for the log, saying whether arrows will hide the
    /// pointer (docs/specs/look-and-feel.md, "Log lines").
    pub fn pointer_line(&self) -> String {
        pointer_line(self.can_hide_pointer())
    }

    /// Shows or hides the Magic Remote pointer. Returns false, and does
    /// nothing, where the SDL has no such call.
    pub fn set_pointer_visible(&self, visible: bool) -> bool {
        let Some(set) = self.webos_cursor_visibility else {
            return false;
        };
        // SAFETY: `SDL_webOSCursorVisibility(SDL_bool)` takes one int and
        // needs only an initialised video subsystem, which `init` gave it.
        unsafe { set(c_int::from(visible)) };
        true
    }

    pub fn quit(&self) {
        // SAFETY: SDL_Quit is safe to call after (or without) SDL_Init.
        unsafe { (self.quit)() };
    }
}

/// `pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the
/// pointer`, or `... not in LG's SDL; the pointer stays as LG leaves it`.
pub fn pointer_line(can_hide: bool) -> String {
    if can_hide {
        "pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the pointer".to_owned()
    } else {
        "pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it"
            .to_owned()
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
        let raw = raw_with(0x700, |_| {});
        assert_eq!(parse_event(&raw, KeyLayout::Upstream), Event::Other(0x700));
    }

    /// A raw event from a `raw event 0x...: 01 04 00 00|9f 45 ...` log line.
    fn raw_from_log(hex: &str) -> RawEvent {
        let mut raw = RawEvent::zeroed();
        let bytes: Vec<u8> = hex
            .split(['|', ' '])
            .filter(|s| !s.is_empty())
            .map(|s| u8::from_str_radix(s, 16).expect("hex byte"))
            .collect();
        assert_eq!(bytes.len(), RAW_BYTES);
        raw.0[..RAW_BYTES].copy_from_slice(&bytes);
        raw
    }

    /// The B9's own mouse events (OLED55B9PUA, firmware 05.50.00, LG's SDL
    /// 2.0.5): `tools/out/archive/tv-check-20261022T015118Z/run.log` lines
    /// 111 and 113 and `...T044903Z/run2.log` lines 30, 31, 650 and 651.
    const B9_MOUSE: [(&str, Event); 7] = [
        (
            "01 04 00 00|9f 45 00 00|01 00 00 00|02 00 00 00|01 01 01 00|80 05 00 00|af 01 00 00|3c 7e 63 00|6c 63 09 01|48 b3 60 00|48 b3 60 00|79 02 8a f3",
            Event::MouseButton {
                button: 1,
                down: true,
                x: 1408,
                y: 431,
            },
        ),
        (
            "02 04 00 00|29 46 00 00|01 00 00 00|02 00 00 00|01 00 01 00|88 05 00 00|b1 01 00 00|fb ff ff ff|00 00 00 00|c9 42 d7 f5|08 e9 84 00|00 00 00 00",
            Event::MouseButton {
                button: 1,
                down: false,
                x: 1416,
                y: 433,
            },
        ),
        (
            "00 04 00 00|0d 1d 00 00|01 00 00 00|02 00 00 00|00 00 00 00|04 04 00 00|3b 02 00 00|04 04 00 00|3b 02 00 00|38 04 00 00|0a 00 00 00|04 04 00 00",
            Event::MouseMotion,
        ),
        (
            "01 04 00 00|d3 30 00 00|01 00 00 00|02 00 00 00|01 01 01 00|46 02 00 00|9d 01 00 00|f1 ff ff ff|fe ff ff ff|c9 e2 db f6|e0 09 24 01|d8 09 24 01",
            Event::MouseButton {
                button: 1,
                down: true,
                x: 582,
                y: 413,
            },
        ),
        (
            "02 04 00 00|6a 31 00 00|01 00 00 00|02 00 00 00|01 00 01 00|45 02 00 00|97 01 00 00|fa ff ff ff|ff ff ff ff|c9 e2 db f6|d0 29 29 01|c8 29 29 01",
            Event::MouseButton {
                button: 1,
                down: false,
                x: 581,
                y: 407,
            },
        ),
        (
            "01 04 00 00|f2 e0 03 00|01 00 00 00|02 00 00 00|01 01 01 00|a6 02 00 00|df 01 00 00|ff ff ff ff|fa ff ff ff|c9 22 d5 f6|08 e9 84 00|00 00 00 00",
            Event::MouseButton {
                button: 1,
                down: true,
                x: 678,
                y: 479,
            },
        ),
        (
            "02 04 00 00|89 e1 03 00|01 00 00 00|02 00 00 00|01 00 01 00|9c 02 00 00|e0 01 00 00|01 00 00 00|f6 ff ff ff|c9 22 d5 f6|e8 32 c3 00|e0 32 c3 00",
            Event::MouseButton {
                button: 1,
                down: false,
                x: 668,
                y: 480,
            },
        ),
    ];

    #[test]
    fn parses_the_b9s_mouse_events_at_upstream_offsets_in_both_layouts() {
        for (hex, want) in B9_MOUSE {
            let raw = raw_from_log(hex);
            assert_eq!(parse_event(&raw, KeyLayout::Lg), want, "{hex}");
            assert_eq!(parse_event(&raw, KeyLayout::Upstream), want, "{hex}");
        }
    }

    #[test]
    fn the_wheel_is_an_event_of_its_own() {
        let raw = raw_with(EVENT_MOUSEWHEEL, |_| {});
        assert_eq!(parse_event(&raw, KeyLayout::Lg), Event::MouseWheel);
    }

    #[test]
    fn a_mouse_events_stamp_is_at_byte_4_too() {
        let raw = raw_from_log(B9_MOUSE[0].0);
        assert_eq!(event_time(&raw), 0x459f);
    }

    #[test]
    fn the_button_comes_from_the_event_not_the_state_byte() {
        // A down event whose state byte reads released still counts as down:
        // the type is what SDL dispatches on.
        let raw = raw_with(EVENT_MOUSEBUTTONDOWN, |b| {
            b[16] = 3;
            b[17] = 0;
            b[20..24].copy_from_slice(&(-7i32).to_ne_bytes());
        });
        assert_eq!(
            parse_event(&raw, KeyLayout::Upstream),
            Event::MouseButton {
                button: 3,
                down: true,
                x: -7,
                y: 0
            }
        );
    }

    #[test]
    fn the_pointer_line_says_whether_arrows_hide_it() {
        assert_eq!(
            pointer_line(true),
            "pointer: SDL_webOSCursorVisibility found in LG's SDL; arrows hide the pointer"
        );
        assert_eq!(
            pointer_line(false),
            "pointer: SDL_webOSCursorVisibility not in LG's SDL; the pointer stays as LG leaves it"
        );
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

    #[test]
    fn reads_the_timestamp_ahead_of_lgs_extra_field() {
        let mut raw = lg_key(EVENT_KEYDOWN, 1, 1, 79);
        raw.0[4..8].copy_from_slice(&123_456u32.to_ne_bytes());
        assert_eq!(event_time(&raw), 123_456);
    }

    fn down(scancode: i32, repeat: bool) -> Event {
        Event::KeyDown {
            scancode,
            keycode: 0,
            repeat,
        }
    }

    #[test]
    fn times_sdls_repeats_of_a_held_key() {
        let mut times = RepeatTimes::default();
        assert_eq!(times.note(down(79, false), 1000), None);
        assert_eq!(
            times.note(down(79, true), 1500).as_deref(),
            Some("SDL repeat 1 of scancode 79: 500 ms after the press, 500 ms after the last")
        );
        assert_eq!(
            times.note(down(79, true), 1550).as_deref(),
            Some("SDL repeat 2 of scancode 79: 550 ms after the press, 50 ms after the last")
        );
        assert!(times.note(down(79, true), 1610).is_some());
        assert_eq!(
            times.note(Event::KeyUp { scancode: 79 }, 1630).as_deref(),
            Some(
                "scancode 79 released 630 ms after the press: 3 SDL repeats, the first \
                 500 ms after the press, then every 55 ms on average"
            )
        );
    }

    #[test]
    fn logs_the_first_repeats_of_the_first_keys_only() {
        let mut times = RepeatTimes::default();
        let mut lines = 0;
        for key in 0..REPEAT_KEYS_LOGGED + 2 {
            let t = key * 10_000;
            times.note(down(80, false), t);
            for n in 1..=REPEATS_LOGGED + 3 {
                lines += u32::from(times.note(down(80, true), t + 400 + n * 100).is_some());
            }
            lines += u32::from(
                times
                    .note(Event::KeyUp { scancode: 80 }, t + 2000)
                    .is_some(),
            );
        }
        // Each logged key gives its first repeats and the summary at release.
        assert_eq!(lines, REPEAT_KEYS_LOGGED * (REPEATS_LOGGED + 1));
    }

    #[test]
    fn taps_and_other_keys_repeats_log_nothing() {
        let mut times = RepeatTimes::default();
        // A tap: no repeats, no summary, and it uses up none of the keys logged.
        times.note(down(40, false), 0);
        assert_eq!(times.note(Event::KeyUp { scancode: 40 }, 120), None);
        // A repeat of a key never pressed, or of one not pressed last.
        assert_eq!(times.note(down(81, true), 500), None);
        times.note(down(79, false), 1000);
        assert_eq!(times.note(down(80, true), 1500), None);
        assert_eq!(times.note(Event::KeyUp { scancode: 80 }, 1600), None);
        assert_eq!(times.keys_repeated, 0);
    }

    #[test]
    fn repeat_times_survive_sdls_clock_wrapping() {
        let mut times = RepeatTimes::default();
        times.note(down(79, false), u32::MAX - 99);
        assert_eq!(
            times.note(down(79, true), 400).as_deref(),
            Some("SDL repeat 1 of scancode 79: 500 ms after the press, 500 ms after the last")
        );
    }

    #[test]
    fn event_age_counts_back_from_sdls_clock() {
        assert_eq!(event_age(1000, 1025), Some(Duration::from_millis(25)));
        assert_eq!(event_age(1000, 1000), Some(Duration::ZERO));
    }

    #[test]
    fn event_age_survives_the_clock_wrapping() {
        assert_eq!(event_age(u32::MAX - 9, 5), Some(Duration::from_millis(15)));
    }

    #[test]
    fn a_stamp_from_the_future_old_or_missing_is_not_on_sdls_clock() {
        assert_eq!(event_age(2000, 1000), None, "from the future");
        assert_eq!(event_age(1001, 1000), None, "1 ms in the future");
        assert_eq!(event_age(1000, 1000 + 10_001), None, "older than 10 s");
        assert_eq!(event_age(0, 5), None, "never stamped");
    }

    #[test]
    fn an_event_exactly_10_s_old_still_counts() {
        assert_eq!(event_age(1000, 11_000), Some(Duration::from_secs(10)));
    }

    #[test]
    fn an_event_is_placed_back_by_its_age() {
        let read = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            event_instant(read, 1000, 1025),
            Some(read - Duration::from_millis(25))
        );
        assert_eq!(event_instant(read, 2000, 1000), None);
    }
}
