//! The shell's own window on the TV's compositor (decision 15): the home
//! loop's events (`home_loop::Events`) and its screen's window
//! (`screen::Window`) without LG's SDL.
//!
//! It follows the recipe Kodi and SDL-webOS use on LG TVs (research of
//! 2026-11-05, "What a native client must do"), written here from that
//! description and the protocol's own tables:
//!
//! 1. Connect to the compositor (`$XDG_RUNTIME_DIR/wayland-0` in the jail),
//!    bind `wl_compositor`, `wl_shell`, `wl_webos_shell`, `wl_seat`,
//!    `wl_output` and `wl_webos_input_manager`.
//! 2. One `wl_surface`, with a `wl_shell` role (`set_toplevel`) and a webOS
//!    shell surface whose string properties tie it to the app (`appId`),
//!    say its window type (`_WEBOS_WINDOW_TYPE`, a card unless the overlay
//!    trial names another) and ask for BACK and EXIT
//!    (`_WEBOS_ACCESS_POLICY_KEYS_*`, as the SDL path's hints do).
//! 3. EGL on the surface (`platform::egl`); the first swap maps it, and the
//!    compositor makes a card full screen.
//! 4. The compositor's states (`state_changed`: minimized or full screen),
//!    `close`, and keyboard focus become the window events the loop knows
//!    from SDL; keys and the pointer become SDL's key and mouse events
//!    (`platform::keys`).
//! 5. A relaunch from the app manager (`platform::lifecycle`) raises the
//!    window (`set_state(fullscreen)`) and reaches the loop as
//!    `Event::Relaunch`.
//!
//! The loop waits on the compositor's socket and a pipe the lifecycle
//! thread writes to, together. `wl_keyboard` sends no repeats, so a held
//! key is said to be still held every `HELD_SIGN`, as SDL's repeats said
//! it, or the core would count it released after 1.5 s.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::{CString, c_int, c_void};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::egl::{self, Egl, Made};
use super::keys::{self, KeyClock};
use super::lifecycle::{LifeEvent, Sink};
use super::wl::{self, Arg, Argument, Interface, Lib, Message, Proxy};
use crate::info;
use crate::render::{SCREEN_H, SCREEN_W};
use crate::sdl::{Event, Rate, Refresh, window};

/// How often a held key is said to be still held.
const HELD_SIGN: Duration = Duration::from_millis(250);
/// The first key events logged raw, as the SDL path logs its first.
const RAW_KEYS: u32 = 8;
/// The most event kinds named the first time they come, and the most
/// unknown key codes named.
const NAMED_MAX: usize = 48;
const UNKNOWN_KEYS_MAX: usize = 16;
/// The most globals listed in the log.
const GLOBALS_MAX: usize = 64;

/// webOS shell surface states (`wl_webos_shell_surface.state`).
mod state {
    pub const DEFAULT: u32 = 0;
    pub const MINIMIZED: u32 = 1;
    pub const MAXIMIZED: u32 = 2;
    pub const FULLSCREEN: u32 = 3;
}

fn state_name(s: u32) -> String {
    match s {
        state::DEFAULT => "default".to_owned(),
        state::MINIMIZED => "minimized".to_owned(),
        state::MAXIMIZED => "maximized".to_owned(),
        state::FULLSCREEN => "fullscreen".to_owned(),
        other => format!("{other}"),
    }
}

const SEAT_POINTER: u32 = 1;
const SEAT_KEYBOARD: u32 = 2;
const OUTPUT_MODE_CURRENT: u32 = 1;
const KEY_PRESSED: u32 = 1;

/// What the window is to say about itself.
pub struct Setup<'a> {
    /// `appId`: the id SAM launched, from `APPID`.
    pub app_id: &'a str,
    /// `_WEBOS_WINDOW_TYPE`.
    pub window_type: &'a str,
    /// `displayAffinity`: `DISPLAY_ID`, or `0`.
    pub display_id: &'a str,
}

/// The opcodes the shell sends, found by name (`wl::Interface::request`).
struct Ops {
    get_registry: u32,
    bind: u32,
    create_surface: u32,
    surface_destroy: u32,
    get_keyboard: u32,
    get_pointer: u32,
    shell: Option<ShellOps>,
    webos: Option<WebosOps>,
    cursor_visibility: Option<u32>,
}

struct ShellOps {
    get_shell_surface: u32,
    set_toplevel: u32,
    pong: u32,
}

struct WebosOps {
    get_shell_surface: u32,
    set_property: u32,
    set_state: u32,
}

