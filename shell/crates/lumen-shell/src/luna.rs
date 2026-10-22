//! The TV's launcher: the apps and inputs LG's Home shows, from luna (webOS's
//! service bus) through `libluna-service2`, loaded at run time
//! (docs/specs/shell-mvp.md, "B3"). The shell runs in the app's jail, where
//! `/usr/bin/luna-send` cannot be executed.
//!
//! * `listLaunchPoints` is LG's Home bar: its apps in its order, with titles,
//!   colours and absolute icon paths. On the B9 it holds no inputs.
//! * `listApps` gives each app's folder, which its icon must stay inside.
//! * `getAllInputStatus` gives the inputs, with the names the owner gave them.
//!
//! IDs, titles, icon paths and colours come from other apps' appinfo, so they
//! are checked, cleaned and confined here before the shell sees them.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::io;
use std::path::{Path, PathBuf};
use std::thread::ScopedJoinHandle;
use std::time::{Duration, Instant};

use libloading::Library;
use lumen_core::apps::{App, AppId, AppKind, LaunchError, Launcher, clean_title};
use serde_json::{Value, json};

use crate::info;

/// This app, which the list leaves out.
pub const SELF_ID: &str = "io.github.sunnyside007.lumen.shell";
/// Apps pinned first, in this order, ahead of LG's.
pub const PINNED: &[&str] = &["com.beb.plxnative"];
/// Where webOS keeps the input icons `getAllInputStatus` names. An input's
/// icon must stay inside it.
const INPUT_ICONS: &str = "/usr/palm/plugins/inputapps";
/// How long one call may take before it counts as failed.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest reply kept. `listApps` is 170 KB on the B9.
const MAX_REPLY: usize = 4 << 20;
/// The longest app ID kept: an app's folder is named after its ID, so it is
/// no longer than a file name.
const MAX_ID_BYTES: usize = 255;
/// luna-service2 (3.21.2 on the B9), and the glib its replies arrive through.
const LIBRARY: &str = "libluna-service2.so.3";
const GLIB: &str = "libglib-2.0.so.0";
/// How long a call waiting for its reply sleeps when nothing came in.
const POLL: Duration = Duration::from_millis(1);

const LIST_LAUNCH_POINTS: &str = "luna://com.webos.applicationManager/listLaunchPoints";
const LIST_APPS: &str = "luna://com.webos.applicationManager/listApps";
const LIST_INPUTS: &str = "luna://com.webos.service.eim/getAllInputStatus";
const LAUNCH: &str = "luna://com.webos.applicationManager/launch";

/// One call on the bus: a request and its single reply.
pub trait Bus: Sync {
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String>;
}

// libluna-service2's types, as lunaservice.h declares them. `watch.rs` (the
// subscriptions) shares them and the `Api` and `Client` below.
pub(crate) type LsHandle = c_void;
pub(crate) type LsMessage = c_void;
pub(crate) type LsMessageToken = c_ulong;
pub(crate) type GMainContext = c_void;
pub(crate) type FilterFunc =
    unsafe extern "C" fn(*mut LsHandle, *mut LsMessage, *mut c_void) -> bool;
/// `LSCall` and `LSCallOneReply` have the same prototype.
pub(crate) type CallFn = unsafe extern "C" fn(
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
pub(crate) struct LsError {
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

/// The functions of libluna-service2 and glib that the bus and the watch call.
pub(crate) struct Api {
    error_init: unsafe extern "C" fn(*mut LsError) -> bool,
    error_free: unsafe extern "C" fn(*mut LsError),
    register: unsafe extern "C" fn(*const c_char, *mut *mut LsHandle, *mut LsError) -> bool,
    unregister: unsafe extern "C" fn(*mut LsHandle, *mut LsError) -> bool,
    attach: unsafe extern "C" fn(*mut LsHandle, *mut GMainContext, *mut LsError) -> bool,
    /// `LSCallOneReply`: one request, one reply.
    pub(crate) call_one_reply: CallFn,
    /// `LSCall`: a subscription, answered until it is cancelled or the client
    /// unregisters.
    pub(crate) call: CallFn,
    cancel: unsafe extern "C" fn(*mut LsHandle, LsMessageToken, *mut LsError) -> bool,
    pub(crate) payload: unsafe extern "C" fn(*mut LsMessage) -> *const c_char,
    context_new: unsafe extern "C" fn() -> *mut GMainContext,
    pub(crate) context_iteration: unsafe extern "C" fn(*mut GMainContext, c_int) -> c_int,
    context_unref: unsafe extern "C" fn(*mut GMainContext),
    /// `g_main_context_wakeup`: makes a blocking iteration return. Safe from
    /// any thread.
    pub(crate) context_wakeup: unsafe extern "C" fn(*mut GMainContext),
    /// Held so every function above stays valid; none in tests.
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
    pub(crate) fn load() -> Result<Self, String> {
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
                context_wakeup: sym(&glib, g, b"g_main_context_wakeup\0")?,
                _libs: vec![luna, glib],
            })
        }
    }

    /// Runs one library function with a fresh `LSError`, turning a failure
    /// into the library's message.
    pub(crate) fn checked(
        &self,
        what: &str,
        op: impl FnOnce(*mut LsError) -> bool,
    ) -> Result<(), String> {
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
}

