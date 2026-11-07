//! libwayland-client and LG's webOS protocol extensions, loaded at run time
//! (decision 15). The shell links neither: EGL needs libwayland's own
//! display object, so the shell uses the TV's library rather than speaking
//! the wire protocol itself, and LG's extensions come from the TV's
//! `libwayland-webos-client` (research of 2026-11-05, "Dropping SDL").
//!
//! Requests and events are found **by name** in the interface tables the
//! TV's libraries carry, never by number. LG's compositor speaks an older
//! variant of webOS's shell protocol whose opcodes differ from webOS OSE's
//! (SDL-webOS maps them the same way), so the opcode of `set_property` is
//! whatever LG's `wl_webos_shell_surface_interface` says, and a request
//! whose arguments differ from what the shell sends is refused before
//! anything is sent: the shell falls back to SDL and the log names LG's
//! signature. Events reach one dispatcher (`wl_proxy_add_dispatcher`),
//! which reads each one's arguments by the signature its own table gives
//! (`decode`) and hands them on by name.
//!
//! The libraries stay loaded for the life of the process: EGL holds
//! libwayland's objects, and the tables are read through `'static`
//! references into them.

use std::ffi::{CStr, c_char, c_int, c_void};

use libloading::Library;

/// `struct wl_proxy`, `struct wl_display`: opaque.
pub type Proxy = c_void;

/// `struct wl_message`: one request or event of an interface.
#[repr(C)]
pub struct Message {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub types: *const *const Interface,
}

/// `struct wl_interface`: an interface's name, version, requests and events.
#[repr(C)]
pub struct Interface {
    pub name: *const c_char,
    pub version: c_int,
    pub method_count: c_int,
    pub methods: *const Message,
    pub event_count: c_int,
    pub events: *const Message,
}

/// `union wl_argument`: one argument of a request or event. Fixed-point
/// numbers are `i`, new ids `u` or `o`, file descriptors `i`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Argument {
    pub i: i32,
    pub u: u32,
    pub s: *const c_char,
    pub o: *mut c_void,
    pub a: *mut Array,
}

impl Argument {
    pub const NULL: Self = Self {
        o: std::ptr::null_mut(),
    };

    pub const fn uint(u: u32) -> Self {
        Self { u }
    }

    #[cfg(test)]
    pub const fn int(i: i32) -> Self {
        Self { i }
    }

    pub const fn object(o: *mut Proxy) -> Self {
        Self { o }
    }

    pub const fn string(s: &CStr) -> Self {
        Self { s: s.as_ptr() }
    }
}

/// `struct wl_array`.
#[repr(C)]
pub struct Array {
    pub size: usize,
    pub alloc: usize,
    pub data: *mut c_void,
}

/// `wl_dispatcher_func_t`: (dispatcher data, proxy, opcode, message, args).
pub type Dispatcher =
    unsafe extern "C" fn(*const c_void, *mut c_void, u32, *const Message, *mut Argument) -> c_int;

/// The most bytes of an array argument kept, and of a string.
const MAX_ARRAY: usize = 4096;
const MAX_STRING: usize = 256;

/// A signature as the shell compares it: its types alone, without the
/// version the message appeared in or the marks for a nullable argument.
pub fn plain(signature: &str) -> String {
    signature
        .chars()
        .filter(|c| !c.is_ascii_digit() && *c != '?')
        .collect()
}

/// One argument of an event, read by its signature.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i32),
    Uint(u32),
    /// 24.8 fixed point, as sent.
    Fixed(i32),
    Str(Option<String>),
    Object(*mut Proxy),
    NewId(*mut Proxy),
    Array(Vec<u8>),
    /// A file descriptor, now the shell's to close.
    Fd(i32),
}