impl Ops {
    fn resolve(lib: &Lib) -> Result<Self, String> {
        let shell = match (lib.shell, lib.shell_surface) {
            (Some(shell), Some(surface)) => Some(ShellOps {
                get_shell_surface: shell.request("get_shell_surface", "no")?,
                set_toplevel: surface.request("set_toplevel", "")?,
                pong: surface.request("pong", "u")?,
            }),
            _ => None,
        };
        let webos = match (lib.webos_shell, lib.webos_shell_surface) {
            (Some(shell), Some(surface)) => Some(WebosOps {
                get_shell_surface: shell.request("get_shell_surface", "no")?,
                set_property: surface.request("set_property", "ss")?,
                set_state: surface.request("set_state", "u")?,
            }),
            _ => None,
        };
        Ok(Self {
            get_registry: lib.display.request("get_registry", "n")?,
            bind: lib.registry.request("bind", "usun")?,
            create_surface: lib.compositor.request("create_surface", "n")?,
            surface_destroy: lib.surface.request("destroy", "")?,
            get_keyboard: lib.seat.request("get_keyboard", "n")?,
            get_pointer: lib.seat.request("get_pointer", "n")?,
            shell,
            webos,
            cursor_visibility: lib
                .webos_input_manager
                .and_then(|m| m.request("set_cursor_visibility", "u").ok()),
        })
    }
}

/// Which of the shell's objects an event is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Registry,
    Seat,
    Keyboard,
    Pointer,
    ShellSurface,
    WebosSurface,
    Output,
    InputManager,
    Surface,
    Other,
}

/// A key held down, and when it was last said to be.
#[derive(Debug, Clone, Copy)]
struct Held {
    code: u32,
    scancode: i32,
    said: Instant,
}

#[derive(Default)]
struct State {
    pointer_at: (i32, i32),
    held: Vec<Held>,
    key_clock: KeyClock,
    refresh_mhz: Option<i32>,
    raw_keys: u32,
    unknown_keys: Vec<u32>,
    named: Vec<String>,
    globals: Vec<String>,
    seat_version: u32,
    /// The connection broke; said once.
    failed: bool,
}

/// What the dispatcher reaches: the objects, the events for the loop and
/// the state behind them. Boxed, so its address holds for the proxies.
struct Inner {
    lib: &'static Lib,
    ops: Ops,
    display: *mut Proxy,
    registry: Cell<*mut Proxy>,
    compositor: Cell<*mut Proxy>,
    shell: Cell<*mut Proxy>,
    webos_shell: Cell<*mut Proxy>,
    seat: Cell<*mut Proxy>,
    keyboard: Cell<*mut Proxy>,
    pointer: Cell<*mut Proxy>,
    output: Cell<*mut Proxy>,
    input_manager: Cell<*mut Proxy>,
    surface: Cell<*mut Proxy>,
    shell_surface: Cell<*mut Proxy>,
    webos_surface: Cell<*mut Proxy>,
    events: RefCell<VecDeque<(Event, Instant)>>,
    state: RefCell<State>,
}

/// The write end of the pipe the lifecycle thread wakes the loop through.
struct Writer(c_int);

impl Writer {
    fn wake(&self) {
        let byte = 1u8;
        // SAFETY: one byte from a live buffer to a pipe this owns; a full
        // pipe (EAGAIN) already holds a wake-up.
        unsafe { libc::write(self.0, (&raw const byte).cast(), 1) };
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: this owns the descriptor and closes it once.
        unsafe { libc::close(self.0) };
    }
}

/// The compositor connection, the window's objects and the wake pipe.
pub struct Wayland {
    lib: &'static Lib,
    display: *mut Proxy,
    fd: c_int,
    wake_read: c_int,
    sink: Arc<Sink>,
    inner: Box<Inner>,
}

/// Wall-clock and monotonic milliseconds, wrapping, as key stamps are.
/// `time_t` and `long` are 32 bits on the TV and 64 on a PC, so the casts
/// are needed on one and not the other.
#[allow(clippy::unnecessary_cast)]
fn clock_ms(clock: libc::clockid_t) -> u32 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid timespec for the call to fill.
    unsafe { libc::clock_gettime(clock, &mut ts) };
    let ms = (ts.tv_sec as i64)
        .wrapping_mul(1000)
        .wrapping_add(ts.tv_nsec as i64 / 1_000_000);
    ms as u32
}

