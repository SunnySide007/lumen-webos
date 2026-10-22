//! The luna watch: what LG's services say while keys go missing
//! (docs/specs/shell-mvp.md, "Input diagnostics"). A thread of its own, with
//! an anonymous client and a glib context of its own, holds three
//! subscriptions for the life of the shell:
//!
//! * `tvpower/power/getPowerState`: `Active`, `Screen Saver`, `Screen Off`.
//! * `applicationManager/getForegroundAppInfo`: the app in front, which names
//!   what takes focus (LG's screensaver, LG's Home, another build).
//! * `tvpower/power/registerScreenSaverRequest`: LG asks before its
//!   screensaver starts. The shell answers `ack: true` (allowed, for the
//!   OLED's sake) and logs the request.
//!
//! Each state line says how long ago the last input came, so a focus loss two
//! minutes after the last key reads as LG's idle timer, not as a lost key
//! (missing-keys research, 2026-10-21). Other services' strings are cleaned
//! and capped before they reach the log. A subscription the hub refuses logs
//! one `luna watch` line and nothing more until it answers again; the shell
//! runs the same without any of them. The pure part (`Watch`) is tested with
//! the B9's reply shapes; the thread is the thin FFI around it.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_void};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::info;
use crate::luna::{Api, Client, GMainContext, LsHandle, LsMessage};

pub const POWER: &str = "luna://com.webos.service.tvpower/power/getPowerState";
pub const FRONT_APP: &str = "luna://com.webos.applicationManager/getForegroundAppInfo";
pub const SCREENSAVER: &str = "luna://com.webos.service.tvpower/power/registerScreenSaverRequest";
pub const SCREENSAVER_RESPONSE: &str =
    "luna://com.webos.service.tvpower/power/responseScreenSaverRequest";
/// The largest message kept; these are a few hundred bytes.
const MAX_MESSAGE: usize = 64 << 10;
/// The longest string from another service that reaches the log: a state, an
/// app id or a reason. Long enough for the hub's permission errors, which
/// name the method and the category.
const MAX_TEXT_CHARS: usize = 200;
/// The longest screensaver timestamp echoed back; LG's is a short number.
const MAX_TIMESTAMP_BYTES: usize = 128;
/// How long the loop waits for the thread at exit before leaving it to the
/// process's end.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// One of the three subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Power,
    FrontApp,
    Screensaver,
}

impl Which {
    pub const ALL: [Self; 3] = [Self::Power, Self::FrontApp, Self::Screensaver];

    /// Its name in the `luna watch <what>` line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::FrontApp => "front app",
            Self::Screensaver => "screensaver",
        }
    }

    pub const fn uri(self) -> &'static str {
        match self {
            Self::Power => POWER,
            Self::FrontApp => FRONT_APP,
            Self::Screensaver => SCREENSAVER,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Power => 0,
            Self::FrontApp => 1,
            Self::Screensaver => 2,
        }
    }

    /// The request that opens it; `client_name` is what the screensaver
    /// request names the shell by.
    pub fn payload(self, client_name: &str) -> Value {
        match self {
            Self::Screensaver => json!({ "subscribe": true, "clientName": client_name }),
            _ => json!({ "subscribe": true }),
        }
    }
}

/// What the watch thread and the loop share: the last input's time, the
/// power state for the heartbeat line, and the stop flag.
#[derive(Debug)]
pub struct Shared {
    /// Milliseconds since the loop's clock started.
    last_input_ms: AtomicU64,
    power: Mutex<Option<String>>,
    stop: AtomicBool,
    /// The thread's context, to wake it at exit; `None` until it is attached
    /// and after the thread is done with it.
    wake: Mutex<Option<Wake>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_input_ms: AtomicU64::new(0),
            power: Mutex::new(None),
            stop: AtomicBool::new(false),
            wake: Mutex::new(None),
        })
    }

    /// Input came `since_start` into the loop's clock.
    pub fn set_last_input(&self, since_start: Duration) {
        let ms = u64::try_from(since_start.as_millis()).unwrap_or(u64::MAX);
        self.last_input_ms.store(ms, Ordering::Relaxed);
    }

    pub fn last_input(&self) -> Duration {
        Duration::from_millis(self.last_input_ms.load(Ordering::Relaxed))
    }

    /// LG's last power state, if the watch has heard one.
    pub fn power(&self) -> Option<String> {
        self.power.lock().ok().and_then(|p| p.clone())
    }

    fn set_power(&self, state: String) {
        if let Ok(mut power) = self.power.lock() {
            *power = Some(state);
        }
    }
}

