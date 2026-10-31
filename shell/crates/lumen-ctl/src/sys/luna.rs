//! `--bus library`: luna through `libluna-service2.so.3`, loaded at run time
//! as the shell does (lumen-shell's `luna.rs` and `watch.rs` on
//! claude/shell-timing, whose FFI this follows).
//!
//! * A call registers an anonymous client on a glib context of its own, makes
//!   one `LSCallOneReply`, dispatches the context until the reply comes or 5 s
//!   pass, and unregisters: the one process a call `luna-send` would have
//!   been, without the process.
//! * The subscriptions live on one thread with one client and one context,
//!   for the life of the process: each message goes to the `Sink`.
//!
//! The library is loaded once and never unloaded (`Box::leak`): contexts the
//! library holds point into it, and the process ends with `_exit`.

use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::time::{Duration, Instant};

use libloading::Library;
use serde_json::Value;

use crate::bus::{Bus, CALL_TIMEOUT, MAX_REPLY, Sink};

/// luna-service2 (3.21.2 on the B9), and the glib its replies arrive through.
const LIBRARY: &str = "libluna-service2.so.3";
const GLIB: &str = "libglib-2.0.so.0";
/// How long a call waiting for its reply sleeps when nothing came in.
const POLL: Duration = Duration::from_millis(1);

// libluna-service2's types, as lunaservice.h declares them.
type LsHandle = c_void;
type LsMessage = c_void;
type LsMessageToken = c_ulong;
type GMainContext = c_void;
type FilterFunc = unsafe extern "C" fn(*mut LsHandle, *mut LsMessage, *mut c_void) -> bool;
/// `LSCall` and `LSCallOneReply` have the same prototype.
type CallFn = unsafe extern "C" fn(
    *mut LsHandle,
    *const c_char,
    *const c_char,
    FilterFunc,
    *mut c_void,
    *mut LsMessageToken,
    *mut LsError,
) -> bool;

/// `LSError`: initialised, filled in and freed by the library.
#[repr(C)]
struct LsError {
    error_code: c_int,
    message: *mut c_char,
    file: *const c_char,
    line: c_int,
    func: *const c_char,
    padding: *mut c_void,
    magic: c_ulong,
}

impl LsError {
    fn empty() -> Self {
        Self {
            error_code: 0,
            message: std::ptr::null_mut(),
            file: std::ptr::null(),
            line: 0,
            func: std::ptr::null(),
            padding: std::ptr::null_mut(),
            magic: 0,
        }
    }

    /// The library's message, on one line, and its code.
    fn text(&self) -> String {
        if self.message.is_null() {
            return format!("error {}", self.error_code);
        }
        // SAFETY: a set LSError's message is a NUL-terminated string, owned
        // by the error until LSErrorFree.
        let message = unsafe { CStr::from_ptr(self.message) }.to_string_lossy();
        let message: String = message
            .trim()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        format!("{message} (error {})", self.error_code)
    }
}

/// The functions of libluna-service2 and glib that lumen-ctl calls.
struct Api {
    error_init: unsafe extern "C" fn(*mut LsError) -> bool,
    error_free: unsafe extern "C" fn(*mut LsError),
    register: unsafe extern "C" fn(*const c_char, *mut *mut LsHandle, *mut LsError) -> bool,
    unregister: unsafe extern "C" fn(*mut LsHandle, *mut LsError) -> bool,
    attach: unsafe extern "C" fn(*mut LsHandle, *mut GMainContext, *mut LsError) -> bool,
    /// `LSCallOneReply`: one request, one reply.
    call_one_reply: CallFn,
    /// `LSCall`: a subscription, answered until it is cancelled or the client
    /// unregisters.
    call: CallFn,
    cancel: unsafe extern "C" fn(*mut LsHandle, LsMessageToken, *mut LsError) -> bool,
    payload: unsafe extern "C" fn(*mut LsMessage) -> *const c_char,
    context_new: unsafe extern "C" fn() -> *mut GMainContext,
    context_iteration: unsafe extern "C" fn(*mut GMainContext, c_int) -> c_int,
    context_unref: unsafe extern "C" fn(*mut GMainContext),
    /// Held so every function above stays valid.
    _libs: Vec<Library>,
}