impl Wayland {
    /// Connects, binds what the shell needs and makes its surface, with the
    /// properties `setup` gives. Nothing is shown before the first swap.
    pub fn connect(setup: &Setup<'_>) -> Result<Self, String> {
        let lib = Lib::load()?;
        let ops = Ops::resolve(lib)?;
        info!(
            "wayland: {} and {}; wl_proxy_add_dispatcher, opcodes by name",
            lib.client_file,
            lib.webos_file
                .as_deref()
                .unwrap_or("no webOS extension library")
        );
        if let Some(table) = lib.webos_shell_surface {
            info!(
                "wayland: {} v{}: {}",
                table.name(),
                table.version(),
                table.requests_line()
            );
        }
        // SAFETY: a null name connects to $WAYLAND_DISPLAY or wayland-0.
        let display = unsafe { (lib.display_connect)(std::ptr::null()) };
        if display.is_null() {
            return Err(format!(
                "no compositor at {}/{}",
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "?".to_owned()),
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned())
            ));
        }
        let mut fds = [0; 2];
        // SAFETY: `fds` holds the two descriptors pipe2 writes.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
            // SAFETY: connected above, and nothing else holds it.
            unsafe { (lib.display_disconnect)(display) };
            return Err(format!("pipe2: {}", std::io::Error::last_os_error()));
        }
        let writer = Arc::new(Writer(fds[1]));
        let sink = Sink::new(move || writer.wake());
        let null = || Cell::new(std::ptr::null_mut());
        let wl = Self {
            lib,
            display,
            // SAFETY: a live display.
            fd: unsafe { (lib.display_get_fd)(display) },
            wake_read: fds[0],
            sink,
            inner: Box::new(Inner {
                lib,
                ops,
                display,
                registry: null(),
                compositor: null(),
                shell: null(),
                webos_shell: null(),
                seat: null(),
                keyboard: null(),
                pointer: null(),
                output: null(),
                input_manager: null(),
                surface: null(),
                shell_surface: null(),
                webos_surface: null(),
                events: RefCell::new(VecDeque::new()),
                state: RefCell::new(State::default()),
            }),
        };
        // From here `Drop` takes the connection down on any failure.
        wl.inner.start()?;
        wl.inner.make_surface(setup)?;
        // SAFETY: a live display.
        unsafe { (lib.display_flush)(display) };
        info!(
            "wayland: globals {}",
            wl.inner.state.borrow().globals.join(", ")
        );
        Ok(wl)
    }

    /// The lifecycle thread's sink: what it hears wakes the loop.
    pub fn lifecycle_sink(&self) -> Arc<Sink> {
        Arc::clone(&self.sink)
    }

    /// The EGL window on the surface, at the UI's 1920x1080, ES 3 if `es3`.
    pub fn open_egl(&self, egl: &'static Egl, es3: bool) -> Result<(EglWindow<'_>, Made), String> {
        let size = (SCREEN_W as i32, SCREEN_H as i32);
        // SAFETY: the display and surface are live for as long as `self`,
        // which the window borrows.
        let (window, made) =
            unsafe { egl::Window::open(egl, self.display, self.inner.surface.get(), size, es3)? };
        Ok((
            EglWindow {
                wl: self,
                window,
                size,
                refused: 0,
            },
            made,
        ))
    }

    /// Asks the compositor to put the window in front.
    pub fn raise(&self) {
        self.inner.set_state(state::FULLSCREEN);
        // SAFETY: a live display.
        unsafe { (self.lib.display_flush)(self.display) };
    }

    /// The output's refresh rate, as the SDL path reports SDL's.
    pub fn refresh(&self) -> Refresh {
        let mode = self
            .inner
            .state
            .borrow()
            .refresh_mhz
            .map_or(Rate::Absent, |mhz| Rate::Hz(i64::from((mhz + 500) / 1000)));
        Refresh {
            webos: Rate::Absent,
            mode,
            took: Duration::ZERO,
        }
    }

    /// Whether arrows can hide the pointer: LG's input manager is bound and
    /// has `set_cursor_visibility`.
    pub fn can_hide_pointer(&self) -> bool {
        self.inner.ops.cursor_visibility.is_some() && !self.inner.input_manager.get().is_null()
    }

    /// The `pointer:` line for the log.
    pub fn pointer_line(&self) -> String {
        if self.can_hide_pointer() {
            "pointer: wl_webos_input_manager.set_cursor_visibility found; arrows hide the pointer"
                .to_owned()
        } else {
            "pointer: wl_webos_input_manager.set_cursor_visibility not found; the pointer stays as LG leaves it"
                .to_owned()
        }
    }

    fn hide_pointer(&self) -> bool {
        let (Some(op), manager) = (
            self.inner.ops.cursor_visibility,
            self.inner.input_manager.get(),
        ) else {
            return false;
        };
        if manager.is_null() {
            return false;
        }
        let mut args = [Argument::uint(0)];
        // SAFETY: the manager is live; its request takes one uint.
        unsafe {
            self.lib.send(manager, op, &mut args);
            (self.lib.display_flush)(self.display);
        }
        true
    }

    /// Turns what the lifecycle thread heard into events: a relaunch raises
    /// the window first.
    fn take_lifecycle(&self) {
        for (event, at) in self.sink.take() {
            let event = match event {
                LifeEvent::Relaunch { .. } => {
                    self.raise();
                    Event::Relaunch {
                        home_key: event.home_key(),
                    }
                }
                LifeEvent::Pause { .. } => Event::Background,
                LifeEvent::Close { .. } => Event::Quit,
                LifeEvent::Registered | LifeEvent::Refused(_) | LifeEvent::Other(_) => continue,
            };
            self.inner.events.borrow_mut().push_back((event, at));
        }
    }

    /// Dispatches what libwayland has read; false once the connection broke.
    fn dispatch_pending(&self) -> bool {
        if self.inner.state.borrow().failed {
            return false;
        }
        // SAFETY: a live display, dispatched from this thread only.
        if unsafe { (self.lib.display_dispatch_pending)(self.display) } < 0 {
            self.broke("dispatching");
            return false;
        }
        true
    }

    fn broke(&self, during: &str) {
        let mut state = self.inner.state.borrow_mut();
        if std::mem::replace(&mut state.failed, true) {
            return;
        }
        // SAFETY: a live display.
        let code = unsafe { (self.lib.display_get_error)(self.display) };
        info!("wayland: the connection failed while {during}: error {code}; leaving");
        self.inner
            .events
            .borrow_mut()
            .push_back((Event::Quit, Instant::now()));
    }

    /// Reads from the socket, waiting up to `wait` (or for ever with `None`)
    /// for it or the wake pipe.
    fn read(&self, wait: Option<Duration>) {
        let (lib, display) = (self.lib, self.display);
        if self.inner.state.borrow().failed {
            if let Some(wait) = wait {
                std::thread::sleep(wait.min(Duration::from_millis(100)));
            }
            return;
        }
        // SAFETY: a live display, read from this thread only; a failed
        // prepare means events are queued, which the caller dispatches.
        if unsafe { (lib.display_prepare_read)(display) } != 0 {
            return;
        }
        // SAFETY: a live display; EAGAIN leaves the rest for the next flush.
        unsafe { (lib.display_flush)(display) };
        let mut fds = [
            libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.wake_read,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ms = wait.map_or(-1, |w| {
            c_int::try_from(w.as_micros().div_ceil(1000)).unwrap_or(c_int::MAX)
        });
        // SAFETY: two valid pollfds.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
        let socket = ready > 0 && fds[0].revents != 0;
        if socket {
            // SAFETY: prepared above.
            if unsafe { (lib.display_read_events)(display) } < 0 {
                self.broke("reading");
            }
        } else {
            // SAFETY: prepared above and not read.
            unsafe { (lib.display_cancel_read)(display) };
        }
        if ready > 0 && fds[1].revents != 0 {
            let mut buf = [0u8; 64];
            // SAFETY: a non-blocking pipe this owns, into a live buffer.
            while unsafe { libc::read(self.wake_read, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
        }
    }

    /// The next event, waiting up to `timeout` (`None`: as long as it
    /// takes).
    fn next(&self, timeout: Option<Duration>) -> Option<(Event, Instant)> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.take_lifecycle();
            self.dispatch_pending();
            if let Some(event) = self.inner.events.borrow_mut().pop_front() {
                return Some(event);
            }
            let now = Instant::now();
            if let Some(sign) = self.inner.held_sign(now) {
                return Some(sign);
            }
            if deadline.is_some_and(|d| d <= now) {
                // One look at the socket without waiting, then done.
                self.read(Some(Duration::ZERO));
                self.take_lifecycle();
                self.dispatch_pending();
                return self.inner.events.borrow_mut().pop_front();
            }
            let until = [deadline, self.inner.next_held_sign()]
                .into_iter()
                .flatten()
                .min();
            self.read(until.map(|u| u.saturating_duration_since(now)));
        }
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        let (lib, inner) = (self.lib, &self.inner);
        // SAFETY: every proxy below was made on this display and is
        // destroyed once, the surface's roles before the surface; the
        // display goes last, after a flush of the surface's destroy.
        unsafe {
            for cell in [&inner.webos_surface, &inner.shell_surface] {
                let proxy = cell.replace(std::ptr::null_mut());
                if !proxy.is_null() {
                    (lib.proxy_destroy)(proxy);
                }
            }
            let surface = inner.surface.replace(std::ptr::null_mut());
            if !surface.is_null() {
                lib.send(surface, inner.ops.surface_destroy, &mut []);
                (lib.proxy_destroy)(surface);
            }
            for cell in [
                &inner.keyboard,
                &inner.pointer,
                &inner.seat,
                &inner.output,
                &inner.input_manager,
                &inner.webos_shell,
                &inner.shell,
                &inner.compositor,
                &inner.registry,
            ] {
                let proxy = cell.replace(std::ptr::null_mut());
                if !proxy.is_null() {
                    (lib.proxy_destroy)(proxy);
                }
            }
            (lib.display_flush)(self.display);
            (lib.display_disconnect)(self.display);
            libc::close(self.wake_read);
        }
    }
}

