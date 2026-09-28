mod commands;
mod config;
mod google;
mod logic;
mod sync;
mod tray;

use std::sync::Mutex;

use tauri::Manager;

pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub config_path: std::path::PathBuf,
    pub db_path: std::path::PathBuf,
    pub calendar_state_path: std::path::PathBuf,
    /// 写真/カレンダー同期（ポーリングスレッド・トレイの「今すぐ同期」・
    /// `sync_now` コマンド）と Drive 同期（`drive_sync_now` コマンド・
    /// ポーリングサイクル末尾の Drive 同期）を相互排除するためのロック。
    /// ロック順序は必ず sync_lock → conn（conn は `with_db_closed` の内側でのみ
    /// 取る）。UI の読み取り専用コマンド（list_places / visits_of / rename_place）
    /// はこのロックを取らない。
    pub sync_lock: Mutex<()>,
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
            let calendar_state_path = data_dir.join("google-calendar-state.json");

            app.manage(AppState {
                conn: Mutex::new(conn),
                config_path,
                db_path,
                calendar_state_path,
                sync_lock: Mutex::new(()),
            });

            crate::tray::setup_tray(app.handle())?;
            crate::sync::spawn_poll_thread(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
            commands::sync_now,
            commands::get_settings,
            commands::save_settings,
            commands::setup_completed,
            commands::import_timeline_file,
            google::google_sign_in,
            google::google_sign_out,
            google::google_status,
            google::drive_sync_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
