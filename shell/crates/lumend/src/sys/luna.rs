//! luna through `libluna-service2.so.3`, loaded at run time as the shell and
//! lumen-ctl load it (lumen-ctl's `sys/luna.rs`, whose FFI this follows).
//!
//! The hub judges a client by its executable and the role files that name
//! it, not by its uid (decision 14), so `lumend` registers under a name of
//! its own, `io.github.sunnyside007.lumend`, which its role allows for
//! `/var/lib/lumen/lumend/lumend` only. The role offers no method: `lumend`
//! calls out and is never called.
//!
//! One thread owns the client and its glib context. Other threads hand it
//! work through a channel and wake it with `g_main_context_wakeup`, which
//! glib makes safe from any thread; every `LS*` call happens on that one
//! thread. A call's reply comes back on a channel of its own; each
//! subscription's messages go to the sink the bus was started with.
//!
//! The library is loaded once and never unloaded (`Box::leak`): contexts
//! the library holds point into it.

use std::convert::Infallible;
use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use libloading::Library;
use serde_json::Value;

/// luna-service2 (3.21.2 on the B9), and the glib its replies arrive through.
const LIBRARY: &str = "libluna-service2.so.3";
const GLIB: &str = "libglib-2.0.so.0";
/// The largest reply kept: `listLaunchPoints` is the largest lumend reads.
pub const MAX_REPLY: usize = 4 << 20;
/// How long the bus thread sleeps between turns while a call waits.
const POLL: Duration = Duration::from_millis(1);

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

/// The functions of libluna-service2 and glib that lumend calls.
struct Api {
    error_init: unsafe extern "C" fn(*mut LsError) -> bool,
    error_free: unsafe extern "C" fn(*mut LsError),
    register: unsafe extern "C" fn(*const c_char, *mut *mut LsHandle, *mut LsError) -> bool,
    attach: unsafe extern "C" fn(*mut LsHandle, *mut GMainContext, *mut LsError) -> bool,
    call_one_reply: CallFn,
    call: CallFn,
    cancel: unsafe extern "C" fn(*mut LsHandle, LsMessageToken, *mut LsError) -> bool,
    payload: unsafe extern "C" fn(*mut LsMessage) -> *const c_char,
    context_new: unsafe extern "C" fn() -> *mut GMainContext,
    context_iteration: unsafe extern "C" fn(*mut GMainContext, c_int) -> c_int,
    context_wakeup: unsafe extern "C" fn(*mut GMainContext),
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
    fn load() -> Result<&'static Self, String> {
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
        let api = unsafe {
            Self {
                error_init: sym(&luna, l, b"LSErrorInit\0")?,
                error_free: sym(&luna, l, b"LSErrorFree\0")?,
                register: sym(&luna, l, b"LSRegister\0")?,
                attach: sym(&luna, l, b"LSGmainContextAttach\0")?,
                call_one_reply: sym(&luna, l, b"LSCallOneReply\0")?,
                call: sym(&luna, l, b"LSCall\0")?,
                cancel: sym(&luna, l, b"LSCallCancel\0")?,
                payload: sym(&luna, l, b"LSMessageGetPayload\0")?,
                context_new: sym(&glib, g, b"g_main_context_new\0")?,
                context_iteration: sym(&glib, g, b"g_main_context_iteration\0")?,
                context_wakeup: sym(&glib, g, b"g_main_context_wakeup\0")?,
                _libs: vec![luna, glib],
            }
        };
        Ok(Box::leak(Box::new(api)))
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

    /// A new context with a client registered as `name` attached to it.
    fn client(&'static self, name: &str) -> Result<(*mut LsHandle, *mut GMainContext), String> {
        let name = CString::new(name).map_err(|_| "a name with a NUL byte".to_owned())?;
        // SAFETY: no arguments; the new context is owned here and never
        // freed (the bus and the stand-in keep theirs for life).
        let context = unsafe { (self.context_new)() };
        if context.is_null() {
            return Err("no glib context".to_owned());
        }
        let mut handle = std::ptr::null_mut();
        self.checked("registering on the bus", |error| {
            // SAFETY: the name is NUL-terminated and outlives the call;
            // `handle` and `error` are valid for the library to write.
            unsafe { (self.register)(name.as_ptr(), &mut handle, error) }
        })?;
        self.checked("attaching to glib", |error| {
            // SAFETY: the handle and the context are live and both ours.
            unsafe { (self.attach)(handle, context, error) }
        })?;
        Ok((handle, context))
    }
}