/// The dispatcher every proxy of the shell's reports to.
///
/// # Safety
/// `data` is the `Inner` of a live `Wayland` (each proxy is destroyed
/// before it); libwayland passes the event's message and one argument per
/// type of its signature.
unsafe extern "C" fn dispatch(
    data: *const c_void,
    target: *mut c_void,
    _opcode: u32,
    message: *const Message,
    args: *mut Argument,
) -> c_int {
    // SAFETY: forwarded from the caller.
    let (inner, message) = unsafe { (&*data.cast::<Inner>(), &*message) };
    // SAFETY: forwarded from the caller.
    let args = unsafe { wl::decode(message.signature(), args) };
    inner.event(target, message.name(), &args);
    0
}

impl Inner {
    /// Gets the registry and binds the globals, then the seat's devices.
    fn start(&self) -> Result<(), String> {
        let mut args = [Argument::NULL];
        // SAFETY: a live display; get_registry's one argument is its new id.
        let registry = unsafe {
            self.lib.create(
                self.display,
                self.ops.get_registry,
                &mut args,
                self.lib.registry,
                1,
            )
        };
        if registry.is_null() {
            return Err("wl_display.get_registry failed".to_owned());
        }
        self.registry.set(registry);
        self.listen(registry);
        // The first round binds the globals; the second hears the seat's
        // capabilities and the output's mode.
        for _ in 0..2 {
            // SAFETY: a live display, dispatched from this thread only.
            if unsafe { (self.lib.display_roundtrip)(self.display) } < 0 {
                // SAFETY: as above.
                let code = unsafe { (self.lib.display_get_error)(self.display) };
                return Err(format!("the compositor broke off: error {code}"));
            }
        }
        if self.compositor.get().is_null() {
            return Err("the compositor offers no wl_compositor".to_owned());
        }
        if self.shell.get().is_null() && self.webos_shell.get().is_null() {
            return Err("the compositor offers neither wl_webos_shell nor wl_shell".to_owned());
        }
        Ok(())
    }

