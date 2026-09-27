use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "AREITU を開く", true, None::<&str>)?;
    let sync_item = MenuItem::with_id(app, "sync_now", "今すぐ同期", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &sync_item, &quit_item])?;

    TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app: &AppHandle, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "sync_now" => {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = app.state::<crate::AppState>();
                    let config = crate::config::load_config(&state.config_path);
                    if let (Ok(geocoder), Ok(mut conn)) = (
                        areitu_core::resolve::geocode::Nominatim::new(
                            "AREITU-desktop-tray/0.1 (+https://github.com/ikeikeikeda66/AREITU)",
                        ),
                        state.conn.lock(),
                    ) {
                        let _ = crate::sync::run_sync(
                            &mut conn,
                            &config.watched_dirs,
                            &geocoder,
                            None,
                            config.min_confidence,
                        );
                    };
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
