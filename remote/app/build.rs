// The app's commands are listed here, so each needs a permission and only
// the capability in capabilities/default.json grants them.
fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "link_open",
        "link_send",
        "link_close",
        "wake",
        "settings_get",
        "settings_set",
        "popup_painted",
        "window_visible",
        "open_window",
        "hide_window",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri-build");
}
