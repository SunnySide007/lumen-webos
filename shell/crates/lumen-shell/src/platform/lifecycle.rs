//! The shell's own lifecycle registration with the app manager (SAM), the
//! one LG's SDL holds on the SDL path (decision 15; research of 2026-11-05,
//! "App lifecycle").
//!
//! With Lumen as the TV's Home and in front, a Home press becomes SAM's
//! relaunch of Lumen, sent to whoever holds Lumen's registration. Through
//! LG's SDL that only raised the window; here the shell holds it, so the
//! relaunch reaches the home loop as `Event::Relaunch`, and with it every
//! pause and close.
//!
//! * **Version 1** (no `nativeLifeCycleInterfaceVersion` in appinfo, as the
//!   IPK ships): `registerNativeApp`; a relaunch comes as
//!   `{"message":"relaunch","parameters":{…}}`.
//! * **Version 2** (`make ipk LIFECYCLE=2`, which SAM shows by launching
//!   with an envelope that names `registerApp`): `registerApp`; events
//!   `relaunch` (with `reason` and `parameters`), `pause` (`keepAlive`) and
//!   `close`. Only a registered version 2 app survives a keep-alive pause.
//!
//! The registration is a subscription held by a thread of its own, on a bus
//! client named with the app's id (SAM finds the caller by it; an anonymous
//! client gets "cannot find caller id"). What it hears goes to a `Sink`,
//! which wakes the home loop's wait. The pure part (`parse`) is tested with
//! the shapes SAM sends; the thread is the thin FFI around it, like the luna
//! watch's.

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::info;
use crate::luna::{Api, Client, GMainContext, LsHandle, LsMessage};
use crate::watch::clean_text;

const REGISTER_APP: &str = "luna://com.webos.applicationManager/registerApp";
const REGISTER_NATIVE_APP: &str = "luna://com.webos.applicationManager/registerNativeApp";
/// The largest message read; SAM's are a few hundred bytes.
const MAX_MESSAGE: usize = 64 << 10;
/// How long the loop waits for the thread at exit.
const STOP_WAIT: Duration = Duration::from_secs(2);
/// The most events kept for the loop: a burst of relaunches beyond this
/// says nothing more.
const MAX_QUEUED: usize = 32;

/// The app manager's registration method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Version 2: `registerApp`.
    RegisterApp,
    /// Version 1: `registerNativeApp`.
    RegisterNativeApp,
}

impl Method {
    /// The method SAM names in a version 2 envelope's `interfaceMethod`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "registerApp" => Some(Self::RegisterApp),
            "registerNativeApp" => Some(Self::RegisterNativeApp),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::RegisterApp => "registerApp",
            Self::RegisterNativeApp => "registerNativeApp",
        }
    }

    pub const fn uri(self) -> &'static str {
        match self {
            Self::RegisterApp => REGISTER_APP,
            Self::RegisterNativeApp => REGISTER_NATIVE_APP,
        }
    }

    /// The lifecycle interface version it belongs to.
    pub const fn version(self) -> u8 {
        match self {
            Self::RegisterApp => 2,
            Self::RegisterNativeApp => 1,
        }
    }
}

/// What the app manager said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeEvent {
    /// The registration was taken.
    Registered,
    /// Launched again while running: `launch_type` is the parameters'
    /// `launchType` (`homeKey` for a Home press), `reason` SAM's own.
    Relaunch {
        reason: Option<String>,
        launch_type: Option<String>,
    },
    /// Paused (version 2, `keepAlive`): to the background, alive.
    Pause { reason: Option<String> },
    /// To close (version 2): SAM kills the app if it does not leave soon.
    Close { reason: Option<String> },
    /// The registration was refused, or a message said `returnValue: false`.
    Refused(String),
    /// Anything else, named for the log.
    Other(String),
}

impl LifeEvent {
    /// A relaunch from the Home key.
    pub fn home_key(&self) -> bool {
        matches!(self, Self::Relaunch { launch_type: Some(t), .. } if t == "homeKey")
    }