impl Arg {
    pub fn uint(&self) -> Option<u32> {
        match *self {
            Self::Uint(u) => Some(u),
            Self::Int(i) => u32::try_from(i).ok(),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i32> {
        match *self {
            Self::Int(i) | Self::Fixed(i) => Some(i),
            Self::Uint(u) => i32::try_from(u).ok(),
            _ => None,
        }
    }

    /// A fixed-point argument in whole units, rounded down.
    pub fn whole(&self) -> Option<i32> {
        match *self {
            Self::Fixed(f) => Some(f >> 8),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Self::Str(Some(s)) => Some(s),
            _ => None,
        }
    }
}

/// Reads an event's arguments by its signature.
///
/// # Safety
/// `args` must hold one valid argument for each type in `signature`, as
/// libwayland hands a dispatcher an event's, and every string and array in
/// them must be live for the call.
pub unsafe fn decode(signature: &str, args: *const Argument) -> Vec<Arg> {
    let types = plain(signature);
    let mut out = Vec::with_capacity(types.len());
    for (n, kind) in types.chars().enumerate() {
        // SAFETY: forwarded from the caller: one argument per type.
        let arg = unsafe { *args.add(n) };
        // SAFETY: each union field read is the one libwayland wrote for this
        // type; strings and arrays are live (the caller).
        let decoded = unsafe {
            match kind {
                'i' => Arg::Int(arg.i),
                'u' => Arg::Uint(arg.u),
                'f' => Arg::Fixed(arg.i),
                'h' => Arg::Fd(arg.i),
                's' => Arg::Str((!arg.s.is_null()).then(|| {
                    let text = CStr::from_ptr(arg.s).to_string_lossy();
                    text.chars().take(MAX_STRING).collect()
                })),
                'o' => Arg::Object(arg.o),
                'n' => Arg::NewId(arg.o),
                'a' => Arg::Array(if arg.a.is_null() || (*arg.a).data.is_null() {
                    Vec::new()
                } else {
                    let len = (*arg.a).size.min(MAX_ARRAY);
                    std::slice::from_raw_parts((*arg.a).data.cast::<u8>(), len).to_vec()
                }),
                // A type this reader does not know: nothing after it can be
                // read safely.
                _ => break,
            }
        };
        out.push(decoded);
    }
    out
}

/// Text from a table's C string.
///
/// # Safety
/// `ptr` is null or a NUL-terminated string that lives as long as `'a`.
unsafe fn text<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: forwarded from the caller.
    unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("")
}

impl Message {
    pub fn name(&self) -> &str {
        // SAFETY: a table's name is a static C string in its library, which
        // stays loaded (module doc), or a test's literal.
        unsafe { text(self.name) }
    }

    pub fn signature(&self) -> &str {
        // SAFETY: as `name`.
        unsafe { text(self.signature) }
    }
}

impl Interface {
    pub fn name(&self) -> &str {
        // SAFETY: as `Message::name`.
        unsafe { text(self.name) }
    }

    /// The version this table speaks, the most the shell binds.
    pub fn version(&self) -> u32 {
        u32::try_from(self.version).unwrap_or(0)
    }

    fn list(&self, events: bool) -> &[Message] {
        let (ptr, count) = if events {
            (self.events, self.event_count)
        } else {
            (self.methods, self.method_count)
        };
        match usize::try_from(count) {
            // SAFETY: a table's array holds its count of messages, static in
            // its library (module doc).
            Ok(n) if n > 0 && !ptr.is_null() => unsafe { std::slice::from_raw_parts(ptr, n) },
            _ => &[],
        }
    }

    /// The opcode of the request `name`, if its arguments are `types` (a
    /// plain signature); else why not, naming what the table has.
    pub fn request(&self, name: &str, types: &str) -> Result<u32, String> {
        let list = self.list(false);
        let Some((opcode, message)) = list.iter().enumerate().find(|(_, m)| m.name() == name)
        else {
            return Err(format!("{} has no request {name}", self.name()));
        };
        let found = plain(message.signature());
        if found != types {
            return Err(format!(
                "{}.{name} takes \"{found}\" here, not \"{types}\"",
                self.name()
            ));
        }
        u32::try_from(opcode).map_err(|_| format!("{}.{name}: opcode too large", self.name()))
    }

