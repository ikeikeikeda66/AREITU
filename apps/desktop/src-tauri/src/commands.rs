use tauri::State;

use crate::config::KeyringSecretStore;
use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
use crate::sync::{sync_on_own_connection, SyncSummary};
use crate::AppState;

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
    let config = crate::config::load_config(&state.config_path);
    // AppState.conn を保持したままネットワークを伴う同期を行うと、その間 UI コマンドが
    // すべてブロックされるため、同期専用の接続を別途開いて実行する。
    sync_on_own_connection(&state.db_path, &config, &KeyringSecretStore)
}
