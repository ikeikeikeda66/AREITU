use tauri::State;

use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
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