    /// The surface, its `wl_shell` role and its webOS shell surface with
    /// its properties.
    fn make_surface(&self, setup: &Setup<'_>) -> Result<(), String> {
        let lib = self.lib;
        let mut args = [Argument::NULL];
        // SAFETY: a live compositor; create_surface's one argument is its new id.
        let surface = unsafe {
            lib.create(
                self.compositor.get(),
                self.ops.create_surface,
                &mut args,
                lib.surface,
                1,
            )
        };
        if surface.is_null() {
            return Err("wl_compositor.create_surface failed".to_owned());
        }
        self.surface.set(surface);
        self.listen(surface);
        if let (Some(ops), Some(table)) = (&self.ops.shell, lib.shell_surface)
            && !self.shell.get().is_null()
        {
            let mut args = [Argument::NULL, Argument::object(surface)];
            // SAFETY: a live wl_shell and surface; "no" as checked.
            let role =
                unsafe { lib.create(self.shell.get(), ops.get_shell_surface, &mut args, table, 1) };
            if !role.is_null() {
                self.shell_surface.set(role);
                self.listen(role);
                // SAFETY: a live wl_shell_surface; set_toplevel takes nothing.
                unsafe { lib.send(role, ops.set_toplevel, &mut []) };
            }
        }
        if let (Some(ops), Some(table)) = (&self.ops.webos, lib.webos_shell_surface)
            && !self.webos_shell.get().is_null()
        {
            let mut args = [Argument::NULL, Argument::object(surface)];
            // SAFETY: a live wl_webos_shell and surface; "no" as checked.
            let webos = unsafe {
                lib.create(
                    self.webos_shell.get(),
                    ops.get_shell_surface,
                    &mut args,
                    table,
                    1,
                )
            };
            if webos.is_null() {
                return Err("wl_webos_shell.get_shell_surface failed".to_owned());
            }
            self.webos_surface.set(webos);
            self.listen(webos);
            for (name, value) in [
                ("appId", setup.app_id),
                ("_WEBOS_WINDOW_TYPE", setup.window_type),
                ("displayAffinity", setup.display_id),
                ("title", "Lumen"),
                ("_WEBOS_ACCESS_POLICY_KEYS_BACK", "true"),
                ("_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true"),
            ] {
                self.set_property(name, value);
            }
            info!(
                "wayland: window appId {}, type {}, displayAffinity {}",
                setup.app_id, setup.window_type, setup.display_id
            );
        }
        Ok(())
    }

    fn set_property(&self, name: &str, value: &str) {
        let (Some(ops), webos) = (&self.ops.webos, self.webos_surface.get()) else {
            return;
        };
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        if webos.is_null() {
            return;
        }
        let mut args = [Argument::string(&name), Argument::string(&value)];
        // SAFETY: a live webOS shell surface; "ss" as checked, both strings
        // live across the call, which copies them.
        unsafe { self.lib.send(webos, ops.set_property, &mut args) };
    }

    fn set_state(&self, to: u32) {
        let (Some(ops), webos) = (&self.ops.webos, self.webos_surface.get()) else {
            return;
        };
        if webos.is_null() {
            return;
        }
        let mut args = [Argument::uint(to)];
        // SAFETY: a live webOS shell surface; "u" as checked.
        unsafe { self.lib.send(webos, ops.set_state, &mut args) };
    }

    /// Routes `proxy`'s events to `dispatch`.
    fn listen(&self, proxy: *mut Proxy) {
        // SAFETY: the proxy is new, with no dispatcher; `self` is boxed in a
        // `Wayland` that destroys every proxy before it goes.
        let routed = unsafe {
            self.lib
                .dispatch_to(proxy, dispatch, std::ptr::from_ref(self).cast())
        };
        if let Err(e) = routed {
            info!("wayland: {e}");
        }
    }

