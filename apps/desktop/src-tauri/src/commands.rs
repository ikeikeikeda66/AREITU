use tauri::State;

use crate::config::{
    config_exists, key_status, load_config, save_config, AppConfig, KeyStatus, KeyringSecretStore, LlmProvider,
    SecretStore, GEMINI_KEY, GOOGLE_PLACES_KEY, OPENAI_KEY,
};
use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
use crate::sync::{sync_on_own_connection_locked, SyncSummary};
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
    let config = load_config(&state.config_path);
    // AppState.conn を保持したままネットワークを伴う同期を行うと、その間 UI コマンドが
    // すべてブロックされるため、同期専用の接続を別途開いて実行する。sync_lock は
    // Drive 同期（drive_sync_now・ポーリングサイクル末尾）との相互排除のために取る。
    sync_on_own_connection_locked(&state.sync_lock, &state.db_path, &state.calendar_state_path, &config, &KeyringSecretStore)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub watched_dirs: Vec<String>,
    pub llm_provider: LlmProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
    pub gemini_model: String,
    pub google_places_enabled: bool,
    pub min_confidence: f64,
    pub poll_interval_minutes: u32,
    pub calendar_enabled: bool,
    pub openai_key_status: KeyStatus,
    pub gemini_key_status: KeyStatus,
    pub google_places_key_status: KeyStatus,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsDto {
    pub watched_dirs: Vec<String>,
    pub llm_provider: LlmProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
    pub gemini_model: String,
    pub google_places_enabled: bool,
    pub min_confidence: f64,
    pub poll_interval_minutes: u32,
    pub calendar_enabled: bool,
    /// None なら変更しない。Some("") ならキーチェーンから削除する。Some(value) (非空) なら保存する。
    pub openai_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub google_places_api_key: Option<String>,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> SettingsDto {
    let config = load_config(&state.config_path);
    let secrets = KeyringSecretStore;
    SettingsDto {
        watched_dirs: config.watched_dirs,
        llm_provider: config.llm_provider,
        ollama_url: config.ollama_url,
        ollama_model: config.ollama_model,
        openai_model: config.openai_model,
        gemini_model: config.gemini_model,
        google_places_enabled: config.google_places_enabled,
        min_confidence: config.min_confidence,
        poll_interval_minutes: config.poll_interval_minutes,
        calendar_enabled: config.calendar_enabled,
        openai_key_status: key_status(&secrets, OPENAI_KEY),
        gemini_key_status: key_status(&secrets, GEMINI_KEY),
        google_places_key_status: key_status(&secrets, GOOGLE_PLACES_KEY),
    }
}

fn apply_secret(secrets: &dyn SecretStore, key: &str, value: Option<String>) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(v) if v.is_empty() => secrets.delete(key),
        Some(v) => secrets.set(key, &v),
    }
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: SaveSettingsDto) -> Result<(), String> {
    let config = AppConfig {
        watched_dirs: settings.watched_dirs,
        llm_provider: settings.llm_provider,
        ollama_url: settings.ollama_url,
        ollama_model: settings.ollama_model,
        openai_model: settings.openai_model,
        gemini_model: settings.gemini_model,
        google_places_enabled: settings.google_places_enabled,
        min_confidence: settings.min_confidence,
        poll_interval_minutes: settings.poll_interval_minutes,
        calendar_enabled: settings.calendar_enabled,
    };
    let secrets = KeyringSecretStore;
    apply_secret(&secrets, OPENAI_KEY, settings.openai_api_key)?;
    apply_secret(&secrets, GEMINI_KEY, settings.gemini_api_key)?;
    apply_secret(&secrets, GOOGLE_PLACES_KEY, settings.google_places_api_key)?;
    save_config(&state.config_path, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn setup_completed(state: State<AppState>) -> bool {
    config_exists(&state.config_path)
}

#[tauri::command]
pub fn import_timeline_file(state: State<AppState>, path: String) -> Result<usize, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    areitu_core::timeline::ingest_timeline_file(&conn, std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_dto_serializes_as_camel_case() {
        let dto = SettingsDto {
            watched_dirs: vec!["/photos".to_string()],
            llm_provider: LlmProvider::Ollama,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "llama3".to_string(),
            openai_model: "gpt-4o-mini".to_string(),
            gemini_model: "gemini-1.5-flash".to_string(),
            google_places_enabled: false,
            min_confidence: 0.6,
            poll_interval_minutes: 30,
            calendar_enabled: false,
            openai_key_status: KeyStatus::NotSet,
            gemini_key_status: KeyStatus::NotSet,
            google_places_key_status: KeyStatus::Unavailable,
        };
        let json = serde_json::to_value(&dto).unwrap();
        let obj = json.as_object().unwrap();
        for key in [
            "watchedDirs",
            "llmProvider",
            "ollamaUrl",
            "ollamaModel",
            "openaiModel",
            "geminiModel",
            "googlePlacesEnabled",
            "minConfidence",
            "pollIntervalMinutes",
            "calendarEnabled",
            "openaiKeyStatus",
            "geminiKeyStatus",
            "googlePlacesKeyStatus",
        ] {
            assert!(obj.contains_key(key), "missing {key}: {json}");
        }
        assert!(!obj.contains_key("watched_dirs"), "snake_case leaked: {json}");
        assert_eq!(obj.get("googlePlacesKeyStatus").unwrap(), "unavailable");
    }

    #[test]
    fn save_settings_dto_deserializes_camel_case_payload() {
        let payload = serde_json::json!({
            "watchedDirs": ["/photos"],
            "llmProvider": "openai",
            "ollamaUrl": "http://localhost:11434",
            "ollamaModel": "",
            "openaiModel": "gpt-4o-mini",
            "geminiModel": "gemini-1.5-flash",
            "googlePlacesEnabled": true,
            "minConfidence": 0.6,
            "pollIntervalMinutes": 30,
            "calendarEnabled": true,
            "openaiApiKey": "sk-test",
            "geminiApiKey": null,
            "googlePlacesApiKey": null,
        });
        let dto: SaveSettingsDto = serde_json::from_value(payload).unwrap();
        assert_eq!(dto.watched_dirs, vec!["/photos".to_string()]);
        assert_eq!(dto.llm_provider, LlmProvider::OpenAi);
        assert!(dto.google_places_enabled);
        assert!(dto.calendar_enabled);
        assert_eq!(dto.openai_api_key.as_deref(), Some("sk-test"));
        assert_eq!(dto.gemini_api_key, None);
    }

    #[test]
    fn setup_completed_reflects_whether_config_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!crate::config::config_exists(&dir.path().join("config.json")));
    }

    #[test]
    fn import_timeline_file_delegates_to_the_core_ingest_function() {
        // コマンドは AppState.conn のロックと ingest_timeline_file への委譲のみ。
        // パースは areitu-core の timeline::tests で検証済みなので、ここでは
        // 委譲先のシグネチャが変わっていないことだけを型で確認する。
        fn _assert_signature(f: fn(&rusqlite::Connection, &std::path::Path) -> areitu_core::Result<usize>) {
            let _ = f;
        }
        _assert_signature(areitu_core::timeline::ingest_timeline_file);
    }
}
