mod commands;
mod config;
mod logic;
mod sync;
mod tray;

use std::sync::Mutex;

use tauri::Manager;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub config_path: std::path::PathBuf,
    pub db_path: std::path::PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            let db_path = data_dir.join("areitu.db");
            let conn = areitu_core::db::open(&db_path).expect("failed to open areitu.db");

            let config_dir = app
                .path()
                .app_config_dir()
                .expect("failed to resolve app config dir");
            std::fs::create_dir_all(&config_dir).expect("failed to create app config dir");
            let config_path = config_dir.join("config.json");

            app.manage(AppState { conn: Mutex::new(conn), config_path, db_path });

            crate::tray::setup_tray(app.handle())?;
            crate::sync::spawn_poll_thread(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
            commands::sync_now,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
