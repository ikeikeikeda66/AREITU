use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    None,
    Ollama,
    #[serde(rename = "openai")]
    OpenAi,
    Gemini,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
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
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            watched_dirs: Vec::new(),
            llm_provider: LlmProvider::None,
            ollama_url: "http://localhost:11434".to_owned(),
            ollama_model: String::new(),
            openai_model: "gpt-4o-mini".to_owned(),
            gemini_model: "gemini-1.5-flash".to_owned(),
            google_places_enabled: false,
            min_confidence: 0.6,
            poll_interval_minutes: 30,
            calendar_enabled: false,
        }
    }
}

pub fn load_config(path: &Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &AppConfig) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(config).expect("AppConfig is always serializable");
    std::fs::write(path, json)
}

pub fn config_exists(path: &Path) -> bool {
    path.exists()
}

pub const OPENAI_KEY: &str = "openai_api_key";
pub const GEMINI_KEY: &str = "gemini_api_key";
pub const GOOGLE_PLACES_KEY: &str = "google_places_api_key";
const SERVICE: &str = "AREITU";

pub trait SecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    Set,
    NotSet,
    Unavailable,
}

pub fn key_status(secrets: &dyn SecretStore, key: &str) -> KeyStatus {
    match secrets.get(key) {
        Ok(Some(_)) => KeyStatus::Set,
        Ok(None) => KeyStatus::NotSet,
        Err(_) => KeyStatus::Unavailable,
    }
}

pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, key)
            .map_err(|e| e.to_string())?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, key) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
pub struct FakeSecretStore(pub std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl FakeSecretStore {
    pub fn new() -> Self {
        FakeSecretStore(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl SecretStore for FakeSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// キーチェーンがロック中・アクセス不可の状態を模す test-only 実装。
#[cfg(test)]
pub struct AlwaysUnavailableSecretStore;

#[cfg(test)]
impl SecretStore for AlwaysUnavailableSecretStore {
    fn get(&self, _key: &str) -> Result<Option<String>, String> {
        Err("keychain is locked".to_owned())
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Err("keychain is locked".to_owned())
    }

    fn delete(&self, _key: &str) -> Result<(), String> {
        Err("keychain is locked".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(load_config(&path), AppConfig::default());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_config(&path), AppConfig::default());
    }

    #[test]
    fn config_from_before_calendar_enabled_existed_preserves_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // 現行フィールドすべてを含み、calendar_enabled だけ欠けた古い config.json を模す。
        std::fs::write(
            &path,
            r#"{
                "watched_dirs": ["/photos"],
                "llm_provider": "none",
                "ollama_url": "http://localhost:11434",
                "ollama_model": "",
                "openai_model": "gpt-4o-mini",
                "gemini_model": "gemini-1.5-flash",
                "google_places_enabled": false,
                "min_confidence": 0.6,
                "poll_interval_minutes": 15
            }"#,
        )
        .unwrap();
        let config = load_config(&path);
        assert_eq!(config.watched_dirs, vec!["/photos".to_owned()]);
        assert_eq!(config.poll_interval_minutes, 15);
        assert!(!config.calendar_enabled);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut config = AppConfig::default();
        config.watched_dirs.push("/photos".to_owned());
        config.llm_provider = LlmProvider::OpenAi;
        config.poll_interval_minutes = 15;
        save_config(&path, &config).unwrap();
        assert_eq!(load_config(&path), config);
    }

    #[test]
    fn llm_provider_open_ai_round_trips_as_openai() {
        let json = serde_json::to_string(&LlmProvider::OpenAi).unwrap();
        assert_eq!(json, "\"openai\"");
        let back: LlmProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LlmProvider::OpenAi);
    }

    #[test]
    fn fake_secret_store_set_get_delete() {
        let store = FakeSecretStore::new();
        assert_eq!(store.get(OPENAI_KEY).unwrap(), None);
        store.set(OPENAI_KEY, "sk-test").unwrap();
        assert_eq!(store.get(OPENAI_KEY).unwrap().as_deref(), Some("sk-test"));
        store.delete(OPENAI_KEY).unwrap();
        assert_eq!(store.get(OPENAI_KEY).unwrap(), None);
    }

    #[test]
    fn calendar_enabled_defaults_to_false() {
        assert!(!AppConfig::default().calendar_enabled);
    }

    #[test]
    fn calendar_enabled_round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = AppConfig { calendar_enabled: true, ..AppConfig::default() };
        save_config(&path, &config).unwrap();
        assert!(load_config(&path).calendar_enabled);
    }

    #[test]
    fn fake_secret_store_get_distinguishes_not_set_from_unavailable() {
        let store = FakeSecretStore::new();
        assert_eq!(store.get(OPENAI_KEY).unwrap(), None);
        store.set(OPENAI_KEY, "sk-test").unwrap();
        assert_eq!(store.get(OPENAI_KEY).unwrap().as_deref(), Some("sk-test"));
        let locked = AlwaysUnavailableSecretStore;
        assert!(locked.get(OPENAI_KEY).is_err());
    }

    #[test]
    fn config_exists_reflects_whether_the_file_has_been_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert!(!config_exists(&path));
        save_config(&path, &AppConfig::default()).unwrap();
        assert!(config_exists(&path));
    }
}
