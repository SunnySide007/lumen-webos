//! Lumen Remote for Windows (docs/specs/remote.md, R3): a tray icon whose
//! left click shows or hides the thin remote at the side of the desktop, a
//! menu (Thin remote, Quick panel, Open window, Pair a phone, Quit), and
//! the same page in a popup above the icon and in a normal window. The page is `remote/ui`,
//! built with `npm run build:pc`; it reaches the TV through the commands
//! below, which run `lumen-ctl serve` over SSH with the app's own key
//! (`lumen-remote-link`). The app talks to nothing but the TV.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod tray;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use lumen_remote_link::session::{Context, SendError};
use lumen_remote_link::settings::{check_host, parse_port};
use lumen_remote_link::{Event, Mac, Paths, Session, Settings, ssh, wol};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, State, WindowEvent};

use crate::applog::AppLog;

/// What the page's socket hears about its session, in order.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum LinkMsg {
    /// ssh has started; the hello follows once it is in.
    Open,
    Line {
        line: String,
    },
    Closed {
        reason: String,
    },
}

/// The settings as the page shows them.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PcSettings {
    host: Option<String>,
    port: u16,
    mac: Option<String>,
    key: String,
    key_found: bool,
    ssh: String,
}

pub struct App {
    paths: Paths,
    log: AppLog,
    links: Arc<Mutex<HashMap<u32, Session>>>,
    next_link: AtomicU32,
    /// When the popup was last opened from the tray, for its timing line.
    pub popup_opened: Mutex<Option<Instant>>,
    /// When the popup last hid on losing the focus: the tray click that
    /// took the focus must not open it again.
    pub popup_hidden: Mutex<Option<Instant>>,
    /// Whether the thin remote has been put at the side of the desktop: it
    /// is placed once, and stays where the owner drags it after that.
    pub side_placed: AtomicBool,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    pub fn log(&self, line: &str) {
        self.log.line(line);
    }

    fn ssh_program(&self, settings: &Settings) -> std::path::PathBuf {
        let env = |n: &str| std::env::var(n).ok();
        env(lumen_remote_link::settings::SSH_ENV)
            .filter(|s| !s.is_empty())
            .map(Into::into)
            .or_else(|| settings.ssh.clone())
            .unwrap_or_else(|| ssh::default_ssh(env))
    }

    fn shown(&self, settings: &Settings) -> PcSettings {
        PcSettings {
            host: settings.host.clone(),
            port: settings.port,
            mac: settings.mac.map(|m| m.to_string()),
            key: self.paths.key.display().to_string(),
            key_found: self.paths.key.is_file(),
            ssh: self.ssh_program(settings).display().to_string(),
        }
    }
}

#[tauri::command]
fn link_open(app: State<'_, App>, on_event: Channel<LinkMsg>) -> Result<u32, String> {
    let settings = app.paths.load()?;
    let host = settings
        .host
        .clone()
        .ok_or("Set the TV's address under This device, on the Pairing screen")?;
    if !app.paths.key.is_file() {
        return Err(format!(
            "No key at {}: tools/remote.sh keygen makes one, and tools/remote.sh install puts it on the TV",
            app.paths.key.display()
        ));
    }
    let program = app.ssh_program(&settings);
    let argv = ssh::argv(&program, &app.paths.key, &host, settings.port);
    let id = app.next_link.fetch_add(1, Ordering::Relaxed);
    // The map is held from before ssh starts until its session is in it, so
    // a session that ends at once still finds itself there to remove.
    let mut links = lock(&app.links);
    // Open goes first: lines can come before spawn returns.
    let _ = on_event.send(LinkMsg::Open);
    let map = Arc::clone(&app.links);
    let log = app.log.clone();
    let ch = on_event.clone();
    let session = Session::spawn(
        &argv,
        Context {
            key: app.paths.key.clone(),
            host: host.clone(),
        },
        move |e| match e {
            Event::Line(line) => {
                let _ = ch.send(LinkMsg::Line { line });
            }
            Event::Closed(reason) => {
                log.line(&format!("link {id}: closed: {reason}"));
                lock(&map).remove(&id);
                let _ = ch.send(LinkMsg::Closed { reason });
            }
        },
    )
    .map_err(|e| format!("Can't run {}: {e}", program.display()))?;
    links.insert(id, session);
    drop(links);
    app.log(&format!(
        "link {id}: ssh to root@{host}:{} ({})",
        settings.port,
        program.display()
    ));
    Ok(id)
}