/// What a message asks the thread to do.
#[derive(Debug, PartialEq)]
pub enum Action {
    Log(String),
    /// Answer the screensaver request with this payload.
    Respond(Value),
    /// Keep this power state for the heartbeat line.
    Power(String),
}

/// The pure part: what each message means and the lines for it.
#[derive(Debug)]
pub struct Watch {
    client_name: String,
    power: Option<String>,
    front_app: Option<String>,
    /// Whether each subscription's last message failed and was warned about;
    /// a good message arms the warning again.
    warned: [bool; Which::ALL.len()],
}

impl Watch {
    /// `app_id` names the shell in its screensaver requests, as
    /// `<appId>.wakelock` (webosbrew's SDL does the same).
    pub fn new(app_id: &str) -> Self {
        Self {
            client_name: format!("{app_id}.wakelock"),
            power: None,
            front_app: None,
            warned: [false; Which::ALL.len()],
        }
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    /// A subscription the library refused, or one whose messages cannot be
    /// read: `luna watch <what>: <error>`, once per run of failures.
    pub fn failed(&mut self, which: Which, error: &str) -> Option<String> {
        let warned = &mut self.warned[which.index()];
        if *warned {
            return None;
        }
        *warned = true;
        Some(format!(
            "luna watch {}: {}",
            which.name(),
            clean_text(error)
        ))
    }

    fn warn(&mut self, which: Which, error: &str) -> Vec<Action> {
        self.failed(which, error)
            .map(Action::Log)
            .into_iter()
            .collect()
    }

    /// A message on `which`, `since_input` after the last input.
    pub fn message(&mut self, which: Which, bytes: &[u8], since_input: Duration) -> Vec<Action> {
        let reply = match parse(bytes) {
            Ok(reply) => reply,
            Err(e) => return self.warn(which, &e),
        };
        if reply.get("returnValue").and_then(Value::as_bool) == Some(false) {
            let why = text(&reply, "errorText").unwrap_or_else(|| "no reason given".to_owned());
            return self.warn(which, &why);
        }
        self.warned[which.index()] = false;
        let mut actions = Vec::new();
        match which {
            Which::Power => {
                if let Some(state) = power_state(&reply)
                    && self.power.as_deref() != Some(&state)
                {
                    actions.push(Action::Log(format!(
                        "power {state}: last input {} s ago",
                        ago(since_input)
                    )));
                    actions.push(Action::Power(state.clone()));
                    self.power = Some(state);
                }
            }
            Which::FrontApp => {
                if let Some(id) = front_app(&reply)
                    && self.front_app.as_deref() != Some(&id)
                {
                    actions.push(Action::Log(format!(
                        "front app {id}: last input {} s ago",
                        ago(since_input)
                    )));
                    self.front_app = Some(id);
                }
            }
            Which::Screensaver => {
                if let Some(timestamp) = screensaver_timestamp(&reply) {
                    actions.push(Action::Log(format!(
                        "screensaver request: allowed; last input {} s ago",
                        ago(since_input)
                    )));
                    actions.push(Action::Respond(json!({
                        "clientName": self.client_name,
                        "ack": true,
                        "timestamp": timestamp,
                    })));
                }
            }
        }
        actions
    }
}

/// A message's JSON, refusing one over `MAX_MESSAGE` or unreadable.
pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_MESSAGE {
        return Err(format!("a message over {MAX_MESSAGE} bytes"));
    }
    serde_json::from_slice(bytes).map_err(|e| format!("unreadable message: {e}"))
}