    /// `lifecycle: relaunch (launchType homeKey, reason …)` and the like.
    pub fn line(&self) -> String {
        let why = |what: &str, reason: &Option<String>| match reason {
            Some(r) => format!("lifecycle: {what} (reason {r})"),
            None => format!("lifecycle: {what}"),
        };
        match self {
            Self::Registered => "lifecycle: registered".to_owned(),
            Self::Relaunch {
                reason,
                launch_type,
            } => {
                let kind = launch_type.as_deref().unwrap_or("none");
                match reason {
                    Some(r) => format!("lifecycle: relaunch (launchType {kind}, reason {r})"),
                    None => format!("lifecycle: relaunch (launchType {kind})"),
                }
            }
            Self::Pause { reason } => why("pause", reason),
            Self::Close { reason } => why("close", reason),
            Self::Refused(e) => format!("lifecycle: refused: {e}"),
            Self::Other(what) => format!("lifecycle: {what}: ignored"),
        }
    }
}

/// Reads one of SAM's messages on the registration.
pub fn parse(bytes: &[u8]) -> LifeEvent {
    if bytes.len() > MAX_MESSAGE {
        return LifeEvent::Other(format!("a message over {MAX_MESSAGE} bytes"));
    }
    let Ok(Value::Object(message)) = serde_json::from_slice::<Value>(bytes) else {
        return LifeEvent::Other("an unreadable message".to_owned());
    };
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(clean_text);
    if message.get("returnValue").and_then(Value::as_bool) == Some(false) {
        let why = text(message.get("errorText"))
            .or_else(|| message.get("errorCode").map(|c| format!("error {c}")))
            .unwrap_or_else(|| "returnValue false".to_owned());
        return LifeEvent::Refused(why);
    }
    // Version 2 names it `event`, version 1 `message`.
    let event = text(message.get("event")).or_else(|| text(message.get("message")));
    let reason = text(message.get("reason"));
    match event.as_deref() {
        // The first answer may be no more than `returnValue: true`.
        None | Some("registered") => LifeEvent::Registered,
        Some("relaunch") => {
            let params = message.get("parameters");
            let launch_type = text(params.and_then(|p| p.get("launchType")))
                .or_else(|| text(message.get("launchType")));
            LifeEvent::Relaunch {
                reason,
                launch_type,
            }
        }
        Some("pause") => LifeEvent::Pause { reason },
        Some("close") => LifeEvent::Close { reason },
        Some(other) => LifeEvent::Other(format!("event {other:?}")),
    }
}

/// Where the thread leaves what it heard, and how it wakes the loop.
pub struct Sink {
    queue: Mutex<VecDeque<(LifeEvent, Instant)>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Sink {
    /// `wake` is called after each event, from the thread.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            wake: Box::new(wake),
        })
    }

    fn push(&self, event: LifeEvent, at: Instant) {
        if let Ok(mut queue) = self.queue.lock() {
            if queue.len() >= MAX_QUEUED {
                queue.pop_front();
            }
            queue.push_back((event, at));
        }
        (self.wake)();
    }

    /// Everything heard since the last call, oldest first, with when.
    pub fn take(&self) -> Vec<(LifeEvent, Instant)> {
        self.queue
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }
}

/// The thread's glib context and its wake-up, to stop it from the loop.
struct Wake {
    context: *mut GMainContext,
    wakeup: unsafe extern "C" fn(*mut GMainContext),
}

// SAFETY: used only for `g_main_context_wakeup`, which glib allows from any
// thread, on a context the thread keeps alive while the `Wake` is published:
// `Published` takes it back on every path out of `run` before the context
// goes.
unsafe impl Send for Wake {}

struct Stop {
    stop: AtomicBool,
    wake: Mutex<Option<Wake>>,
}

/// Clears the published `Wake` when dropped, panics included.
struct Published<'a>(&'a Stop);

impl Drop for Published<'_> {
    fn drop(&mut self) {
        if let Ok(mut wake) = self.0.wake.lock() {
            *wake = None;
        }
    }
}

