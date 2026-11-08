//! The tray icon, its menu, and placing the popup above it.

use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewWindow};

use crate::{App, lock};

/// A tray click this soon after the popup hid on losing the focus is the
/// click that took the focus: it closes the popup, not reopens it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);
/// The gap between the popup and the taskbar.
const MARGIN: f64 = 12.0;

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open window", true, None::<&str>)?;
    let pair = MenuItem::with_id(app, "pair", "Pair a phone", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pair, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("lumen")
        .tooltip("Lumen Remote")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => open_main(app, None),
            "pair" => open_main(app, Some("pairing")),
            "quit" => {
                app.state::<App>().log("quit");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popup(tray.app_handle(), rect);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Tells a page whether its window is on screen; the page lets its link go
/// a while after it hides (`host.ts`).
pub fn visibility(app: &AppHandle, label: &str, visible: bool) {
    let _ = app.emit_to(label, "lumen-visibility", visible);
}

pub fn open_main(app: &AppHandle, route: Option<&str>) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if let Some(route) = route {
        // A fixed route name, never anything from outside.
        let _ = w.eval(format!("location.hash = '#/{route}'"));
    }
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
    visibility(app, "main", true);
}

fn toggle_popup(app: &AppHandle, rect: Rect) {
    let Some(w) = app.get_webview_window("popup") else {
        return;
    };
    let state = app.state::<App>();
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        visibility(app, "popup", false);
        return;
    }
    if lock(&state.popup_hidden).is_some_and(|at| at.elapsed() < REOPEN_GUARD) {
        return;
    }
    place(&w, rect);
    *lock(&state.popup_opened) = Some(Instant::now());
    let _ = w.show();
    let _ = w.set_focus();
    visibility(app, "popup", true);
}

/// Centres the popup over the icon, on the taskbar's side of the screen,
/// inside the monitor's work area.
fn place(w: &WebviewWindow, rect: Rect) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let icon = rect.position.to_physical::<f64>(scale);
    let icon_size = rect.size.to_physical::<f64>(scale);
    let (cx, cy) = (
        icon.x + icon_size.width / 2.0,
        icon.y + icon_size.height / 2.0,
    );
    let Ok(Some(monitor)) = w.monitor_from_point(cx, cy) else {
        return;
    };
    let area = monitor.work_area();
    let size = w.outer_size().unwrap_or_default();
    let margin = MARGIN * monitor.scale_factor();
    let (ax, ay) = (f64::from(area.position.x), f64::from(area.position.y));
    let (aw, ah) = (f64::from(area.size.width), f64::from(area.size.height));
    let (pw, ph) = (f64::from(size.width), f64::from(size.height));
    let x = (cx - pw / 2.0).clamp(ax + margin, (ax + aw - pw - margin).max(ax + margin));
    let y = if cy >= ay + ah / 2.0 {
        ay + ah - ph - margin
    } else {
        ay + margin
    };
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}