/// An anonymous client on the bus, dispatched on a glib context of its own,
/// and unregistered on drop. Anonymous is the registration the hub takes
/// from an app's jail on webOS 4.10 (plx-native measured it; see the spec).
pub(crate) struct Client<'a> {
    api: &'a Api,
    pub(crate) handle: *mut LsHandle,
    pub(crate) context: *mut GMainContext,
}

impl<'a> Client<'a> {
    pub(crate) fn register(api: &'a Api) -> Result<Self, String> {
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

impl Drop for Client<'_> {
    fn drop(&mut self) {
        let api = self.api;
        let unregistered = api.checked("unregistering", |error| {
            // SAFETY: the handle is live, and used from this thread only.
            unsafe { (api.unregister)(self.handle, error) }
        });
        match unregistered {
            Ok(()) => {
                // SAFETY: unregistering detached the handle, so nothing uses
                // the context any more.
                unsafe { (api.context_unref)(self.context) };
            }
            // The handle may still be attached to it: the context is leaked.
            Err(e) => info!("luna: {e}; leaking its glib context"),
        }
    }
}

/// Where `on_reply` leaves a call's reply, through the call's `ctx`.
struct Slot {
    payload: unsafe extern "C" fn(*mut LsMessage) -> *const c_char,
    reply: Option<Result<Vec<u8>, String>>,
}

/// The `LSFilterFunc` of a one-reply call: copies the reply into its slot.
///
/// # Safety
/// `ctx` is a live `Slot` that nothing else touches during the call, and
/// `message` a live message.
unsafe extern "C" fn on_reply(_: *mut LsHandle, message: *mut LsMessage, ctx: *mut c_void) -> bool {
    // SAFETY: forwarded from the caller.
    let slot = unsafe { &mut *ctx.cast::<Slot>() };
    // SAFETY: forwarded from the caller.
    let payload = unsafe { (slot.payload)(message) };
    let reply = if payload.is_null() {
        Err("a reply without a payload".to_owned())
    } else {
        // SAFETY: a message's payload is a NUL-terminated string that lives
        // as long as the message.
        let bytes = unsafe { CStr::from_ptr(payload) }.to_bytes();
        if bytes.len() > MAX_REPLY {
            Err(format!("a reply over {MAX_REPLY} bytes"))
        } else {
            Ok(bytes.to_vec())
        }
    };
    if slot.reply.is_none() {
        slot.reply = Some(reply);
    }
    true
}

/// The bus through libluna-service2: one anonymous client a call, like the
/// one process a call `luna-send` would have been.
pub struct Ls2 {
    /// The library, or why it could not be loaded.
    api: Result<Api, String>,
    timeout: Duration,
}

impl Ls2 {
    /// The TV's library, loaded once. Without it, every call fails with the
    /// reason.
    pub fn system() -> Self {
        let api = Api::load();
        match &api {
            Ok(_) => info!("luna through {LIBRARY}"),
            Err(e) => info!("no luna: {e}"),
        }
        Self {
            api,
            timeout: CALL_TIMEOUT,
        }
    }
}

impl Bus for Ls2 {
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        let api = self.api.as_ref().map_err(String::clone)?;
        let uri = CString::new(uri).map_err(|_| "a URI with a NUL byte".to_owned())?;
        // serde_json writes a NUL inside a string as \u0000, so this holds.
        let payload = CString::new(payload.to_string())
            .map_err(|_| "a payload with a NUL byte".to_owned())?;
        let client = Client::register(api)?;
        let slot = Box::into_raw(Box::new(Slot {
            payload: api.payload,
            reply: None,
        }));
        let mut token: LsMessageToken = 0;
        let sent = api.checked("calling", |error| {
            // SAFETY: the handle is live; the library copies the URI and the
            // payload; `on_reply` is an LSFilterFunc, and its `ctx`, the
            // slot, stays valid until the reply is in or the call cancelled.
            unsafe {
                (api.call_one_reply)(
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
        let deadline = Instant::now() + self.timeout;
        while !answered() && Instant::now() < deadline {
            // SAFETY: the client's own context, dispatched from this thread
            // only; 0 asks it not to block.
            if unsafe { (api.context_iteration)(client.context, 0) } == 0 {
                std::thread::sleep(POLL);
            }
        }
        let no_reply = || format!("no reply in {} ms", self.timeout.as_millis());
        if !answered() {
            let cancelled = api.checked("cancelling", |error| {
                // SAFETY: the handle is live, and the token its open call's.
                unsafe { (api.cancel)(client.handle, token, error) }
            });
            if let Err(e) = cancelled {
                // The call is still open and could yet write to the slot,
                // through the client: both are leaked rather than freed
                // under it.
                info!("luna: {e}; leaking the call");
                std::mem::forget(client);
                return Err(no_reply());
            }
        }
        drop(client);
        // SAFETY: the call was answered or cancelled, so the library no
        // longer holds the slot.
        let slot = unsafe { Box::from_raw(slot) };
        let reply = slot.reply.unwrap_or_else(|| Err(no_reply()))?;
        serde_json::from_slice(&reply).map_err(|e| format!("unreadable reply: {e}"))
    }
}

/// A reply's error, unless it says it succeeded.
fn check(reply: &Value) -> Result<(), String> {
    if reply.get("returnValue").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let text =
        str_field(reply, "errorText").map_or_else(|| "no reason given".to_owned(), clean_title);
    Err(text)
}

fn list<'a>(reply: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    reply
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// A string field, if present and not empty.
fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// An app ID as webOS makes them: printable ASCII without spaces, and no
/// longer than `MAX_ID_BYTES`. IDs reach the log and `launch` as they are.
fn plausible_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES && id.bytes().all(|b| b.is_ascii_graphic())
}

/// `#rrggbb`, in either case, as RGB.
fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}

/// `path` (absolute, or relative to `folder`) with its links and `..`
/// resolved, if it is still inside `folder`.
pub fn confine(path: &Path, folder: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(folder.join(path)).ok()?;
    let folder = std::fs::canonicalize(folder).ok()?;
    real.starts_with(&folder).then_some(real)
}

/// What `listApps` says about one app.
struct AppInfo {
    folder: Option<PathBuf>,
    color: Option<[u8; 3]>,
}

fn app_infos(reply: &Value) -> HashMap<&str, AppInfo> {
    list(reply, "apps")
        .filter_map(|app| {
            let info = AppInfo {
                folder: str_field(app, "folderPath")
                    .filter(|p| p.starts_with('/'))
                    .map(PathBuf::from),
                color: str_field(app, "iconColor").and_then(parse_color),
            };
            Some((str_field(app, "id")?, info))
        })
        .collect()
}

/// A tile, and the parameters launching it sends.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    app: App,
    params: Value,
}

/// What `entries` left out, for the log.
#[derive(Debug, Default, PartialEq)]
struct Skipped {
    /// Launch points without a plausible ID, others than an app's own
    /// (bookmarks, channels), and repeats of an app already listed.
    launch_points: usize,
    /// Inputs with nothing connected.
    inputs: usize,
}

/// Turns the three replies into tiles: the pinned apps, then the rest of
/// LG's launch points in its order, then the connected inputs. `confine`
/// returns an icon's path only if it stays inside the given folder.
fn entries(
    points: &Value,
    apps: &Value,
    inputs: &Value,
    confine: &dyn Fn(&Path, &Path) -> Option<PathBuf>,
) -> (Vec<Entry>, Skipped) {
    let infos = app_infos(apps);
    let mut seen = HashSet::new();
    let mut skipped = Skipped::default();
    let mut out = Vec::new();
    for point in list(points, "launchPoints") {
        let Some(id) = str_field(point, "id").filter(|id| plausible_id(id)) else {
            skipped.launch_points += 1;
            continue;
        };
        let lptype = str_field(point, "lptype").unwrap_or("default");
        if id == SELF_ID {
            continue;
        }
        if lptype != "default" || !seen.insert(id.to_owned()) {
            skipped.launch_points += 1;
            continue;
        }
        let info = infos.get(id);
        let folder = info.and_then(|i| i.folder.as_deref());
        // The 130 px icon, which a tile draws at up to 138 px, else the 80.
        let icon = folder.and_then(|folder| {
            ["largeIcon", "icon"]
                .iter()
                .filter_map(|key| str_field(point, key))
                .find_map(|path| confine(Path::new(path), folder))
        });
        let color = str_field(point, "iconColor")
            .and_then(parse_color)
            .or_else(|| info.and_then(|i| i.color));
        let params = point
            .get("params")
            .filter(|p| p.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        out.push(Entry {
            app: App {
                id: AppId::new(id),
                title: title_or_id(str_field(point, "title"), id),
                kind: AppKind::App,
                icon,
                color,
            },
            params,
        });
    }
    // Stable, so the rest keep LG's order.
    out.sort_by_key(|e| {
        PINNED
            .iter()
            .position(|&p| p == e.app.id.as_str())
            .unwrap_or(PINNED.len())
    });

    for device in list(inputs, "devices") {
        let Some(id) = str_field(device, "appId").filter(|id| plausible_id(id)) else {
            continue;
        };
        if device.get("connected").and_then(Value::as_bool) != Some(true) {
            skipped.inputs += 1;
            continue;
        }
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let icon = str_field(device, "iconPrefix")
            .zip(str_field(device, "icon"))
            .and_then(|(prefix, name)| {
                confine(&Path::new(prefix).join(name), Path::new(INPUT_ICONS))
            });
        out.push(Entry {
            app: App {
                id: AppId::new(id),
                title: title_or_id(str_field(device, "label"), id),
                kind: AppKind::Input,
                icon,
                color: infos.get(id).and_then(|i| i.color),
            },
            params: json!({}),
        });
    }
    (out, skipped)
}

/// A call made on a thread of its own, or an error if the thread could not
/// start or panicked.
fn joined<T>(spawned: io::Result<ScopedJoinHandle<'_, Result<T, String>>>) -> Result<T, String> {
    match spawned {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|_| Err("the call's thread panicked".to_owned())),
        Err(e) => {
            info!("no thread for a luna call: {e}");
            Err(e.to_string())
        }
    }
}

fn title_or_id(title: Option<&str>, id: &str) -> String {
    let title = title.map(clean_title).unwrap_or_default();
    if title.is_empty() {
        clean_title(id)
    } else {
        title
    }
}

/// Lists and launches through luna.
pub struct LunaLauncher<B: Bus = Ls2> {
    bus: B,
    /// Each tile's launch parameters, from the last list.
    params: HashMap<AppId, Value>,
}

impl<B: Bus> LunaLauncher<B> {
    pub fn new(bus: B) -> Self {
        Self {
            bus,
            params: HashMap::new(),
        }
    }