/// Where a subscription's messages go: its id, and the message or why it
/// failed.
pub type Sink = Arc<dyn Fn(u32, Result<Value, String>) + Send + Sync>;

/// The bus thread's work.
enum Cmd {
    Call {
        uri: CString,
        payload: CString,
        timeout: Duration,
        reply: Sender<Result<Value, String>>,
    },
    Subscribe {
        id: u32,
        uri: CString,
        payload: CString,
    },
}

/// The context pointer, for `g_main_context_wakeup` from other threads.
struct Context(*mut GMainContext);

// SAFETY: the context is never freed, and the only function called with it
// outside the bus thread is g_main_context_wakeup, which glib documents as
// safe to call from any thread.
unsafe impl Send for Context {}
// SAFETY: as above: shared use is wakeup only.
unsafe impl Sync for Context {}

/// A handle on the bus thread: cheap to share between threads.
pub struct Luna {
    api: &'static Api,
    tx: Sender<Cmd>,
    context: Context,
}

/// Where `on_reply` leaves a call's first reply.
struct Slot {
    api: &'static Api,
    reply: Option<Result<Vec<u8>, String>>,
}

/// The `LSFilterFunc` of a call: copies the first reply into its slot.
///
/// # Safety
/// `ctx` is a live `Slot` that only the bus thread touches, and `message` a
/// live message.
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
/// library has it.
struct Watched {
    api: &'static Api,
    id: u32,
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
    let message = unsafe { watched.api.read(message) }.and_then(|bytes| parse(&bytes));
    (watched.sink)(watched.id, message);
    true
}

fn parse(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("unreadable reply: {e}"))
}

/// A call in flight, owned by the bus thread.
struct Pending {
    token: LsMessageToken,
    slot: *mut Slot,
    deadline: Instant,
    reply: Sender<Result<Value, String>>,
}

impl Luna {
    /// Loads the library, registers `name` on a thread of its own and
    /// returns once it is registered, or why it couldn't be.
    pub fn start(name: &'static str, sink: Sink) -> Result<Self, String> {
        let api = Api::load()?;
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Context, String>>();
        std::thread::Builder::new()
            .name("luna".to_owned())
            .stack_size(256 << 10)
            .spawn(move || match api.client(name) {
                Ok((handle, context)) => {
                    if ready_tx.send(Ok(Context(context))).is_ok() {
                        bus_thread(api, handle, context, &rx, &sink);
                    }
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| format!("no thread for luna: {e}"))?;
        let context = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "the bus thread never answered".to_owned())??;
        Ok(Self { api, tx, context })
    }

    fn send(&self, cmd: Cmd) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "the bus thread is gone".to_owned())?;
        // SAFETY: the context lives for the process, and wakeup is safe from
        // any thread (see `Context`).
        unsafe { (self.api.context_wakeup)(self.context.0) };
        Ok(())
    }

    /// One call, waiting up to `timeout` for its reply. `Err` is the bus
    /// failing; LG's refusals are replies with `returnValue` false.
    pub fn call(&self, uri: &str, payload: &Value, timeout: Duration) -> Result<Value, String> {
        let uri = CString::new(uri).map_err(|_| "a URI with a NUL byte".to_owned())?;
        // serde_json writes a NUL inside a string as \u0000, so this holds.
        let payload = CString::new(payload.to_string())
            .map_err(|_| "a payload with a NUL byte".to_owned())?;
        let (reply, rx) = mpsc::channel();
        self.send(Cmd::Call {
            uri,
            payload,
            timeout,
            reply,
        })?;
        // The bus thread answers by the deadline; a little more covers its
        // turn.
        match rx.recv_timeout(timeout + Duration::from_secs(1)) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                Err(format!("no reply in {} ms", timeout.as_millis()))
            }
            Err(RecvTimeoutError::Disconnected) => Err("the bus thread is gone".to_owned()),
        }
    }

    /// Opens a subscription whose messages, the first included, go to the
    /// sink with `id`. A refusal arrives there too.
    pub fn subscribe(&self, id: u32, uri: &str, payload: &Value) -> Result<(), String> {
        let uri = CString::new(uri).map_err(|_| "a URI with a NUL byte".to_owned())?;
        let payload = CString::new(payload.to_string())
            .map_err(|_| "a payload with a NUL byte".to_owned())?;
        self.send(Cmd::Subscribe { id, uri, payload })
    }
}