#[tauri::command]
fn link_send(app: State<'_, App>, id: u32, line: String) -> Result<(), String> {
    let links = lock(&app.links);
    let session = links.get(&id).ok_or("The link to the TV has closed")?;
    session.send(&line).map_err(|e| match e {
        SendError::BadLine(why) => why.to_owned(),
        SendError::Closed => "The link to the TV has closed".to_owned(),
    })
}

#[tauri::command]
fn link_close(app: State<'_, App>, id: u32) {
    // Dropped outside the lock: Closed, which takes it, follows.
    let session = lock(&app.links).remove(&id);
    if let Some(s) = session {
        s.close();
    }
}

#[tauri::command]
fn wake(app: State<'_, App>) -> Result<(), String> {
    let settings = app.paths.load()?;
    let mac = settings
        .mac
        .ok_or("Set the TV's MAC address under This device, on the Pairing screen")?;
    let targets = wol::targets(settings.host.as_deref());
    let sent = wol::wake(mac, &targets).map_err(|e| format!("Wake-on-LAN failed: {e}"))?;
    app.log(&format!("wake: {sent} packets for {mac} to {targets:?}"));
    Ok(())
}

#[tauri::command]
fn settings_get(app: State<'_, App>) -> Result<PcSettings, String> {
    Ok(app.shown(&app.paths.load()?))
}

#[tauri::command]
fn settings_set(
    app: State<'_, App>,
    host: String,
    port: String,
    mac: String,
) -> Result<PcSettings, String> {
    let mut s = app.paths.load().unwrap_or_default();
    let host = host.trim();
    check_host(host)?;
    s.host = Some(host.to_owned());
    s.port = parse_port(port.trim())?;
    s.mac = match mac.trim() {
        "" => None,
        m => Some(Mac::parse(m)?),
    };
    app.paths.save(&s)?;
    app.log(&format!("settings: host {host}, port {}", s.port));
    Ok(app.shown(&s))
}

/// The page painted after the popup opened: the timing line remote.md's
/// "warm popup in 150 ms" is read from.
#[tauri::command]
fn popup_painted(app: State<'_, App>) {
    if let Some(at) = lock(&app.popup_opened).take() {
        app.log(&format!(
            "popup: painted {} ms after the tray click",
            at.elapsed().as_millis()
        ));
    }
}

/// Whether the asking page's window is on screen, for a page that loads
/// after its window was shown and so missed the visibility event.
#[tauri::command]
fn window_visible(window: tauri::WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false)
}

/// The full window, in front: the thin remote's way to everything else.
#[tauri::command]
fn open_window(app: tauri::AppHandle) {
    tray::open_main(&app, None);
}

/// Hides the asking page's own window; the tray icon shows it again.
#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) {
    let _ = window.hide();
    tray::visibility(window.app_handle(), window.label(), false);
}

fn main() {
    let paths = match Paths::from_env(|n| std::env::var(n).ok()) {
        Some(p) => p,
        None => {
            eprintln!("lumen-remote: no USERPROFILE, so no ~/.lumen/remote/");
            std::process::exit(2);
        }
    };
    let log = AppLog::new(paths.log());
    log.line(&format!(
        "start: lumen-remote {}",
        env!("CARGO_PKG_VERSION")
    ));
    let first_run = paths.load().map(|s| s.host.is_none()).unwrap_or(true);

    tauri::Builder::default()
        .manage(App {
            paths,
            log,
            links: Arc::default(),
            next_link: AtomicU32::new(1),
            popup_opened: Mutex::new(None),
            popup_hidden: Mutex::new(None),
            side_placed: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            link_open,
            link_send,
            link_close,
            wake,
            settings_get,
            settings_set,
            popup_painted,
            window_visible,
            open_window,
            hide_window
        ])
        .on_window_event(|window, event| match (window.label(), event) {
            ("popup", WindowEvent::Focused(false)) => {
                let _ = window.hide();
                *lock(&window.state::<App>().popup_hidden) = Some(Instant::now());
                tray::visibility(window.app_handle(), "popup", false);
            }
            (label @ ("main" | "side"), WindowEvent::CloseRequested { api, .. }) => {
                // Closing a window leaves the app in the tray; Quit ends it.
                api.prevent_close();
                let _ = window.hide();
                tray::visibility(window.app_handle(), label, false);
            }
            _ => {}
        })
        .setup(move |app| {
            tray::build(app.handle())?;
            if first_run {
                tray::open_main(app.handle(), Some("pairing"));
            } else {
                // A TV is set: the remote is on the desk from the start.
                tray::show_side(app.handle());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Lumen Remote could not start");
}