/// The registration's thread; dropping it unregisters and stops it.
pub struct Running {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

/// Registers `app_id` with `method` on a thread of its own; what SAM says
/// goes to `sink`.
pub fn start(method: Method, app_id: &str, sink: Arc<Sink>) -> Running {
    let stop = Arc::new(Stop {
        stop: AtomicBool::new(false),
        wake: Mutex::new(None),
    });
    let app_id = app_id.to_owned();
    let on_thread = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("lifecycle".to_owned())
        .spawn(move || run(method, &app_id, &sink, &on_thread));
    let thread = match thread {
        Ok(thread) => Some(thread),
        Err(e) => {
            info!("lifecycle thread: not started: {e}");
            None
        }
    };
    Running { stop, thread }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.stop.store(true, Ordering::SeqCst);
        if let Ok(wake) = self.stop.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            // SAFETY: the context is live while the `Wake` is published.
            unsafe { (wake.wakeup)(wake.context) };
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        let until = Instant::now() + STOP_WAIT;
        while !thread.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !thread.is_finished() {
            info!("lifecycle thread: still running at exit; left to the process");
        } else if thread.join().is_err() {
            info!("lifecycle thread: panicked");
        }
    }
}

/// What the registration's callback gets as its `ctx`.
struct Ctx {
    api: &'static Api,
    sink: Arc<Sink>,
}

/// The thread: register under the app's id, subscribe, dispatch until
/// stopped. The library stays loaded for the life of the process, as the
/// watch's does.
fn run(method: Method, app_id: &str, sink: &Arc<Sink>, stop: &Stop) {
    let api: &'static Api = match Api::load() {
        Ok(api) => Box::leak(Box::new(api)),
        Err(e) => {
            info!("lifecycle: no luna: {e}");
            return;
        }
    };
    let client = match Client::register_as(api, Some(app_id)) {
        Ok(client) => client,
        Err(e) => {
            info!(
                "lifecycle: {} as {app_id}: {}",
                method.name(),
                clean_text(&e)
            );
            return;
        }
    };
    let (Ok(uri), Ok(payload)) = (CString::new(method.uri()), CString::new("{}")) else {
        return;
    };
    let ctx = Box::into_raw(Box::new(Ctx {
        api,
        sink: Arc::clone(sink),
    }));
    let mut token = 0;
    let sent = api.checked("registering", |error| {
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
        info!("lifecycle: {}: {}", method.name(), clean_text(&e));
        // SAFETY: the library refused the call, so it kept no pointer to the
        // ctx and nothing else holds it.
        drop(unsafe { Box::from_raw(ctx) });
        return;
    }
    info!(
        "lifecycle: asked {} (version {}) as {app_id}",
        method.name(),
        method.version()
    );
    let published = {
        let Ok(mut wake) = stop.wake.lock() else {
            return;
        };
        if stop.stop.load(Ordering::SeqCst) {
            return;
        }
        *wake = Some(Wake {
            context: client.context,
            wakeup: api.context_wakeup,
        });
        Published(stop)
    };
    while !stop.stop.load(Ordering::SeqCst) {
        // SAFETY: the client's own context, dispatched from this thread only;
        // 1 lets it block until a message or a wake-up.
        unsafe { (api.context_iteration)(client.context, 1) };
    }
    drop(published);
    drop(client);
}

