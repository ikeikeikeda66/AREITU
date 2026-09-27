mod commands;
#[allow(dead_code)] // consumed starting in Task 4
mod config;
mod logic;

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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            let conn = areitu_core::db::open(&data_dir.join("areitu.db"))
                .expect("failed to open areitu.db");

            let config_dir = app
                .path()
                .app_config_dir()
                .expect("failed to resolve app config dir");
            std::fs::create_dir_all(&config_dir).expect("failed to create app config dir");
            let config_path = config_dir.join("config.json");

            app.manage(AppState { conn: Mutex::new(conn), config_path });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
