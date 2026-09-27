use tauri::State;

use crate::config::{load_config, KeyringSecretStore};
use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
use crate::sync::{run_sync, SyncSummary};
use crate::AppState;
use areitu_core::resolve::geocode::Nominatim;

const USER_AGENT: &str = concat!(
    "AREITU-desktop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

#[tauri::command]
pub fn list_places(
    state: State<AppState>,
    sort: String,
    keyword: Option<String>,
) -> Result<Vec<PlaceDto>, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    list_places_dto(&conn, &sort, keyword.as_deref())
}

#[tauri::command]
pub fn visits_of(state: State<AppState>, place_id: i64) -> Result<Vec<VisitDto>, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    visits_of_dto(&conn, place_id)
}

#[tauri::command]
pub fn rename_place(state: State<AppState>, place_id: i64, name: String) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    rename_place_dto(&mut conn, place_id, &name)
}

#[tauri::command]
pub fn sync_now(state: State<AppState>) -> Result<SyncSummary, String> {
    let config = load_config(&state.config_path);
    let geocoder = Nominatim::new(USER_AGENT).map_err(|e| e.to_string())?;
    let mut conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    // LLM プロバイダの切り替えは Task 11 (build_geocoder/build_llm) で完成させる。
    // ここでは Nominatim 固定・LLM 無しで動作する最小実装。
    let _ = KeyringSecretStore; // Task 11 で使用する
    run_sync(&mut conn, &config.watched_dirs, &geocoder, None, config.min_confidence)
}
