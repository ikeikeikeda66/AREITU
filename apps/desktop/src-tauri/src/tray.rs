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
                    let _ = crate::sync::sync_on_own_connection_locked(
                        &state.sync_lock,
                        &state.db_path,
                        &state.calendar_state_path,
                        &config,
                        &crate::config::KeyringSecretStore,
                    );
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