    fn kind_of(&self, target: *mut Proxy) -> Kind {
        let is = |cell: &Cell<*mut Proxy>| cell.get() == target;
        if is(&self.registry) {
            Kind::Registry
        } else if is(&self.keyboard) {
            Kind::Keyboard
        } else if is(&self.pointer) {
            Kind::Pointer
        } else if is(&self.webos_surface) {
            Kind::WebosSurface
        } else if is(&self.shell_surface) {
            Kind::ShellSurface
        } else if is(&self.seat) {
            Kind::Seat
        } else if is(&self.output) {
            Kind::Output
        } else if is(&self.input_manager) {
            Kind::InputManager
        } else if is(&self.surface) {
            Kind::Surface
        } else {
            Kind::Other
        }
    }

    fn push(&self, event: Event, at: Instant) {
        self.events.borrow_mut().push_back((event, at));
    }

    /// One event from the compositor.
    fn event(&self, target: *mut Proxy, name: &str, args: &[Arg]) {
        let kind = self.kind_of(target);
        self.name_first(kind, name, args);
        let now = Instant::now();
        let uint = |n: usize| args.get(n).and_then(Arg::uint);
        match (kind, name) {
            (Kind::Registry, "global") => self.global(args),
            (Kind::Seat, "capabilities") => self.capabilities(uint(0).unwrap_or(0)),
            (Kind::Keyboard, "keymap") => {
                if let Some(Arg::Fd(fd)) = args.get(1) {
                    // SAFETY: libwayland handed the keymap's descriptor to
                    // the shell, which reads no keymap and closes it once.
                    unsafe { libc::close(*fd) };
                }
            }
            (Kind::Keyboard, "enter") => self.push(Event::Window(window::FOCUS_GAINED), now),
            (Kind::Keyboard, "leave") => {
                let held = std::mem::take(&mut self.state.borrow_mut().held);
                for key in held {
                    self.push(
                        Event::KeyUp {
                            scancode: key.scancode,
                        },
                        now,
                    );
                }
                self.push(Event::Window(window::FOCUS_LOST), now);
            }
            (Kind::Keyboard, "key") => {
                if let (Some(time), Some(code), Some(pressed)) = (uint(1), uint(2), uint(3)) {
                    self.key(time, code, pressed == KEY_PRESSED, now);
                }
            }
            (Kind::Keyboard, "repeat_info") => {}
            (Kind::Pointer, "enter" | "motion") => {
                let at = if name == "enter" { 2 } else { 1 };
                if let (Some(x), Some(y)) = (
                    args.get(at).and_then(Arg::whole),
                    args.get(at + 1).and_then(Arg::whole),
                ) {
                    self.state.borrow_mut().pointer_at = (x, y);
                }
                self.push(Event::MouseMotion, now);
            }
            (Kind::Pointer, "button") => {
                if let (Some(code), Some(down)) = (uint(2), uint(3)) {
                    let (x, y) = self.state.borrow().pointer_at;
                    match keys::mouse_button(code) {
                        Some(button) => self.push(
                            Event::MouseButton {
                                button,
                                down: down == KEY_PRESSED,
                                x,
                                y,
                            },
                            now,
                        ),
                        None => self.push(Event::Other(0x401), now),
                    }
                }
            }
            (Kind::Pointer, "axis") => self.push(Event::MouseWheel, now),
            (Kind::ShellSurface, "ping") => {
                if let (Some(ops), Some(serial)) = (&self.ops.shell, uint(0)) {
                    let mut args = [Argument::uint(serial)];
                    // SAFETY: the target is the live wl_shell_surface; "u".
                    unsafe { self.lib.send(target, ops.pong, &mut args) };
                }
            }
            (Kind::WebosSurface, "state_changed") => {
                let to = uint(0).unwrap_or(u32::MAX);
                info!("wayland: window state {}", state_name(to));
                match to {
                    state::MINIMIZED => self.push(Event::Window(window::MINIMIZED), now),
                    state::FULLSCREEN | state::MAXIMIZED | state::DEFAULT => {
                        self.push(Event::Window(window::RESTORED), now);
                    }
                    _ => {}
                }
            }
            (Kind::WebosSurface, "state_about_to_change") => {
                info!(
                    "wayland: window state about to change to {}",
                    state_name(uint(0).unwrap_or(u32::MAX))
                );
            }
            (Kind::WebosSurface, "close") => {
                info!("wayland: the compositor closed the window");
                self.push(Event::Quit, now);
            }
            (Kind::Output, "mode") => {
                let (flags, w, h, refresh) = (
                    uint(0).unwrap_or(0),
                    args.get(1).and_then(Arg::int),
                    args.get(2).and_then(Arg::int),
                    args.get(3).and_then(Arg::int),
                );
                if flags & OUTPUT_MODE_CURRENT != 0 {
                    let mut state = self.state.borrow_mut();
                    if state.refresh_mhz.is_none() {
                        info!(
                            "wayland: output {}x{} at {:.3} Hz",
                            w.unwrap_or(0),
                            h.unwrap_or(0),
                            f64::from(refresh.unwrap_or(0)) / 1000.0
                        );
                    }
                    state.refresh_mhz = refresh;
                }
            }
            (Kind::InputManager, "cursor_visibility") => {
                let scancode = if uint(0).unwrap_or(0) != 0 {
                    lumen_core::input::scancode::WEBOS_CURSOR_SHOW
                } else {
                    lumen_core::input::scancode::WEBOS_CURSOR_HIDE
                };
                self.push(
                    Event::KeyDown {
                        scancode,
                        keycode: 0,
                        repeat: false,
                    },
                    now,
                );
            }
            _ => {}
        }
    }

