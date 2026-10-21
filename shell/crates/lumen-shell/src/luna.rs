//! The TV's launcher: the apps and inputs LG's Home shows, from luna (webOS's
//! service bus) through `luna-send` (docs/specs/shell-mvp.md, "B3").
//!
//! * `listLaunchPoints` is LG's Home bar: its apps in its order, with titles,
//!   colours and absolute icon paths. On the B9 it holds no inputs.
//! * `listApps` gives each app's folder, which its icon must stay inside.
//! * `getAllInputStatus` gives the inputs, with the names the owner gave them.
//!
//! IDs, titles, icon paths and colours come from other apps' appinfo, so they
//! are checked, cleaned and confined here before the shell sees them.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::ScopedJoinHandle;
use std::time::{Duration, Instant};

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
/// The largest reply read. `listApps` is 170 KB on the B9.
const MAX_REPLY: usize = 4 << 20;
/// The longest app ID kept: an app's folder is named after its ID, so it is
/// no longer than a file name.
const MAX_ID_BYTES: usize = 255;
const LUNA_SEND: &str = "/usr/bin/luna-send";

const LIST_LAUNCH_POINTS: &str = "luna://com.webos.applicationManager/listLaunchPoints";
const LIST_APPS: &str = "luna://com.webos.applicationManager/listApps";
const LIST_INPUTS: &str = "luna://com.webos.service.eim/getAllInputStatus";
const LAUNCH: &str = "luna://com.webos.applicationManager/launch";

/// One call on the bus: a request and its single reply.
pub trait Bus: Sync {
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String>;
}

/// The bus through `luna-send`, one process a call.
pub struct LunaSend {
    program: PathBuf,
    /// Arguments ahead of luna-send's own; tests run a shell script this way.
    prefix: Vec<String>,
    timeout: Duration,
}

impl LunaSend {
    /// The TV's `luna-send`; never one found on `PATH`, since the shell runs
    /// as root.
    pub fn system() -> Self {
        Self {
            program: PathBuf::from(LUNA_SEND),
            prefix: Vec::new(),
            timeout: CALL_TIMEOUT,
        }
    }
}

/// A running `luna-send`, killed and reaped however `call` returns, so none
/// is left running or as a zombie.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        // Neither does anything to one that has exited and been waited for.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Reads `from` to its end, keeping one byte more than `MAX_REPLY` to tell a
/// reply that is too long from one that fits; the rest is drained and dropped.
fn read_capped(mut from: impl Read) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    from.by_ref()
        .take(MAX_REPLY as u64 + 1)
        .read_to_end(&mut out)?;
    io::copy(&mut from, &mut io::sink())?;
    Ok(out)
}

impl Bus for LunaSend {
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        let child = Command::new(&self.program)
            .args(&self.prefix)
            .args(["-n", "1", "-f", uri, &payload.to_string()])
            // Over SSH, luna-send -f prints nothing unless its stdin is
            // /dev/null (docs/device/b9.md); nothing should read the shell's.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Its complaints (no bus, no permission) go where the shell's go.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", self.program.display()))?;
        let mut child = Reaped(child);
        let stdout = child.0.stdout.take().ok_or("luna-send has no stdout")?;
        // Read on a thread of its own, which hands the reply over when the
        // pipe closes: a reply bigger than the pipe would otherwise stall
        // luna-send, and anything holding the pipe open would stall this.
        let (done, reply) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("luna-send".to_owned())
            .spawn(move || {
                let _ = done.send(read_capped(stdout));
            })
            .map_err(|e| format!("no thread to read luna-send's reply: {e}"))?;
        let started = Instant::now();
        let no_reply = || format!("no reply in {} ms", self.timeout.as_millis());
        let out = match reply.recv_timeout(self.timeout) {
            Ok(read) => read.map_err(|e| e.to_string())?,
            Err(RecvTimeoutError::Timeout) => return Err(no_reply()),
            Err(RecvTimeoutError::Disconnected) => {
                return Err("reading luna-send's reply failed".to_owned());
            }
        };
        // The reply is in; luna-send has what is left of the time to exit.
        let status = loop {
            match child.0.try_wait().map_err(|e| e.to_string())? {
                Some(status) => break status,
                None if started.elapsed() < self.timeout => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                None => return Err(no_reply()),
            }
        };
        if !status.success() {
            return Err(format!("luna-send failed ({status})"));
        }
        if out.len() > MAX_REPLY {
            return Err(format!("a reply over {MAX_REPLY} bytes"));
        }
        serde_json::from_slice(&out).map_err(|e| format!("unreadable reply: {e}"))
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
pub struct LunaLauncher<B: Bus = LunaSend> {
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

    /// `luna-send` played by `sh -c script`: $4 is the URI, $5 the payload.
    #[cfg(unix)]
    fn fake_luna_send(script: &str, timeout_ms: u64) -> LunaSend {
        LunaSend {
            program: PathBuf::from("/bin/sh"),
            prefix: vec!["-c".to_owned(), script.to_owned(), "luna-send".to_owned()],
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[cfg(unix)]
    #[test]
    fn luna_send_passes_the_uri_and_payload_and_reads_the_reply() {
        let bus = fake_luna_send(
            r#"printf '{"returnValue":true,"uri":"%s","payload":%s}' "$4" "$5""#,
            5000,
        );
        let reply = bus.call(LAUNCH, &json!({ "id": "a'b\"c" })).unwrap();
        assert_eq!(reply["uri"], LAUNCH);
        assert_eq!(reply["payload"], json!({ "id": "a'b\"c" }));
    }

    #[cfg(unix)]
    #[test]
    fn luna_send_failures_are_errors() {
        let silent = fake_luna_send("sleep 5", 100);
        let started = Instant::now();
        assert_eq!(
            silent.call(LAUNCH, &json!({})).unwrap_err(),
            "no reply in 100 ms"
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "killed, not waited for"
        );

        // It exits, but something it started keeps the pipe open: the limit
        // still holds.
        let lingering = fake_luna_send("sleep 5 & echo '{}'", 200);
        let started = Instant::now();
        assert_eq!(
            lingering.call(LAUNCH, &json!({})).unwrap_err(),
            "no reply in 200 ms"
        );
        assert!(started.elapsed() < Duration::from_secs(4));

        let failing = fake_luna_send("echo '{}'; exit 3", 5000);
        assert!(
            failing
                .call(LAUNCH, &json!({}))
                .unwrap_err()
                .contains("failed")
        );

        let junk = fake_luna_send("echo not json", 5000);
        assert!(
            junk.call(LAUNCH, &json!({}))
                .unwrap_err()
                .starts_with("unreadable reply")
        );

        // More than the pipe holds, and more than MAX_REPLY: drained, refused.
        let huge = fake_luna_send("head -c 5000000 /dev/zero", 5000);
        assert!(huge.call(LAUNCH, &json!({})).unwrap_err().contains("over"));

        let missing = LunaSend {
            program: PathBuf::from("/no/such/luna-send"),
            prefix: Vec::new(),
            timeout: CALL_TIMEOUT,
        };
        assert!(missing.call(LAUNCH, &json!({})).is_err());
    }
}