    /// One call, checked and timed for the log.
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        let started = Instant::now();
        let reply = self.bus.call(uri, payload).and_then(|reply| {
            check(&reply)?;
            Ok(reply)
        });
        let method = uri.rsplit('/').next().unwrap_or(uri);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        match &reply {
            Ok(_) => info!("luna {method}: {ms:.0} ms"),
            Err(e) => info!("luna {method} failed after {ms:.0} ms: {e}"),
        }
        reply
    }
}

impl<B: Bus> Launcher for LunaLauncher<B> {
    /// Asks for the three lists at once. Without launch points there is
    /// nothing to show; without `listApps` no icon can be confined, so the
    /// tiles show letters; without the inputs there are only apps.
    fn list(&mut self) -> Result<Vec<App>, LaunchError> {
        let this = &*self;
        let ask = &json!({});
        let (points, apps, inputs) = std::thread::scope(|s| {
            let spawn = move |uri: &'static str| {
                std::thread::Builder::new().spawn_scoped(s, move || this.call(uri, ask))
            };
            let apps = spawn(LIST_APPS);
            let inputs = spawn(LIST_INPUTS);
            let points = this.call(LIST_LAUNCH_POINTS, ask);
            (points, joined(apps), joined(inputs))
        });
        let points = points.map_err(LaunchError)?;
        let apps = apps.unwrap_or_else(|_| json!({}));
        let inputs = inputs.unwrap_or_else(|_| json!({}));
        let (entries, skipped) = entries(&points, &apps, &inputs, &confine);
        let icons = entries.iter().filter(|e| e.app.icon.is_some()).count();
        info!(
            "{} tiles, {icons} with icons; left out {} launch points and {} unconnected inputs",
            entries.len(),
            skipped.launch_points,
            skipped.inputs
        );
        self.params = entries
            .iter()
            .map(|e| (e.app.id.clone(), e.params.clone()))
            .collect();
        Ok(entries.into_iter().map(|e| e.app).collect())
    }

    fn launch(&mut self, id: &AppId) -> Result<(), LaunchError> {
        let params = self.params.get(id).cloned().unwrap_or_else(|| json!({}));
        self.call(LAUNCH, &json!({ "id": id.as_str(), "params": params }))
            .map(drop)
            .map_err(LaunchError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::Mutex;

    const POINTS: &str = include_str!("../fixtures/luna/b9-launch-points.json");
    const APPS: &str = include_str!("../fixtures/luna/b9-apps.json");
    const INPUTS: &str = include_str!("../fixtures/luna/b9-inputs.json");

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture parses")
    }

    /// A bus that answers from the B9's fixtures and records launches.
    struct FakeBus {
        replies: HashMap<&'static str, Result<Value, String>>,
        sent: Mutex<Vec<(String, Value)>>,
    }

    impl FakeBus {
        fn b9() -> Self {
            let replies = [
                (LIST_LAUNCH_POINTS, Ok(parse(POINTS))),
                (LIST_APPS, Ok(parse(APPS))),
                (LIST_INPUTS, Ok(parse(INPUTS))),
                (
                    LAUNCH,
                    Ok(json!({ "returnValue": true, "processId": "1001" })),
                ),
            ];
            Self {
                replies: replies.into_iter().collect(),
                sent: Mutex::new(Vec::new()),
            }
        }

        fn answering(mut self, uri: &'static str, reply: Result<Value, String>) -> Self {
            self.replies.insert(uri, reply);
            self
        }
    }

    impl Bus for FakeBus {
        fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
            self.sent
                .lock()
                .unwrap()
                .push((uri.to_owned(), payload.clone()));
            self.replies
                .get(uri)
                .cloned()
                .unwrap_or_else(|| Err(format!("no fake reply for {uri}")))
        }
    }

    /// Confinement without the file system: the joined path, if it starts
    /// with the folder. The real `confine` has tests of its own below.
    fn lexical(path: &Path, folder: &Path) -> Option<PathBuf> {
        let path = folder.join(path);
        path.starts_with(folder).then_some(path)
    }

    fn b9_entries() -> Vec<Entry> {
        entries(&parse(POINTS), &parse(APPS), &parse(INPUTS), &lexical).0
    }

    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.app.id.as_str()).collect()
    }

    #[test]
    fn plx_native_first_then_lgs_order_then_connected_inputs() {
        let entries = b9_entries();
        assert_eq!(
            ids(&entries),
            [
                "com.beb.plxnative",
                "com.webos.app.photovideo",
                "netflix",
                "cdp-30",
                "com.webos.app.discovery",
                "com.open.hidden.menu",
                "com.webos.app.hdmi1",
                "com.webos.app.hdmi2",
                "com.webos.app.hdmi3",
                "com.webos.app.hdmi4",
            ]
        );
        let (_, skipped) = entries_with(&lexical);
        assert_eq!(
            skipped,
            Skipped {
                launch_points: 0,
                inputs: 1
            },
            "Lumen itself is not counted; AV has nothing connected"
        );
    }

    fn entries_with(confine: &dyn Fn(&Path, &Path) -> Option<PathBuf>) -> (Vec<Entry>, Skipped) {
        entries(&parse(POINTS), &parse(APPS), &parse(INPUTS), confine)
    }

    #[test]
    fn tiles_get_titles_colours_and_kinds() {
        let entries = b9_entries();
        let get = |id: &str| {
            &entries
                .iter()
                .find(|e| e.app.id.as_str() == id)
                .unwrap()
                .app
        };
        assert_eq!(get("netflix").title, "Netflix");
        assert_eq!(get("netflix").color, Some([255, 255, 255]));
        assert_eq!(get("netflix").kind, AppKind::App);
        assert_eq!(
            get("com.open.hidden.menu").color,
            None,
            "an empty iconColor"
        );
        // Inputs take the names the owner gave them, trimmed, and the HDMI
        // apps' colour from listApps.
        assert_eq!(get("com.webos.app.hdmi1").title, "Game console");
        assert_eq!(get("com.webos.app.hdmi3").title, "Streamer");
        assert_eq!(get("com.webos.app.hdmi3").kind, AppKind::Input);
        assert_eq!(get("com.webos.app.hdmi3").color, Some([0x4b, 0x4b, 0x4b]));
    }

    #[test]
    fn icons_prefer_the_large_one_and_stay_in_their_folder() {
        let entries = b9_entries();
        let icon = |id: &str| {
            entries
                .iter()
                .find(|e| e.app.id.as_str() == id)
                .and_then(|e| e.app.icon.clone())
        };
        assert_eq!(
            icon("netflix"),
            Some(PathBuf::from(
                "/media/cryptofs/apps/usr/palm/applications/netflix/LARGE_APP_ICON.png"
            ))
        );
        // Its largeIcon is empty, so the 80 px one.
        assert_eq!(
            icon("com.open.hidden.menu"),
            Some(PathBuf::from(
                "/media/developer/apps/usr/palm/applications/com.open.hidden.menu/icon.png"
            ))
        );
        assert_eq!(
            icon("com.webos.app.hdmi2"),
            Some(PathBuf::from(
                "/usr/palm/plugins/inputapps/assets/small/hometheater.png"
            ))
        );
    }

    #[test]
    fn an_icon_outside_its_folder_is_dropped() {
        let points = json!({ "returnValue": true, "launchPoints": [
            { "id": "evil", "title": "Evil", "largeIcon": "/etc/shadow", "icon": "/var/x.png" },
            { "id": "nofolder", "title": "No folder", "icon": "/usr/palm/applications/nofolder/i.png" },
        ]});
        let apps = json!({ "returnValue": true, "apps": [
            { "id": "evil", "folderPath": "/usr/palm/applications/evil" },
            { "id": "nofolder", "folderPath": "relative/path" },
        ]});
        let inputs = json!({ "returnValue": true, "devices": [
            { "appId": "com.webos.app.hdmi1", "connected": true, "label": "X",
              "iconPrefix": "/usr/palm/plugins/inputapps/assets/", "icon": "/etc/passwd" },
        ]});
        let (entries, _) = entries(&points, &apps, &inputs, &lexical);
        assert_eq!(ids(&entries), ["evil", "nofolder", "com.webos.app.hdmi1"]);
        assert!(entries.iter().all(|e| e.app.icon.is_none()), "{entries:?}");
    }

    #[test]
    fn other_launch_points_repeats_and_lumen_are_left_out() {
        let points = json!({ "returnValue": true, "launchPoints": [
            { "id": "netflix", "title": "Netflix", "lptype": "default" },
            { "id": "com.webos.app.browser", "title": "A bookmark", "lptype": "bookmark",
              "params": { "target": "https://example.com" } },
            { "id": "netflix", "title": "Netflix again" },
            { "id": SELF_ID, "title": "Lumen" },
            { "title": "No ID" },
            { "id": "two\nlines", "title": "A forged log line" },
            { "id": "with space" },
            { "id": "x".repeat(MAX_ID_BYTES + 1) },
        ]});
        let inputs = json!({ "returnValue": true, "devices": [
            { "appId": "hdmi\u{1b}[2J", "connected": true, "label": "X" },
        ]});
        let (entries, skipped) = entries(&points, &json!({}), &inputs, &lexical);
        assert_eq!(ids(&entries), ["netflix"]);
        assert_eq!(skipped.launch_points, 6);
        assert!(plausible_id(&"x".repeat(MAX_ID_BYTES)));
    }

    #[test]
    fn titles_fall_back_to_the_id_and_are_capped() {
        let long = "A".repeat(1000);
        let points = json!({ "returnValue": true, "launchPoints": [
            { "id": "a", "title": long },
            { "id": "b", "title": "  " },
            { "id": "c" },
        ]});
        let (entries, _) = entries(&points, &json!({}), &json!({}), &lexical);
        assert_eq!(
            entries[0].app.title.len(),
            lumen_core::apps::MAX_TITLE_CHARS
        );
        assert_eq!(entries[1].app.title, "b");
        assert_eq!(entries[2].app.title, "c");
    }

    #[test]
    fn colours_must_be_six_hex_digits() {
        assert_eq!(parse_color("#FFFFFF"), Some([255, 255, 255]));
        assert_eq!(parse_color("#0b0f1c"), Some([0x0b, 0x0f, 0x1c]));
        for bad in [
            "",
            "#fff",
            "0b0f1c",
            "#+12345",
            "#12345g",
            "#0b0f1c00",
            "#é1234",
        ] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_launcher_lists_and_launches_with_each_tiles_params() {
        let points = json!({ "returnValue": true, "launchPoints": [
            { "id": "netflix", "title": "Netflix", "params": { "contentTarget": "x" } },
        ]});
        let bus = FakeBus::b9().answering(LIST_LAUNCH_POINTS, Ok(points));
        let mut launcher = LunaLauncher::new(bus);
        let apps = launcher.list().unwrap();
        assert_eq!(apps[0].id.as_str(), "netflix");
        launcher.launch(&AppId::new("netflix")).unwrap();
        launcher.launch(&AppId::new("com.webos.app.hdmi2")).unwrap();
        let sent = launcher.bus.sent.lock().unwrap();
        let launches: Vec<&Value> = sent
            .iter()
            .filter(|(uri, _)| uri == LAUNCH)
            .map(|(_, payload)| payload)
            .collect();
        assert_eq!(
            launches,
            [
                &json!({ "id": "netflix", "params": { "contentTarget": "x" } }),
                &json!({ "id": "com.webos.app.hdmi2", "params": {} }),
            ]
        );
    }

    #[test]
    fn a_refused_launch_reports_lunas_reason() {
        let refused = json!({ "returnValue": false, "errorText": "app not found\n" });
        let mut launcher = LunaLauncher::new(FakeBus::b9().answering(LAUNCH, Ok(refused)));
        let err = launcher.launch(&AppId::new("nope")).unwrap_err();
        assert_eq!(err.to_string(), "app not found");
        let mut launcher = LunaLauncher::new(FakeBus::b9().answering(LAUNCH, Ok(json!({}))));
        assert_eq!(
            launcher.launch(&AppId::new("x")).unwrap_err().to_string(),
            "no reason given"
        );
    }

    #[test]
    fn listing_needs_the_launch_points_and_nothing_else() {
        let failing = FakeBus::b9().answering(LIST_LAUNCH_POINTS, Err("bus down".to_owned()));
        let err = LunaLauncher::new(failing).list().unwrap_err();
        assert_eq!(err.to_string(), "bus down");

        // Without listApps nothing can be confined: no icons, but every tile.
        let bus = FakeBus::b9()
            .answering(LIST_APPS, Err("denied".to_owned()))
            .answering(LIST_INPUTS, Ok(json!({ "returnValue": false })));
        let apps = LunaLauncher::new(bus).list().unwrap();
        assert_eq!(apps.len(), 6, "the launch points, without inputs");
        assert!(apps.iter().all(|a| a.icon.is_none()));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-luna-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn confine_keeps_paths_inside_the_folder() {
        let root = temp_dir("confine");
        let app = root.join("app");
        std::fs::create_dir_all(app.join("assets")).unwrap();
        std::fs::write(app.join("assets/icon.png"), b"x").unwrap();
        std::fs::write(root.join("secret"), b"x").unwrap();
        let inside = confine(Path::new("assets/icon.png"), &app).expect("inside");
        assert!(inside.ends_with("assets/icon.png"));
        assert!(
            confine(&app.join("assets/icon.png"), &app).is_some(),
            "absolute"
        );
        assert_eq!(confine(Path::new("../secret"), &app), None);
        assert_eq!(confine(&root.join("secret"), &app), None);
        assert_eq!(confine(Path::new("missing.png"), &app), None);
        // A sibling whose name starts the same is not inside.
        std::fs::create_dir_all(root.join("app2")).unwrap();
        std::fs::write(root.join("app2/i.png"), b"x").unwrap();
        assert_eq!(confine(Path::new("../app2/i.png"), &app), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn confine_follows_links_out_and_refuses_them() {
        let root = temp_dir("links");
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(root.join("secret"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("secret"), app.join("icon.png")).unwrap();
        assert_eq!(confine(Path::new("icon.png"), &app), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What the fake libluna-service2 below does and saw, one per test
    /// thread. The service answers `reply` once `reply_after` iterations have
    /// found nothing, or never if there is no reply.
    #[derive(Default)]
    struct FakeLs2 {
        /// The step the library refuses: register, attach, call, cancel or
        /// unregister.
        refuse: Option<&'static str>,
        reply: Option<CString>,
        reply_after: u32,
        /// The open call's callback and its `ctx`.
        open: Option<(FilterFunc, *mut c_void)>,
        anonymous: bool,
        sent: Vec<(String, String)>,
        steps: Vec<&'static str>,
        /// LSErrors initialised and not yet freed.
        live_errors: i32,
    }

    thread_local! {
        static FAKE: RefCell<FakeLs2> = RefCell::default();
    }

    fn fake<R>(f: impl FnOnce(&mut FakeLs2) -> R) -> R {
        FAKE.with(|state| f(&mut state.borrow_mut()))
    }

    /// The bus over the fake library, with its state reset.
    fn fake_bus(timeout_ms: u64) -> Ls2 {
        fake(|s| *s = FakeLs2::default());
        Ls2 {
            api: Ok(Api {
                error_init: fake_error_init,
                error_free: fake_error_free,
                register: fake_register,
                unregister: fake_unregister,
                attach: fake_attach,
                call_one_reply: fake_call_one_reply,
                call: fake_call_one_reply,
                cancel: fake_cancel,
                payload: fake_payload,
                context_new: fake_context_new,
                context_iteration: fake_context_iteration,
                context_unref: fake_context_unref,
                context_wakeup: fake_context_wakeup,
                _libs: Vec::new(),
            }),
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    fn reply(text: &str) -> Option<CString> {
        Some(CString::new(text).unwrap())
    }

    /// Records `step`, and fails it, as the library would, if the test said
    /// to: false, with a message in the error.
    fn refused(step: &'static str, error: *mut LsError) -> bool {
        let refuse = fake(|s| {
            s.steps.push(step);
            s.refuse == Some(step)
        });
        if refuse {
            let message = CString::new(format!("{step} refused\nby the hub")).unwrap();
            // SAFETY: every library function gets a live, initialised LSError.
            unsafe {
                (*error).error_code = -1027;
                (*error).message = message.into_raw();
            }
        }
        refuse
    }

    unsafe extern "C" fn fake_error_init(error: *mut LsError) -> bool {
        fake(|s| s.live_errors += 1);
        // SAFETY: callers pass a valid LSError.
        unsafe { *error = LsError::empty() };
        true
    }

    unsafe extern "C" fn fake_error_free(error: *mut LsError) {
        fake(|s| s.live_errors -= 1);
        // SAFETY: callers pass an LSError from `fake_error_init`, whose
        // message, if any, `refused` made with `CString::into_raw`.
        unsafe {
            if !(*error).message.is_null() {
                drop(CString::from_raw((*error).message));
            }
            *error = LsError::empty();
        }
    }

    unsafe extern "C" fn fake_register(
        name: *const c_char,
        handle: *mut *mut LsHandle,
        error: *mut LsError,
    ) -> bool {
        fake(|s| s.anonymous = name.is_null());
        if refused("register", error) {
            return false;
        }
        // SAFETY: callers pass a valid place for the handle.
        unsafe { *handle = NonNull::<u8>::dangling().as_ptr().cast() };
        true
    }

    unsafe extern "C" fn fake_unregister(_: *mut LsHandle, error: *mut LsError) -> bool {
        !refused("unregister", error)
    }

    unsafe extern "C" fn fake_attach(
        _: *mut LsHandle,
        _: *mut GMainContext,
        error: *mut LsError,
    ) -> bool {
        !refused("attach", error)
    }

    unsafe extern "C" fn fake_call_one_reply(
        _: *mut LsHandle,
        uri: *const c_char,
        payload: *const c_char,
        callback: FilterFunc,
        ctx: *mut c_void,
        token: *mut LsMessageToken,
        error: *mut LsError,
    ) -> bool {
        // SAFETY: callers pass two C strings.
        let (uri, payload) = unsafe { (CStr::from_ptr(uri), CStr::from_ptr(payload)) };
        let sent = (
            uri.to_string_lossy().into_owned(),
            payload.to_string_lossy().into_owned(),
        );
        fake(|s| s.sent.push(sent));
        if refused("call", error) {
            return false;
        }
        fake(|s| s.open = Some((callback, ctx)));
        // SAFETY: callers pass a valid place for the token.
        unsafe { *token = 7 };
        true
    }

    unsafe extern "C" fn fake_cancel(
        _: *mut LsHandle,
        token: LsMessageToken,
        error: *mut LsError,
    ) -> bool {
        assert_eq!(token, 7);
        if refused("cancel", error) {
            return false;
        }
        fake(|s| s.open = None);
        true
    }

    /// A fake message is its payload.
    unsafe extern "C" fn fake_payload(message: *mut LsMessage) -> *const c_char {
        message.cast_const().cast()
    }

    unsafe extern "C" fn fake_context_new() -> *mut GMainContext {
        fake(|s| s.steps.push("context"));
        NonNull::<u8>::dangling().as_ptr().cast()
    }

    unsafe extern "C" fn fake_context_unref(_: *mut GMainContext) {
        fake(|s| s.steps.push("unref"));
    }

    unsafe extern "C" fn fake_context_wakeup(_: *mut GMainContext) {
        fake(|s| s.steps.push("wakeup"));
    }

    /// Delivers the reply to the open call once it is due.
    unsafe extern "C" fn fake_context_iteration(_: *mut GMainContext, may_block: c_int) -> c_int {
        assert_eq!(may_block, 0, "a call never blocks in glib");
        let due = fake(|s| {
            let message = s.reply.as_ref()?.as_ptr();
            let (callback, ctx) = s.open?;
            if s.reply_after > 0 {
                s.reply_after -= 1;
                return None;
            }
            s.open = None;
            Some((callback, ctx, message))
        });
        let Some((callback, ctx, message)) = due else {
            return 0;
        };
        // SAFETY: what the library does with an open call: its callback, its
        // ctx, and a message, here the reply, which FAKE keeps alive.
        unsafe { callback(std::ptr::null_mut(), message.cast_mut().cast(), ctx) };
        1
    }

    #[test]
    fn ls2_calls_as_an_anonymous_client_and_reads_the_reply() {
        let bus = fake_bus(5000);
        fake(|s| {
            s.reply = reply(r#"{"returnValue":true,"processId":"1001"}"#);
            s.reply_after = 3;
        });
        let answer = bus.call(LAUNCH, &json!({ "id": "a'b\"c" })).unwrap();
        assert_eq!(answer["processId"], "1001");
        fake(|s| {
            assert!(s.anonymous);
            assert_eq!(
                s.sent,
                [(LAUNCH.to_owned(), r#"{"id":"a'b\"c"}"#.to_owned())]
            );
            assert_eq!(
                s.steps,
                [
                    "context",
                    "register",
                    "attach",
                    "call",
                    "unregister",
                    "unref"
                ]
            );
            assert_eq!(s.live_errors, 0, "every LSError freed");
        });
    }

    #[test]
    fn ls2_cancels_a_call_without_a_reply_and_cleans_up() {
        let bus = fake_bus(30);
        let started = Instant::now();
        assert_eq!(
            bus.call(LAUNCH, &json!({})).unwrap_err(),
            "no reply in 30 ms"
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        fake(|s| {
            assert_eq!(
                s.steps,
                [
                    "context",
                    "register",
                    "attach",
                    "call",
                    "cancel",
                    "unregister",
                    "unref"
                ]
            );
            assert!(s.open.is_none());
            assert_eq!(s.live_errors, 0);
        });
    }

    #[test]
    fn ls2_reports_the_librarys_refusals_and_cleans_up() {
        let cases: [(&str, &[&str]); 3] = [
            ("register", &["context", "register", "unref"]),
            (
                "attach",
                &["context", "register", "attach", "unregister", "unref"],
            ),
            (
                "call",
                &[
                    "context",
                    "register",
                    "attach",
                    "call",
                    "unregister",
                    "unref",
                ],
            ),
        ];
        for (step, steps) in cases {
            let bus = fake_bus(5000);
            fake(|s| s.refuse = Some(step));
            let err = bus.call(LAUNCH, &json!({})).unwrap_err();
            assert!(err.ends_with(" refused by the hub (error -1027)"), "{err}");
            fake(|s| {
                assert_eq!(s.steps, steps, "{step}");
                assert_eq!(s.live_errors, 0, "{step}");
            });
        }
        assert_eq!(LsError::empty().text(), "error 0");
    }

    #[test]
    fn ls2_leaks_a_call_it_could_not_cancel() {
        let bus = fake_bus(20);
        fake(|s| {
            s.refuse = Some("cancel");
            s.reply = reply(r#"{"late":true}"#);
            s.reply_after = u32::MAX;
        });
        assert_eq!(
            bus.call(LAUNCH, &json!({})).unwrap_err(),
            "no reply in 20 ms"
        );
        fake(|s| {
            assert!(!s.steps.contains(&"unregister"), "{:?}", s.steps);
            assert!(!s.steps.contains(&"unref"), "{:?}", s.steps);
            s.reply_after = 0;
        });
        // The late reply still finds its slot, which was leaked, not freed.
        // SAFETY: the fake context ignores its argument.
        let delivered = unsafe { fake_context_iteration(std::ptr::null_mut(), 0) };
        assert_eq!(delivered, 1);
    }

    #[test]
    fn ls2_refuses_bad_replies_and_arguments() {
        let bus = fake_bus(5000);
        fake(|s| s.reply = reply("not json"));
        let err = bus.call(LAUNCH, &json!({})).unwrap_err();
        assert!(err.starts_with("unreadable reply"), "{err}");

        let bus = fake_bus(5000);
        fake(|s| s.reply = reply(&"x".repeat(MAX_REPLY + 1)));
        let err = bus.call(LAUNCH, &json!({})).unwrap_err();
        assert_eq!(err, format!("a reply over {MAX_REPLY} bytes"));

        let bus = fake_bus(5000);
        let err = bus.call("luna://a\0b", &json!({})).unwrap_err();
        assert_eq!(err, "a URI with a NUL byte");
        fake(|s| assert!(s.steps.is_empty(), "nothing registered"));

        let missing = Ls2 {
            api: Err("libluna-service2.so.3: no such file".to_owned()),
            timeout: CALL_TIMEOUT,
        };
        assert_eq!(
            missing.call(LAUNCH, &json!({})).unwrap_err(),
            "libluna-service2.so.3: no such file"
        );
    }
}