    /// Names an event kind the first time it comes, with its arguments, so
    /// a TV run shows what LG's compositor sends.
    fn name_first(&self, kind: Kind, name: &str, args: &[Arg]) {
        let key = format!("{kind:?}.{name}");
        let mut state = self.state.borrow_mut();
        if state.named.len() >= NAMED_MAX || state.named.contains(&key) {
            return;
        }
        state.named.push(key.clone());
        // Keys and the pointer's moves are named without their values,
        // which the key lines give.
        let shown: Vec<String> = match kind {
            Kind::Keyboard | Kind::Pointer => vec!["…".to_owned()],
            _ => args.iter().map(show).collect(),
        };
        drop(state);
        info!("wayland: first {key}({})", shown.join(", "));
    }

    fn global(&self, args: &[Arg]) {
        let (Some(name), Some(iface), Some(version)) = (
            args.first().and_then(Arg::uint),
            args.get(1).and_then(Arg::str),
            args.get(2).and_then(Arg::uint),
        ) else {
            return;
        };
        {
            let mut state = self.state.borrow_mut();
            if state.globals.len() < GLOBALS_MAX {
                state.globals.push(format!("{iface} {version}"));
            }
        }
        let lib = self.lib;
        let bind_into = |cell: &Cell<*mut Proxy>, table: Option<&'static Interface>, cap: u32| {
            if let Some(table) = table
                && cell.get().is_null()
            {
                cell.set(self.bind(name, table, version, cap));
            }
        };
        match iface {
            "wl_compositor" => bind_into(&self.compositor, Some(lib.compositor), 3),
            "wl_shell" => bind_into(&self.shell, lib.shell, 1),
            "wl_webos_shell" => bind_into(&self.webos_shell, lib.webos_shell, 1),
            "wl_seat" => {
                bind_into(&self.seat, Some(lib.seat), 4);
                let bound = version.min(lib.seat.version()).clamp(1, 4);
                self.state.borrow_mut().seat_version = bound;
            }
            "wl_output" => bind_into(&self.output, Some(lib.output), 2),
            "wl_webos_input_manager" => {
                bind_into(&self.input_manager, lib.webos_input_manager, 1);
            }
            _ => {}
        }
    }

    /// Binds global `name` as `table`, at the version both sides speak, at
    /// most `cap`; null if libwayland refused.
    fn bind(&self, name: u32, table: &'static Interface, advertised: u32, cap: u32) -> *mut Proxy {
        let version = advertised.min(table.version()).min(cap).max(1);
        let Ok(iface) = CString::new(table.name()) else {
            return std::ptr::null_mut();
        };
        let mut args = [
            Argument::uint(name),
            Argument::string(&iface),
            Argument::uint(version),
            Argument::NULL,
        ];
        // SAFETY: the registry is live (this runs in its dispatch); "usun"
        // as checked; the name lives across the call.
        let proxy = unsafe {
            self.lib.create(
                self.registry.get(),
                self.ops.bind,
                &mut args,
                table,
                version,
            )
        };
        if !proxy.is_null() {
            self.listen(proxy);
        }
        proxy
    }

    fn capabilities(&self, caps: u32) {
        let seat = self.seat.get();
        if seat.is_null() {
            return;
        }
        let version = self.state.borrow().seat_version.max(1);
        let lib = self.lib;
        for (bit, cell, op, table) in [
            (
                SEAT_KEYBOARD,
                &self.keyboard,
                self.ops.get_keyboard,
                lib.keyboard,
            ),
            (
                SEAT_POINTER,
                &self.pointer,
                self.ops.get_pointer,
                lib.pointer,
            ),
        ] {
            if caps & bit != 0 && cell.get().is_null() {
                let mut args = [Argument::NULL];
                // SAFETY: a live seat (this runs in its dispatch); "n".
                let device = unsafe { lib.create(seat, op, &mut args, table, version) };
                if !device.is_null() {
                    cell.set(device);
                    self.listen(device);
                }
            }
        }
    }

    /// A key pressed or released at the compositor's `time`.
    fn key(&self, time: u32, code: u32, down: bool, now: Instant) {
        let scancode = keys::scancode_for(code);
        let mut state = self.state.borrow_mut();
        let (age, clock_line) = state.key_clock.age(
            time,
            clock_ms(libc::CLOCK_MONOTONIC),
            clock_ms(libc::CLOCK_REALTIME),
        );
        let at = age.map_or(now, |age| now.checked_sub(age).unwrap_or(now));
        let raw = state.raw_keys < RAW_KEYS;
        state.raw_keys = state.raw_keys.saturating_add(1);
        let unknown = scancode.is_none()
            && !state.unknown_keys.contains(&code)
            && state.unknown_keys.len() < UNKNOWN_KEYS_MAX;
        if unknown {
            state.unknown_keys.push(code);
        }
        let scancode = scancode.unwrap_or(0);
        if down {
            state.held.retain(|h| h.code != code);
            state.held.push(Held {
                code,
                scancode,
                said: at,
            });
        } else {
            state.held.retain(|h| h.code != code);
        }
        drop(state);
        if let Some(line) = clock_line {
            info!("{line}");
        }
        if raw {
            info!(
                "wayland key {code} {} at {time} ms: scancode {scancode}",
                if down { "down" } else { "up" }
            );
        }
        if unknown {
            info!("wayland key {code}: not a key the shell knows; passed on as scancode 0");
        }
        let event = if down {
            Event::KeyDown {
                scancode,
                keycode: i32::try_from(code).unwrap_or(0),
                repeat: false,
            }
        } else {
            Event::KeyUp { scancode }
        };
        self.push(event, at);
    }

    /// When a held key is next to be said to be still held.
    fn next_held_sign(&self) -> Option<Instant> {
        self.state
            .borrow()
            .held
            .iter()
            .map(|h| h.said + HELD_SIGN)
            .min()
    }

    /// A held key's sign of life, if one is due by `now`: a key-down marked
    /// as a repeat, which fires nothing in the core.
    fn held_sign(&self, now: Instant) -> Option<(Event, Instant)> {
        let mut state = self.state.borrow_mut();
        let held = state.held.iter_mut().find(|h| h.said + HELD_SIGN <= now)?;
        held.said = now;
        Some((
            Event::KeyDown {
                scancode: held.scancode,
                keycode: i32::try_from(held.code).unwrap_or(0),
                repeat: true,
            },
            now,
        ))
    }
}