/// The bus thread: takes work, dispatches the context, hands replies back.
fn bus_thread(
    api: &'static Api,
    handle: *mut LsHandle,
    context: *mut GMainContext,
    rx: &Receiver<Cmd>,
    sink: &Sink,
) {
    let mut pending: Vec<Pending> = Vec::new();
    loop {
        loop {
            match rx.try_recv() {
                Ok(Cmd::Call {
                    uri,
                    payload,
                    timeout,
                    reply,
                }) => {
                    let slot = Box::into_raw(Box::new(Slot { api, reply: None }));
                    let mut token: LsMessageToken = 0;
                    let sent = api.checked("calling", |error| {
                        // SAFETY: the handle is live and this thread's; the
                        // library copies the URI and the payload; `on_reply`
                        // is an LSFilterFunc whose ctx, the slot, stays
                        // valid until the call is answered or cancelled.
                        unsafe {
                            (api.call_one_reply)(
                                handle,
                                uri.as_ptr(),
                                payload.as_ptr(),
                                on_reply,
                                slot.cast(),
                                &mut token,
                                error,
                            )
                        }
                    });
                    match sent {
                        Ok(()) => pending.push(Pending {
                            token,
                            slot,
                            deadline: Instant::now() + timeout,
                            reply,
                        }),
                        Err(e) => {
                            // SAFETY: no call was made, so nothing else
                            // holds the slot.
                            drop(unsafe { Box::from_raw(slot) });
                            let _ = reply.send(Err(e));
                        }
                    }
                }
                Ok(Cmd::Subscribe { id, uri, payload }) => {
                    let ctx = Box::into_raw(Box::new(Watched {
                        api,
                        id,
                        sink: Arc::clone(sink),
                    }));
                    let mut token: LsMessageToken = 0;
                    let sent = api.checked("subscribing", |error| {
                        // SAFETY: as for a call; `on_message`'s ctx is
                        // never freed once the library has it.
                        unsafe {
                            (api.call)(
                                handle,
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
                        // SAFETY: the library refused the call and kept no
                        // pointer to the ctx.
                        drop(unsafe { Box::from_raw(ctx) });
                        sink(id, Err(e));
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                // The handle is gone: the process is ending.
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if pending.is_empty() {
            // SAFETY: the client's own context, dispatched from this thread
            // only; 1 blocks until a message comes or another thread wakes
            // it.
            unsafe { (api.context_iteration)(context, 1) };
        } else {
            // SAFETY: as above; 0 doesn't block, so deadlines are kept.
            if unsafe { (api.context_iteration)(context, 0) } == 0 {
                std::thread::sleep(POLL);
            }
        }
        let now = Instant::now();
        pending.retain(|p| {
            // SAFETY: only this thread touches the slot, and the library
            // writes it only inside context_iteration, which has returned.
            let answered = unsafe { (*p.slot).reply.take() };
            if let Some(result) = answered {
                // SAFETY: a one-reply call is over once its reply came, so
                // the library no longer holds the slot.
                drop(unsafe { Box::from_raw(p.slot) });
                let _ = p.reply.send(result.and_then(|b| parse(&b)));
                return false;
            }
            if now < p.deadline {
                return true;
            }
            let cancelled = api.checked("cancelling", |error| {
                // SAFETY: the handle is live, and the token its open call's.
                unsafe { (api.cancel)(handle, p.token, error) }
            });
            if cancelled.is_ok() {
                // SAFETY: the call is cancelled, so the library no longer
                // holds the slot.
                drop(unsafe { Box::from_raw(p.slot) });
            }
            // Otherwise the slot is leaked rather than freed under the
            // library.
            let _ = p.reply.send(Err("no reply in time".to_owned()));
            false
        });
    }
}

/// The stand-in's whole life on the bus: registers `name`, which offers no
/// method (so every call gets the library's "unknown method" at once), and
/// dispatches until the process is killed.
pub fn hold_name(name: &str) -> Result<Infallible, String> {
    let api = Api::load()?;
    let (_handle, context) = api.client(name)?;
    loop {
        // SAFETY: the context is this thread's own, and 1 blocks until there
        // is something to dispatch.
        unsafe { (api.context_iteration)(context, 1) };
    }
}