    /// Each request's name and opcode, `name 2, name 3`, for the log.
    pub fn requests_line(&self) -> String {
        self.list(false)
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{} {i}", m.name()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Copies a function out of `lib`.
///
/// # Safety
/// `T` must be the exact `extern "C"` function-pointer type of `name`.
unsafe fn func<T: Copy>(lib: &Library, file: &str, name: &[u8]) -> Result<T, String> {
    // SAFETY: forwarded from the caller.
    unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| {
        let shown = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
        format!("{file} has no {shown}: {e}")
    })
}

/// An interface table out of `lib`, if it has one by that name.
fn table(lib: &'static Library, name: &[u8]) -> Option<&'static Interface> {
    // SAFETY: the symbol is a `struct wl_interface`, data in a library that
    // stays loaded (module doc); its address is taken, never called.
    let ptr = unsafe { lib.get::<*const Interface>(name) }
        .ok()
        .map(|s| *s)?;
    // SAFETY: as above: a static table for the life of the process.
    unsafe { ptr.as_ref() }
}

/// Opens the first of `names` that loads, kept for the process.
fn open(names: &[&str]) -> Result<(&'static Library, String), String> {
    let mut tried = Vec::new();
    for &name in names {
        // SAFETY: loading the TV's Wayland libraries runs only their own
        // initialisers, which have no preconditions.
        match unsafe { Library::new(name) } {
            Ok(lib) => return Ok((Box::leak(Box::new(lib)), name.to_owned())),
            Err(e) => tried.push(format!("{name}: {e}")),
        }
    }
    Err(tried.join("; "))
}

/// libwayland-client's functions and the interface tables the shell uses.
pub struct Lib {
    pub display_connect: unsafe extern "C" fn(*const c_char) -> *mut Proxy,
    pub display_disconnect: unsafe extern "C" fn(*mut Proxy),
    pub display_get_fd: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_dispatch_pending: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_roundtrip: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_flush: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_prepare_read: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_read_events: unsafe extern "C" fn(*mut Proxy) -> c_int,
    pub display_cancel_read: unsafe extern "C" fn(*mut Proxy),
    pub display_get_error: unsafe extern "C" fn(*mut Proxy) -> c_int,
    marshal_array: unsafe extern "C" fn(*mut Proxy, u32, *mut Argument),
    marshal_array_constructor:
        unsafe extern "C" fn(*mut Proxy, u32, *mut Argument, *const Interface) -> *mut Proxy,
    /// Since Wayland 1.10; without it a new proxy takes its factory's version.
    marshal_array_constructor_versioned: Option<
        unsafe extern "C" fn(*mut Proxy, u32, *mut Argument, *const Interface, u32) -> *mut Proxy,
    >,
    add_dispatcher:
        unsafe extern "C" fn(*mut Proxy, Dispatcher, *const c_void, *mut c_void) -> c_int,
    pub proxy_destroy: unsafe extern "C" fn(*mut Proxy),
    pub display: &'static Interface,
    pub registry: &'static Interface,
    pub compositor: &'static Interface,
    pub surface: &'static Interface,
    pub seat: &'static Interface,
    pub keyboard: &'static Interface,
    pub pointer: &'static Interface,
    pub output: &'static Interface,
    /// Core but deprecated: SDL-webOS and Kodi still give the surface a
    /// `wl_shell` role too.
    pub shell: Option<&'static Interface>,
    pub shell_surface: Option<&'static Interface>,
    /// LG's extensions, from `libwayland-webos-client`; none off the TV.
    pub webos_shell: Option<&'static Interface>,
    pub webos_shell_surface: Option<&'static Interface>,
    pub webos_input_manager: Option<&'static Interface>,
    /// Which files they came from, for the log.
    pub client_file: String,
    pub webos_file: Option<String>,
}

impl Lib {
    /// Loads the TV's libwayland-client and, where there is one, LG's
    /// extension library.
    pub fn load() -> Result<&'static Self, String> {
        let (lib, file) = open(&["libwayland-client.so.0", "libwayland-client.so"])?;
        let webos = open(&[
            "libwayland-webos-client.so.1",
            "libwayland-webos-client.so.0",
            "libwayland-webos-client.so",
        ])
        .ok();
        let need = |name: &[u8]| {
            table(lib, name).ok_or_else(|| {
                let shown = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
                format!("{file} has no {shown}")
            })
        };
        let webos_table = |name: &[u8]| webos.as_ref().and_then(|(lib, _)| table(lib, name));
        let f = file.as_str();
        // SAFETY: each type below is the C prototype of the named function in
        // wayland-client-core.h (Wayland 1.4 and later; the versioned
        // constructor 1.10).
        let lib = unsafe {
            Self {
                display_connect: func(lib, f, b"wl_display_connect\0")?,
                display_disconnect: func(lib, f, b"wl_display_disconnect\0")?,
                display_get_fd: func(lib, f, b"wl_display_get_fd\0")?,
                display_dispatch_pending: func(lib, f, b"wl_display_dispatch_pending\0")?,
                display_roundtrip: func(lib, f, b"wl_display_roundtrip\0")?,
                display_flush: func(lib, f, b"wl_display_flush\0")?,
                display_prepare_read: func(lib, f, b"wl_display_prepare_read\0")?,
                display_read_events: func(lib, f, b"wl_display_read_events\0")?,
                display_cancel_read: func(lib, f, b"wl_display_cancel_read\0")?,
                display_get_error: func(lib, f, b"wl_display_get_error\0")?,
                marshal_array: func(lib, f, b"wl_proxy_marshal_array\0")?,
                marshal_array_constructor: func(lib, f, b"wl_proxy_marshal_array_constructor\0")?,
                marshal_array_constructor_versioned: func(
                    lib,
                    f,
                    b"wl_proxy_marshal_array_constructor_versioned\0",
                )
                .ok(),
                add_dispatcher: func(lib, f, b"wl_proxy_add_dispatcher\0")?,
                proxy_destroy: func(lib, f, b"wl_proxy_destroy\0")?,
                display: need(b"wl_display_interface\0")?,
                registry: need(b"wl_registry_interface\0")?,
                compositor: need(b"wl_compositor_interface\0")?,
                surface: need(b"wl_surface_interface\0")?,
                seat: need(b"wl_seat_interface\0")?,
                keyboard: need(b"wl_keyboard_interface\0")?,
                pointer: need(b"wl_pointer_interface\0")?,
                output: need(b"wl_output_interface\0")?,
                shell: table(lib, b"wl_shell_interface\0"),
                shell_surface: table(lib, b"wl_shell_surface_interface\0"),
                webos_shell: webos_table(b"wl_webos_shell_interface\0"),
                webos_shell_surface: webos_table(b"wl_webos_shell_surface_interface\0"),
                webos_input_manager: webos_table(b"wl_webos_input_manager_interface\0"),
                client_file: file.clone(),
                webos_file: webos.map(|(_, file)| file),
            }
        };
        Ok(Box::leak(Box::new(lib)))
    }

    /// Sends a request.
    ///
    /// # Safety
    /// `proxy` is live, `opcode` one of its interface's requests, and `args`
    /// match that request's signature with every string and object live.
    pub unsafe fn send(&self, proxy: *mut Proxy, opcode: u32, args: &mut [Argument]) {
        // SAFETY: forwarded from the caller.
        unsafe { (self.marshal_array)(proxy, opcode, args.as_mut_ptr()) };
    }

    /// Sends a request that makes a new object of `iface`, at `version`
    /// where the library can say it; its new-id argument in `args` is a
    /// placeholder that libwayland fills in. Null if libwayland refused.
    ///
    /// # Safety
    /// As `send`, and `iface` is the new object's interface.
    pub unsafe fn create(
        &self,
        proxy: *mut Proxy,
        opcode: u32,
        args: &mut [Argument],
        iface: &Interface,
        version: u32,
    ) -> *mut Proxy {
        // SAFETY: forwarded from the caller.
        unsafe {
            match self.marshal_array_constructor_versioned {
                Some(versioned) => versioned(proxy, opcode, args.as_mut_ptr(), iface, version),
                None => (self.marshal_array_constructor)(proxy, opcode, args.as_mut_ptr(), iface),
            }
        }
    }

    /// Routes `proxy`'s events to `dispatcher` with `data`.
    ///
    /// # Safety
    /// `proxy` is live and has no listener or dispatcher yet; `data` lives
    /// as long as the proxy, and `dispatcher` reads it as what it is.
    pub unsafe fn dispatch_to(
        &self,
        proxy: *mut Proxy,
        dispatcher: Dispatcher,
        data: *const c_void,
    ) -> Result<(), String> {
        // SAFETY: forwarded from the caller.
        match unsafe { (self.add_dispatcher)(proxy, dispatcher, data, std::ptr::null_mut()) } {
            0 => Ok(()),
            _ => Err("wl_proxy_add_dispatcher refused".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn msg(name: &'static CStr, signature: &'static CStr) -> Message {
        Message {
            name: name.as_ptr(),
            signature: signature.as_ptr(),
            types: std::ptr::null(),
        }
    }

    /// A webOS shell surface table laid out as LG's TV build is said to be
    /// (research of 2026-11-05): `set_size` at 4 where OSE has `set_addon`.
    fn lg_surface(methods: &[Message]) -> Interface {
        Interface {
            name: c"wl_webos_shell_surface".as_ptr(),
            version: 1,
            method_count: methods.len() as c_int,
            methods: methods.as_ptr(),
            event_count: 0,
            events: std::ptr::null(),
        }
    }

    #[test]
    fn plain_signatures_drop_versions_and_null_marks() {
        assert_eq!(plain("2u"), "u");
        assert_eq!(plain("?ss"), "ss");
        assert_eq!(plain("usun"), "usun");
        assert_eq!(plain("3?o"), "o");
        assert_eq!(plain(""), "");
    }

    #[test]
    fn requests_are_found_by_name_and_checked_by_signature() {
        let methods = [
            msg(c"set_location_hint", c"u"),
            msg(c"set_state", c"u"),
            msg(c"set_property", c"ss"),
            msg(c"set_key_mask", c"u"),
            msg(c"set_size", c"ii"),
        ];
        let table = lg_surface(&methods);
        assert_eq!(table.name(), "wl_webos_shell_surface");
        assert_eq!(table.request("set_property", "ss"), Ok(2));
        assert_eq!(table.request("set_state", "u"), Ok(1));
        assert_eq!(table.request("set_size", "ii"), Ok(4));
        assert_eq!(
            table.request("set_addon", "s"),
            Err("wl_webos_shell_surface has no request set_addon".to_owned())
        );
        assert_eq!(
            table.request("set_property", "s"),
            Err("wl_webos_shell_surface.set_property takes \"ss\" here, not \"s\"".to_owned())
        );
        assert_eq!(
            table.requests_line(),
            "set_location_hint 0, set_state 1, set_property 2, set_key_mask 3, set_size 4"
        );
    }

    #[test]
    fn a_reordered_table_moves_the_opcodes_with_it() {
        let methods = [msg(c"set_property", c"ss"), msg(c"set_state", c"2u")];
        let table = lg_surface(&methods);
        assert_eq!(table.request("set_property", "ss"), Ok(0));
        assert_eq!(table.request("set_state", "u"), Ok(1));
    }

    #[test]
    fn an_empty_table_has_nothing() {
        let table = lg_surface(&[]);
        assert!(table.request("set_state", "u").is_err());
        assert_eq!(table.requests_line(), "");
    }

    #[test]
    fn events_are_read_by_their_signature() {
        let text = c"wl_seat";
        let mut bytes = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut array = Array {
            size: 8,
            alloc: 8,
            data: bytes.as_mut_ptr().cast(),
        };
        let args = [
            Argument::uint(7),
            Argument::string(text),
            Argument::int(-3),
            Argument { i: 0x180 },
            Argument { a: &raw mut array },
            Argument::NULL,
            Argument { i: 9 },
        ];
        // SAFETY: one live argument per type.
        let read = unsafe { decode("usif?a?sh", args.as_ptr()) };
        assert_eq!(
            read,
            [
                Arg::Uint(7),
                Arg::Str(Some("wl_seat".to_owned())),
                Arg::Int(-3),
                Arg::Fixed(0x180),
                Arg::Array(vec![1, 2, 3, 4, 5, 6, 7, 8]),
                Arg::Str(None),
                Arg::Fd(9),
            ]
        );
        assert_eq!(read[3].whole(), Some(1));
        assert_eq!(read[0].uint(), Some(7));
        assert_eq!(read[2].uint(), None);
        assert_eq!(read[1].str(), Some("wl_seat"));
    }

    #[test]
    fn an_unknown_type_stops_the_reading() {
        let args = [Argument::uint(1), Argument::uint(2)];
        // SAFETY: the reader stops at 'x', before the second argument.
        let read = unsafe { decode("uxu", args.as_ptr()) };
        assert_eq!(read, [Arg::Uint(1)]);
    }

    #[test]
    fn an_argument_is_one_pointer_wide() {
        assert_eq!(size_of::<Argument>(), size_of::<*const c_void>());
    }
}