/// An argument as `name_first` shows it.
fn show(arg: &Arg) -> String {
    match arg {
        Arg::Int(i) => i.to_string(),
        Arg::Uint(u) => u.to_string(),
        Arg::Fixed(f) => format!("{:.2}", f64::from(*f) / 256.0),
        Arg::Str(Some(s)) => format!("{s:?}"),
        Arg::Str(None) => "null".to_owned(),
        Arg::Object(p) | Arg::NewId(p) => {
            if p.is_null() {
                "null".to_owned()
            } else {
                "object".to_owned()
            }
        }
        Arg::Array(bytes) => format!("[{} bytes]", bytes.len()),
        Arg::Fd(_) => "fd".to_owned(),
    }
}

/// The TV's events for the home loop: the compositor's and the app
/// manager's.
impl crate::home_loop::Events for &Wayland {
    fn poll(&mut self) -> Option<(Event, Instant)> {
        self.next(Some(Duration::ZERO))
    }

    fn wait(&mut self, timeout: Option<Duration>) -> Option<(Event, Instant)> {
        self.next(timeout)
    }

    fn can_hide_pointer(&self) -> bool {
        Wayland::can_hide_pointer(self)
    }

    fn hide_pointer(&mut self) -> bool {
        Wayland::hide_pointer(self)
    }
}

/// The EGL window the screen draws on.
pub struct EglWindow<'a> {
    wl: &'a Wayland,
    window: egl::Window,
    size: (i32, i32),
    /// Swaps EGL refused, said at the first and every `SAID_EVERY` after.
    refused: u32,
}

/// How often a run of refused swaps is said again: ten seconds at 60 Hz.
const SAID_EVERY: u32 = 600;

impl crate::screen::Window for EglWindow<'_> {
    fn drawable_size(&self) -> (i32, i32) {
        self.size
    }

    fn swap(&mut self) {
        if !self.window.swap() {
            self.refused += 1;
            if self.refused % SAID_EVERY == 1 {
                info!("eglSwapBuffers refused ({} so far)", self.refused);
            }
        }
    }

    fn refresh(&mut self) -> Refresh {
        self.wl.refresh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_named() {
        assert_eq!(state_name(1), "minimized");
        assert_eq!(state_name(3), "fullscreen");
        assert_eq!(state_name(9), "9");
    }

    #[test]
    fn arguments_are_shown_short() {
        assert_eq!(show(&Arg::Fixed(384)), "1.50");
        assert_eq!(show(&Arg::Str(Some("a".to_owned()))), "\"a\"");
        assert_eq!(show(&Arg::Array(vec![0; 32])), "[32 bytes]");
        assert_eq!(show(&Arg::Object(std::ptr::null_mut())), "null");
        assert_eq!(show(&Arg::Fd(3)), "fd");
    }
}