/// `getPowerState`'s `state` (`Active`, `Screen Saver`, `Screen Off`).
pub fn power_state(reply: &Value) -> Option<String> {
    text(reply, "state")
}

/// `getForegroundAppInfo`'s `appId`; an empty one (nothing in front, as
/// between two apps) is `(none)`, so the return to the previous app is a
/// change again.
pub fn front_app(reply: &Value) -> Option<String> {
    let id = reply.get("appId")?.as_str()?;
    let cleaned = clean_text(id);
    Some(if cleaned.is_empty() {
        "(none)".to_owned()
    } else {
        cleaned
    })
}

/// A screensaver request's `timestamp`, to echo in the answer; the
/// subscription's own first reply has none. A string goes back as it came,
/// unless it is over `MAX_TIMESTAMP_BYTES`; a number as it is.
pub fn screensaver_timestamp(reply: &Value) -> Option<Value> {
    match reply.get("timestamp")? {
        Value::String(s) if s.len() <= MAX_TIMESTAMP_BYTES => Some(Value::String(s.clone())),
        Value::Number(n) => Some(Value::Number(n.clone())),
        _ => None,
    }
}

/// A string field, cleaned, if present and not empty.
fn text(reply: &Value, key: &str) -> Option<String> {
    let cleaned = clean_text(reply.get(key)?.as_str()?);
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Another service's string, fit for one log line: trimmed, control
/// characters and line or paragraph separators turned into spaces, and cut
/// to `MAX_TEXT_CHARS`.
pub fn clean_text(text: &str) -> String {
    text.trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .take(MAX_TEXT_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Seconds to one place, as the lines show the time since the last input.
fn ago(since: Duration) -> String {
    format!("{:.1}", since.as_secs_f64())
}

/// The thread's glib context and the call that wakes it. Safe to send: glib
/// allows `g_main_context_wakeup` from any thread.
#[derive(Debug)]
struct Wake {
    context: *mut GMainContext,
    wakeup: unsafe extern "C" fn(*mut GMainContext),
}

// SAFETY: a `Wake` is only ever used for `g_main_context_wakeup`, which glib
// documents as callable from any thread, on a context the watch thread keeps
// alive while the `Wake` is published: `Published` takes it back, on every
// path out of `run` including a panic, before the client and its context go.
unsafe impl Send for Wake {}

impl Wake {
    fn wake(&self) {
        // SAFETY: the context is live while this `Wake` is in `Shared`; see
        // the impl above.
        unsafe { (self.wakeup)(self.context) };
    }
}

/// Clears the published `Wake` when dropped, so no path out of the thread,
/// a panic included, leaves a freed context behind for the stopper to wake.
struct Published<'a>(&'a Shared);

impl Drop for Published<'_> {
    fn drop(&mut self) {
        if let Ok(mut wake) = self.0.wake.lock() {
            *wake = None;
        }
    }
}

/// The running watch; dropping it stops the thread.
pub struct Running {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

/// Starts the watch on its own thread. `zero` is when the loop's clock
/// started, which `Shared`'s last-input times count from.
pub fn start(shared: Arc<Shared>, app_id: &str, zero: Instant) -> Running {
    let app_id = app_id.to_owned();
    let on_thread = Arc::clone(&shared);
    let thread = std::thread::Builder::new()
        .name("luna-watch".to_owned())
        .spawn(move || run(&on_thread, &app_id, zero));
    let thread = match thread {
        Ok(thread) => Some(thread),
        Err(e) => {
            info!("luna watch thread: not started: {e}");
            None
        }
    };
    Running { shared, thread }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(wake) = self.shared.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            wake.wake();
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        // The thread unregisters its client on its way out. One that does not
        // come back in time is left to the end of the process rather than
        // holding up the exit.
        let until = Instant::now() + STOP_WAIT;
        while !thread.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !thread.is_finished() {
            info!("luna watch thread: still running at exit; left to the process");
        } else if thread.join().is_err() {
            info!("luna watch thread: panicked");
        }
    }
}

/// What a subscription's callback gets as its `ctx`.
struct Ctx {
    which: Which,
    watch: Rc<RefCell<Watch>>,
    api: &'static Api,
    handle: *mut LsHandle,
    shared: Arc<Shared>,
    zero: Instant,
}

/// The thread: register, subscribe three times, then dispatch until stopped.
/// The library stays loaded for the life of the process (`Box::leak`): the
/// contexts the library holds point into it, and the launcher keeps the same
/// libraries open anyway.
fn run(shared: &Arc<Shared>, app_id: &str, zero: Instant) {
    let api: &'static Api = match Api::load() {
        Ok(api) => Box::leak(Box::new(api)),
        Err(e) => {
            info!("luna watch thread: {e}");
            return;
        }
    };
    let client = match Client::register(api) {
        Ok(client) => client,
        Err(e) => {
            info!("luna watch thread: {e}");
            return;
        }
    };
    let watch = Rc::new(RefCell::new(Watch::new(app_id)));
    let mut open = 0;
    for which in Which::ALL {
        let (Ok(uri), Ok(payload)) = (
            CString::new(which.uri()),
            CString::new(which.payload(watch.borrow().client_name()).to_string()),
        ) else {
            if let Some(line) = watch
                .borrow_mut()
                .failed(which, "a NUL byte in the request")
            {
                info!("{line}");
            }
            continue;
        };
        let ctx = Box::into_raw(Box::new(Ctx {
            which,
            watch: Rc::clone(&watch),
            api,
            handle: client.handle,
            shared: Arc::clone(shared),
            zero,
        }));
        let mut token = 0;
        let sent = api.checked("subscribing", |error| {
            // SAFETY: the handle is live; the library copies the URI and the
            // payload; `on_message` is an LSFilterFunc whose `ctx` is never
            // freed once the library has it (below).
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
        match sent {
            Ok(()) => open += 1,
            Err(e) => {
                if let Some(line) = watch.borrow_mut().failed(which, &e) {
                    info!("{line}");
                }
                // SAFETY: the library refused the call, so it kept no pointer
                // to the ctx and nothing else holds it.
                drop(unsafe { Box::from_raw(ctx) });
            }
        }
        // A ctx the library took is never freed: three small boxes at most,
        // for the life of the process, rather than one freed under a callback
        // if the unregister at the end were refused.
    }
    if open == 0 {
        return;
    }
    let published = {
        let Ok(mut wake) = shared.wake.lock() else {
            return;
        };
        // Published under the lock the stop checks it under, so a stop that
        // comes before this sees the thread leave, and one after it wakes
        // the context.
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        *wake = Some(Wake {
            context: client.context,
            wakeup: api.context_wakeup,
        });
        Published(shared)
    };
    while !shared.stop.load(Ordering::SeqCst) {
        // SAFETY: the client's own context, dispatched from this thread only;
        // 1 lets it block until a message or a wake-up.
        unsafe { (api.context_iteration)(client.context, 1) };
    }
    // The wake slot is cleared before the context goes.
    drop(published);
    drop(client);
}

/// The `LSFilterFunc` of the three subscriptions: reads the message, lets
/// `Watch` decide, and acts.
///
/// # Safety
/// `ctx` is a live `Ctx` for the thread's life, and `message` a live message.
unsafe extern "C" fn on_message(
    _: *mut LsHandle,
    message: *mut LsMessage,
    ctx: *mut c_void,
) -> bool {
    // SAFETY: forwarded from the caller.
    let ctx = unsafe { &*ctx.cast::<Ctx>() };
    let api = ctx.api;
    // SAFETY: forwarded from the caller.
    let payload = unsafe { (api.payload)(message) };
    let bytes = if payload.is_null() {
        &[][..]
    } else {
        // SAFETY: a message's payload is a NUL-terminated string that lives
        // as long as the message.
        unsafe { CStr::from_ptr(payload) }.to_bytes()
    };
    let since_input = ctx.zero.elapsed().saturating_sub(ctx.shared.last_input());
    let actions = ctx
        .watch
        .borrow_mut()
        .message(ctx.which, bytes, since_input);
    for action in actions {
        match action {
            Action::Log(line) => info!("{line}"),
            Action::Power(state) => ctx.shared.set_power(state),
            Action::Respond(payload) => respond(api, ctx.handle, &payload),
        }
    }
    true
}

/// Answers a screensaver request. The reply to the answer is not read.
fn respond(api: &Api, handle: *mut LsHandle, payload: &Value) {
    let (Ok(uri), Ok(payload)) = (
        CString::new(SCREENSAVER_RESPONSE),
        CString::new(payload.to_string()),
    ) else {
        info!("luna watch screensaver: the answer holds a NUL byte; not sent");
        return;
    };
    let mut token = 0;
    let sent = api.checked("answering the screensaver request", |error| {
        // SAFETY: the handle is live (this runs inside its own dispatch); the
        // library copies the strings; `ignore_reply` takes a null ctx.
        unsafe {
            (api.call_one_reply)(
                handle,
                uri.as_ptr(),
                payload.as_ptr(),
                ignore_reply,
                std::ptr::null_mut(),
                &mut token,
                error,
            )
        }
    });
    if let Err(e) = sent {
        info!("luna watch screensaver: {}", clean_text(&e));
    }
}

/// The callback of the answer's reply, which carries nothing the shell uses.
unsafe extern "C" fn ignore_reply(_: *mut LsHandle, _: *mut LsMessage, _: *mut c_void) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "io.github.sunnyside007.lumen.shell";

    fn watch() -> Watch {
        Watch::new(APP)
    }

    fn secs(n: f64) -> Duration {
        Duration::from_secs_f64(n)
    }

    #[test]
    fn each_subscription_asks_to_subscribe_and_the_screensaver_names_the_client() {
        let w = watch();
        assert_eq!(
            w.client_name(),
            "io.github.sunnyside007.lumen.shell.wakelock"
        );
        assert_eq!(
            Which::Power.payload(w.client_name()),
            json!({ "subscribe": true })
        );
        assert_eq!(
            Which::Screensaver.payload(w.client_name()),
            json!({ "subscribe": true, "clientName": "io.github.sunnyside007.lumen.shell.wakelock" })
        );
        assert_eq!(
            Which::FrontApp.uri(),
            "luna://com.webos.applicationManager/getForegroundAppInfo"
        );
        assert_eq!(
            Which::Screensaver.uri(),
            "luna://com.webos.service.tvpower/power/registerScreenSaverRequest"
        );
        assert_eq!(Which::Screensaver.name(), "screensaver");
        assert_eq!(Which::FrontApp.name(), "front app");
    }

    #[test]
    fn a_power_state_is_logged_and_kept_when_it_changes() {
        let mut w = watch();
        // The subscription's first reply carries the state too.
        let first = br#"{"returnValue":true,"subscribed":true,"state":"Active"}"#;
        assert_eq!(
            w.message(Which::Power, first, secs(0.04)),
            [
                Action::Log("power Active: last input 0.0 s ago".to_owned()),
                Action::Power("Active".to_owned()),
            ]
        );
        assert!(
            w.message(Which::Power, first, secs(5.0)).is_empty(),
            "unchanged"
        );
        let saver = br#"{"returnValue":true,"state":"Screen Saver"}"#;
        assert_eq!(
            w.message(Which::Power, saver, secs(118.34)),
            [
                Action::Log("power Screen Saver: last input 118.3 s ago".to_owned()),
                Action::Power("Screen Saver".to_owned()),
            ]
        );
    }

    #[test]
    fn the_front_app_is_logged_when_it_changes_and_nothing_in_front_counts() {
        let mut w = watch();
        let shell = br#"{"returnValue":true,"subscribed":true,"appId":"io.github.sunnyside007.lumen.shell","windowId":"","processId":"6899"}"#;
        assert_eq!(
            w.message(Which::FrontApp, shell, secs(1.0)),
            [Action::Log(
                "front app io.github.sunnyside007.lumen.shell: last input 1.0 s ago".to_owned()
            )]
        );
        let saver = br#"{"returnValue":true,"appId":"com.webos.app.screensaver","windowId":"","processId":"1234"}"#;
        assert_eq!(
            w.message(Which::FrontApp, saver, secs(120.26)),
            [Action::Log(
                "front app com.webos.app.screensaver: last input 120.3 s ago".to_owned()
            )]
        );
        assert!(w.message(Which::FrontApp, saver, secs(121.0)).is_empty());
        // Between two apps nothing is in front; the return is then a change.
        let none = br#"{"returnValue":true,"appId":""}"#;
        assert_eq!(
            w.message(Which::FrontApp, none, secs(130.0)),
            [Action::Log(
                "front app (none): last input 130.0 s ago".to_owned()
            )]
        );
        assert_eq!(w.message(Which::FrontApp, saver, secs(131.0)).len(), 1);
        assert!(
            w.message(Which::FrontApp, br#"{"returnValue":true}"#, secs(132.0))
                .is_empty(),
            "no appId at all says nothing"
        );
    }

    #[test]
    fn a_screensaver_request_is_logged_and_answered_with_its_timestamp() {
        let mut w = watch();
        let ack = br#"{"returnValue":true,"subscribed":true}"#;
        assert!(w.message(Which::Screensaver, ack, secs(0.0)).is_empty());
        let request = br#"{"timestamp":"1792645126.9","returnValue":true}"#;
        assert_eq!(
            w.message(Which::Screensaver, request, secs(112.5)),
            [
                Action::Log("screensaver request: allowed; last input 112.5 s ago".to_owned()),
                Action::Respond(json!({
                    "clientName": "io.github.sunnyside007.lumen.shell.wakelock",
                    "ack": true,
                    "timestamp": "1792645126.9",
                })),
            ]
        );
        let numeric = br#"{"timestamp":1792645126}"#;
        let actions = w.message(Which::Screensaver, numeric, secs(1.0));
        assert_eq!(
            actions[1],
            Action::Respond(json!({
                "clientName": "io.github.sunnyside007.lumen.shell.wakelock",
                "ack": true,
                "timestamp": 1792645126,
            }))
        );
        // Every request is answered, however many come.
        assert_eq!(w.message(Which::Screensaver, request, secs(2.0)).len(), 2);
    }

    #[test]
    fn a_refused_subscription_warns_once_until_it_answers_again() {
        let mut w = watch();
        let refused = b"{\"returnValue\":false,\"errorText\":\"Can not find service\\n\\u001b[2J permissions\"}";
        assert_eq!(
            w.message(Which::FrontApp, refused, secs(0.0)),
            [Action::Log(
                "luna watch front app: Can not find service  [2J permissions".to_owned()
            )]
        );
        assert!(
            w.message(Which::FrontApp, refused, secs(1.0)).is_empty(),
            "once"
        );
        // A good message arms the warning again, so a later failure shows.
        let good = br#"{"returnValue":true,"appId":"netflix"}"#;
        assert_eq!(w.message(Which::FrontApp, good, secs(2.0)).len(), 1);
        assert_eq!(w.message(Which::FrontApp, refused, secs(3.0)).len(), 1);
        // Another subscription warns on its own.
        assert_eq!(
            w.failed(Which::Power, "subscribing failed: denied (error -1)")
                .as_deref(),
            Some("luna watch power: subscribing failed: denied (error -1)")
        );
        assert_eq!(w.failed(Which::Power, "again"), None);
        let no_reason = br#"{"returnValue":false}"#;
        assert_eq!(
            w.message(Which::Screensaver, no_reason, secs(0.0)),
            [Action::Log(
                "luna watch screensaver: no reason given".to_owned()
            )]
        );
    }

    #[test]
    fn a_long_reason_keeps_its_method_and_its_error_code() {
        let mut w = watch();
        let reason = format!(
            "subscribing failed: Denied method call \"getForegroundAppInfo\" for category \"/\" {} (error -1027)",
            "x".repeat(60)
        );
        assert!(reason.chars().count() > 100 && reason.chars().count() < MAX_TEXT_CHARS);
        let line = w.failed(Which::FrontApp, &reason).unwrap();
        assert!(line.ends_with("(error -1027)"), "{line}");
        assert!(line.contains("getForegroundAppInfo"));
        let too_long = "y".repeat(MAX_TEXT_CHARS + 50);
        assert_eq!(clean_text(&too_long).chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn malformed_and_oversized_messages_warn_once_and_do_nothing_else() {
        let mut w = watch();
        assert_eq!(
            w.message(Which::Power, b"not json", secs(0.0)).len(),
            1,
            "warned"
        );
        assert!(w.message(Which::Power, b"", secs(0.0)).is_empty(), "once");
        let mut w = watch();
        let huge = format!("{{\"state\":\"{}\"}}", "x".repeat(MAX_MESSAGE)).into_bytes();
        assert_eq!(
            w.message(Which::Power, &huge, secs(0.0)),
            [Action::Log(format!(
                "luna watch power: a message over {MAX_MESSAGE} bytes"
            ))]
        );
        // Fields of the wrong type, or empty, are not states.
        let mut w = watch();
        for odd in [
            &br#"{"state":7}"#[..],
            br#"{"state":""}"#,
            br#"{"state":null}"#,
            br#"[1,2,3]"#,
            br#"{"appId":"x"}"#,
        ] {
            assert!(
                w.message(Which::Power, odd, secs(0.0)).is_empty(),
                "{odd:?}"
            );
        }
    }

    #[test]
    fn other_services_strings_are_cleaned_and_capped() {
        let long = "A".repeat(300);
        let reply: Value = serde_json::from_str(&format!(
            r#"{{"state":"Screen\u0000 Saver\u001b","appId":"{long}","timestamp":"{long}"}}"#
        ))
        .unwrap();
        assert_eq!(power_state(&reply).as_deref(), Some("Screen  Saver"));
        assert_eq!(
            front_app(&reply).map(|s| s.chars().count()),
            Some(MAX_TEXT_CHARS)
        );
        // A timestamp goes back as it came, or not at all.
        assert_eq!(screensaver_timestamp(&reply), None, "over the cap");
        assert_eq!(
            screensaver_timestamp(&json!({ "timestamp": " 17.5 " })),
            Some(Value::String(" 17.5 ".to_owned()))
        );
        assert_eq!(screensaver_timestamp(&json!({ "timestamp": true })), None);
        assert_eq!(screensaver_timestamp(&json!({})), None);
        // Line and paragraph separators cannot break a line either.
        assert_eq!(clean_text("a\u{2028}b\u{2029}c\r\nd"), "a b c  d");
    }

    #[test]
    fn the_shared_state_keeps_the_last_input_and_the_power() {
        let shared = Shared::new();
        assert_eq!(shared.last_input(), Duration::ZERO);
        shared.set_last_input(Duration::from_millis(12_345));
        assert_eq!(shared.last_input(), Duration::from_millis(12_345));
        assert_eq!(shared.power(), None);
        shared.set_power("Active".to_owned());
        assert_eq!(shared.power().as_deref(), Some("Active"));
    }

    #[test]
    fn a_published_wake_is_taken_back_when_the_thread_leaves() {
        let shared = Shared::new();
        {
            let mut wake = shared.wake.lock().unwrap();
            *wake = Some(Wake {
                context: std::ptr::null_mut(),
                wakeup: fake_wakeup,
            });
        }
        let published = Published(&shared);
        assert!(shared.wake.lock().unwrap().is_some());
        drop(published);
        assert!(shared.wake.lock().unwrap().is_none());
    }

    unsafe extern "C" fn fake_wakeup(_: *mut GMainContext) {}

    #[test]
    fn a_stopped_watch_without_a_library_returns_at_once() {
        // Off the TV there is no libluna-service2: the thread logs once and
        // ends, and dropping the handle neither hangs nor panics.
        let shared = Shared::new();
        let running = start(Arc::clone(&shared), APP, Instant::now());
        drop(running);
        assert!(shared.stop.load(Ordering::SeqCst));
        assert!(shared.wake.lock().unwrap().is_none());
    }
}