/// The registration's `LSFilterFunc`: reads the message, logs it, and
/// leaves it for the loop.
///
/// # Safety
/// `ctx` is a live `Ctx` for the thread's life, and `message` a live message.
unsafe extern "C" fn on_message(
    _: *mut LsHandle,
    message: *mut LsMessage,
    ctx: *mut c_void,
) -> bool {
    let at = Instant::now();
    // SAFETY: forwarded from the caller.
    let ctx = unsafe { &*ctx.cast::<Ctx>() };
    // SAFETY: forwarded from the caller.
    let payload = unsafe { (ctx.api.payload)(message) };
    let bytes = if payload.is_null() {
        &[][..]
    } else {
        // SAFETY: a message's payload is a NUL-terminated string that lives
        // as long as the message.
        unsafe { CStr::from_ptr(payload) }.to_bytes()
    };
    let event = parse(bytes);
    info!("{}", event.line());
    ctx.sink.push(event, at);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn relaunch(kind: Option<&str>, reason: Option<&str>) -> LifeEvent {
        LifeEvent::Relaunch {
            reason: reason.map(str::to_owned),
            launch_type: kind.map(str::to_owned),
        }
    }

    #[test]
    fn methods_by_name_and_version() {
        for m in [Method::RegisterApp, Method::RegisterNativeApp] {
            assert_eq!(Method::from_name(m.name()), Some(m));
            assert!(m.uri().ends_with(m.name()));
        }
        assert_eq!(Method::RegisterApp.version(), 2);
        assert_eq!(Method::RegisterNativeApp.version(), 1);
        assert_eq!(Method::from_name("register"), None);
    }

    #[test]
    fn version_2_events() {
        assert_eq!(
            parse(br#"{"returnValue":true,"event":"registered"}"#),
            LifeEvent::Registered
        );
        assert_eq!(parse(br#"{"returnValue":true}"#), LifeEvent::Registered);
        let home = parse(
            br#"{"returnValue":true,"event":"relaunch","reason":"homeKey",
                 "parameters":{"launchType":"homeKey","@system_native_app":true}}"#,
        );
        assert_eq!(home, relaunch(Some("homeKey"), Some("homeKey")));
        assert!(home.home_key());
        assert_eq!(
            parse(br#"{"event":"pause","reason":"keepAlive"}"#),
            LifeEvent::Pause {
                reason: Some("keepAlive".to_owned())
            }
        );
        assert_eq!(
            parse(br#"{"event":"close","reason":"memoryReclaim"}"#),
            LifeEvent::Close {
                reason: Some("memoryReclaim".to_owned())
            }
        );
    }

    #[test]
    fn version_1_says_message_instead_of_event() {
        let from_list = parse(br#"{"message":"relaunch","parameters":{}}"#);
        assert_eq!(from_list, relaunch(None, None));
        assert!(!from_list.home_key());
        let home = parse(br#"{"message":"relaunch","parameters":{"launchType":"homeKey"}}"#);
        assert!(home.home_key());
    }

    #[test]
    fn refusals_and_oddities() {
        assert_eq!(
            parse(br#"{"returnValue":false,"errorCode":-1,"errorText":"cannot find caller id"}"#),
            LifeEvent::Refused("cannot find caller id".to_owned())
        );
        assert_eq!(
            parse(br#"{"returnValue":false,"errorCode":-1}"#),
            LifeEvent::Refused("error -1".to_owned())
        );
        assert_eq!(
            parse(br#"{"event":"splash"}"#),
            LifeEvent::Other("event \"splash\"".to_owned())
        );
        assert!(matches!(parse(b"[1]"), LifeEvent::Other(_)));
        assert!(matches!(parse(b"not json"), LifeEvent::Other(_)));
        let big = vec![b' '; MAX_MESSAGE + 1];
        assert!(matches!(parse(&big), LifeEvent::Other(_)));
        // Other programs' strings are cleaned before they reach the log.
        let odd = parse(b"{\"event\":\"relaunch\",\"reason\":\"a\\nb\"}");
        assert_eq!(odd, relaunch(None, Some("a b")));
    }

    #[test]
    fn the_lines_say_what_came() {
        assert_eq!(
            relaunch(Some("homeKey"), None).line(),
            "lifecycle: relaunch (launchType homeKey)"
        );
        assert_eq!(
            relaunch(None, Some("x")).line(),
            "lifecycle: relaunch (launchType none, reason x)"
        );
        assert_eq!(LifeEvent::Pause { reason: None }.line(), "lifecycle: pause");
        assert_eq!(
            LifeEvent::Close {
                reason: Some("y".to_owned())
            }
            .line(),
            "lifecycle: close (reason y)"
        );
    }

    #[test]
    fn the_sink_keeps_order_wakes_and_caps() {
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let sink = Sink::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let at = Instant::now();
        sink.push(LifeEvent::Registered, at);
        sink.push(relaunch(Some("homeKey"), None), at);
        assert_eq!(woken.load(Ordering::SeqCst), 2);
        let taken = sink.take();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].0, LifeEvent::Registered);
        assert!(sink.take().is_empty());
        for _ in 0..MAX_QUEUED + 5 {
            sink.push(LifeEvent::Registered, at);
        }
        assert_eq!(sink.take().len(), MAX_QUEUED);
    }
}