/// Copies a symbol out of `lib`, which is `file`, as a function pointer.
///
/// # Safety
/// `T` must be the exact `extern "C"` function-pointer type of `name`.
unsafe fn sym<T: Copy>(lib: &Library, file: &str, name: &[u8]) -> Result<T, String> {
    // SAFETY: forwarded from the caller: T matches the symbol's real type.
    unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| {
        let shown = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
        format!("{file} has no {shown}: {e}")
    })
}

impl Api {
    fn load() -> Result<Self, String> {
        let open = |file: &str| {
            // SAFETY: loading either library runs only its own initialisers,
            // which have no preconditions. glib is luna's own dependency, so
            // it is already loaded by then.
            unsafe { Library::new(file) }.map_err(|e| format!("{file}: {e}"))
        };
        let luna = open(LIBRARY)?;
        let glib = open(GLIB)?;
        let (l, g) = (LIBRARY, GLIB);
        // SAFETY: each type below is the C prototype of the named function,
        // from luna-service2 3.21's lunaservice.h and glib's gmain.h.
        unsafe {
            Ok(Self {
                error_init: sym(&luna, l, b"LSErrorInit\0")?,
                error_free: sym(&luna, l, b"LSErrorFree\0")?,
                register: sym(&luna, l, b"LSRegister\0")?,
                unregister: sym(&luna, l, b"LSUnregister\0")?,
                attach: sym(&luna, l, b"LSGmainContextAttach\0")?,
                call_one_reply: sym(&luna, l, b"LSCallOneReply\0")?,
                call: sym(&luna, l, b"LSCall\0")?,
                cancel: sym(&luna, l, b"LSCallCancel\0")?,
                payload: sym(&luna, l, b"LSMessageGetPayload\0")?,
                context_new: sym(&glib, g, b"g_main_context_new\0")?,
                context_iteration: sym(&glib, g, b"g_main_context_iteration\0")?,
                context_unref: sym(&glib, g, b"g_main_context_unref\0")?,
                _libs: vec![luna, glib],
            })
        }
    }

    /// Runs one library function with a fresh `LSError`, turning a failure
    /// into the library's message.
    fn checked(&self, what: &str, op: impl FnOnce(*mut LsError) -> bool) -> Result<(), String> {
        let mut error = LsError::empty();
        // SAFETY: `error` is an LSError for the library to initialise.
        unsafe { (self.error_init)(&mut error) };
        let result = if op(&mut error) {
            Ok(())
        } else {
            Err(format!("{what} failed: {}", error.text()))
        };
        // SAFETY: initialised above; this frees its message, if it has one.
        unsafe { (self.error_free)(&mut error) };
        result
    }

    /// The payload of a live message, copied, or why not.
    ///
    /// # Safety
    /// `message` is a live message, as an `LSFilterFunc` is given.
    unsafe fn read(&self, message: *mut LsMessage) -> Result<Vec<u8>, String> {
        // SAFETY: forwarded from the caller.
        let payload = unsafe { (self.payload)(message) };
        if payload.is_null() {
            return Err("a reply without a payload".to_owned());
        }
        // SAFETY: a message's payload is a NUL-terminated string that lives
        // as long as the message.
        let bytes = unsafe { CStr::from_ptr(payload) }.to_bytes();
        if bytes.len() > MAX_REPLY {
            Err(format!("a reply over {MAX_REPLY} bytes"))
        } else {
            Ok(bytes.to_vec())
        }
    }
}

/// An anonymous client on the bus, dispatched on a glib context of its own,
/// and unregistered on drop. Not `Send`: one thread uses it.
struct Client {
    api: &'static Api,
    handle: *mut LsHandle,
    context: *mut GMainContext,
}

impl Client {
    fn register(api: &'static Api) -> Result<Self, String> {
        // SAFETY: no arguments; the new context is owned here.
        let context = unsafe { (api.context_new)() };
        if context.is_null() {
            return Err("no glib context".to_owned());
        }
        let mut handle = std::ptr::null_mut();
        let registered = api.checked("registering on the bus", |error| {
            // SAFETY: a null name asks for an anonymous client; `handle` and
            // `error` are valid for the library to write.
            unsafe { (api.register)(std::ptr::null(), &mut handle, error) }
        });
        if let Err(e) = registered {
            // SAFETY: the context is ours, and nothing is attached to it.
            unsafe { (api.context_unref)(context) };
            return Err(e);
        }
        let client = Self {
            api,
            handle,
            context,
        };
        api.checked("attaching to glib", |error| {
            // SAFETY: the handle and the context are live, and both this
            // client's.
            unsafe { (api.attach)(handle, context, error) }
        })?;
        Ok(client)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let api = self.api;
        let unregistered = api.checked("unregistering", |error| {
            // SAFETY: the handle is live, and used from this thread only.
            unsafe { (api.unregister)(self.handle, error) }
        });
        if unregistered.is_ok() {
            // SAFETY: unregistering detached the handle, so nothing uses the
            // context any more.
            unsafe { (api.context_unref)(self.context) };
        }
        // Otherwise the handle may still be attached: the context is leaked.
    }
}

/// Where `on_reply` leaves a call's first reply, through the call's `ctx`.
struct Slot {
    api: &'static Api,
    reply: Option<Result<Vec<u8>, String>>,
}

/// The `LSFilterFunc` of a call: copies the first reply into its slot.
///
/// # Safety
/// `ctx` is a live `Slot` that nothing else touches during the call, and
/// `message` a live message.
unsafe extern "C" fn on_reply(_: *mut LsHandle, message: *mut LsMessage, ctx: *mut c_void) -> bool {
    // SAFETY: forwarded from the caller.
    let slot = unsafe { &mut *ctx.cast::<Slot>() };
    if slot.reply.is_none() {
        // SAFETY: forwarded from the caller.
        slot.reply = Some(unsafe { slot.api.read(message) });
    }
    true
}

/// What a subscription's callback gets as its `ctx`: never freed once the
/// library has it, as lumen-shell's watch does.
struct Watched {
    api: &'static Api,
    index: usize,
    sink: Sink,
}

/// The `LSFilterFunc` of the subscriptions: hands each message on.
///
/// # Safety
/// `ctx` is a live `Watched` for the process's life, and `message` a live
/// message.
unsafe extern "C" fn on_message(
    _: *mut LsHandle,
    message: *mut LsMessage,
    ctx: *mut c_void,
) -> bool {
    // SAFETY: forwarded from the caller.
    let watched = unsafe { &*ctx.cast::<Watched>() };
    // SAFETY: forwarded from the caller.
    let message = unsafe { watched.api.read(message) }.and_then(|bytes| {
        serde_json::from_slice(&bytes).map_err(|e| format!("unreadable message: {e}"))
    });
    (watched.sink)(watched.index, message);
    true
}

pub struct LibraryBus {
    api: &'static Api,
}

impl LibraryBus {
    /// Loads the library and registers once, so a hub that refuses an unknown
    /// root client says so here.
    pub fn open() -> Result<Self, String> {
        let api: &'static Api = Box::leak(Box::new(Api::load()?));
        drop(Client::register(api)?);
        Ok(Self { api })
    }

    /// One call, waiting up to `timeout` for its first reply. With
    /// `subscribe` the call is an `LSCall`, cancelled after that reply:
    /// `probe` sees whether a method answers a subscription, and leaves none
    /// open.
    pub fn call_for(
        &self,
        uri: &str,
        payload: &Value,
        timeout: Duration,
        subscribe: bool,
    ) -> Result<Value, String> {
        let api = self.api;
        let uri = CString::new(uri).map_err(|_| "a URI with a NUL byte".to_owned())?;
        // serde_json writes a NUL inside a string as \u0000, so this holds.
        let payload = CString::new(payload.to_string())
            .map_err(|_| "a payload with a NUL byte".to_owned())?;
        let client = Client::register(api)?;
        let slot = Box::into_raw(Box::new(Slot { api, reply: None }));
        let mut token: LsMessageToken = 0;
        let call = if subscribe {
            api.call
        } else {
            api.call_one_reply
        };
        let sent = api.checked("calling", |error| {
            // SAFETY: the handle is live; the library copies the URI and the
            // payload; `on_reply` is an LSFilterFunc, and its `ctx`, the
            // slot, stays valid until the call is done or cancelled.
            unsafe {
                call(
                    client.handle,
                    uri.as_ptr(),
                    payload.as_ptr(),
                    on_reply,
                    slot.cast(),
                    &mut token,
                    error,
                )
            }
        });
        if let Err(e) = sent {
            // SAFETY: no call was made, so nothing else holds the slot.
            drop(unsafe { Box::from_raw(slot) });
            return Err(e);
        }
        // SAFETY: the slot is valid until it is taken back below, and only
        // `on_reply` writes to it, on this thread, inside `context_iteration`.
        let answered = || unsafe { (*slot).reply.is_some() };
        let deadline = Instant::now() + timeout;
        while !answered() && Instant::now() < deadline {
            // SAFETY: the client's own context, dispatched from this thread
            // only; 0 asks it not to block.
            if unsafe { (api.context_iteration)(client.context, 0) } == 0 {
                std::thread::sleep(POLL);
            }
        }
        let no_reply = || format!("no reply in {} ms", timeout.as_millis());
        // A one-reply call that answered is done; anything else is still open
        // and is cancelled before the slot goes.
        if subscribe || !answered() {
            let cancelled = api.checked("cancelling", |error| {
                // SAFETY: the handle is live, and the token its open call's.
                unsafe { (api.cancel)(client.handle, token, error) }
            });
            if cancelled.is_err() {
                // The call is still open and could yet write to the slot,
                // through the client: both are leaked rather than freed
                // under it.
                std::mem::forget(client);
                // SAFETY: read only, on this thread; the library writes to
                // the slot only inside context_iteration, not called again.
                let got = unsafe { (*slot).reply.clone() };
                return parse(got.unwrap_or_else(|| Err(no_reply())));
            }
        }
        drop(client);
        // SAFETY: the call was answered or cancelled and its client
        // unregistered, so the library no longer holds the slot.
        let slot = unsafe { Box::from_raw(slot) };
        parse(slot.reply.unwrap_or_else(|| Err(no_reply())))
    }
}

fn parse(reply: Result<Vec<u8>, String>) -> Result<Value, String> {
    serde_json::from_slice(&reply?).map_err(|e| format!("unreadable reply: {e}"))
}

/// The subscriptions' thread: one client, every subscription, then dispatch
/// for the life of the process.
fn watch_thread(api: &'static Api, subs: Vec<(&'static str, Value)>, sink: Sink) {
    let client = match Client::register(api) {
        Ok(client) => client,
        Err(e) => {
            for i in 0..subs.len() {
                sink(i, Err(e.clone()));
            }
            return;
        }
    };
    for (index, (uri, payload)) in subs.into_iter().enumerate() {
        let (Ok(uri), Ok(payload)) = (CString::new(uri), CString::new(payload.to_string())) else {
            sink(index, Err("a NUL byte in the request".to_owned()));
            continue;
        };
        let ctx = Box::into_raw(Box::new(Watched {
            api,
            index,
            sink: std::sync::Arc::clone(&sink),
        }));
        let mut token = 0;
        let sent = api.checked("subscribing", |error| {
            // SAFETY: the handle is live; the library copies the URI and the
            // payload; `on_message` is an LSFilterFunc whose `ctx` is never
            // freed once the library has it.
            unsafe {
                (api.call)(
                    client.handle,
                    uri.as_ptr(),
                    payload.as_ptr(),
                    on_message,
                    ctx.cast(),
                    &mut token,
                    error,
                )
            }
        });
        if let Err(e) = sent {
            // SAFETY: the library refused the call, so it kept no pointer to
            // the ctx and nothing else holds it.
            drop(unsafe { Box::from_raw(ctx) });
            sink(index, Err(e));
        }
    }
    loop {
        // SAFETY: the client's own context, dispatched from this thread only;
        // 1 lets it block until a message comes.
        unsafe { (api.context_iteration)(client.context, 1) };
    }
}

impl Bus for LibraryBus {
    fn name(&self) -> &'static str {
        "library"
    }

    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        self.call_for(uri, payload, CALL_TIMEOUT, false)
    }

    fn watch(&self, subs: Vec<(&'static str, Value)>, sink: Sink) -> Option<Result<(), String>> {
        let api = self.api;
        let spawned = std::thread::Builder::new()
            .name("luna-watch".to_owned())
            .stack_size(256 << 10)
            .spawn(move || watch_thread(api, subs, sink));
        Some(
            spawned
                .map(drop)
                .map_err(|e| format!("no thread for luna: {e}")),
        )
    }
}
