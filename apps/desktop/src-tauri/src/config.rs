use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    None,
    Ollama,
    OpenAi,
    Gemini,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

pub const OPENAI_KEY: &str = "openai_api_key";
pub const GEMINI_KEY: &str = "gemini_api_key";
pub const GOOGLE_PLACES_KEY: &str = "google_places_api_key";
const SERVICE: &str = "AREITU";

pub trait SecretStore {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, key).ok()?.get_password().ok()
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
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
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
    fn fake_secret_store_set_get_delete() {
        let store = FakeSecretStore::new();
        assert_eq!(store.get(OPENAI_KEY), None);
        store.set(OPENAI_KEY, "sk-test").unwrap();
        assert_eq!(store.get(OPENAI_KEY).as_deref(), Some("sk-test"));
        store.delete(OPENAI_KEY).unwrap();
        assert_eq!(store.get(OPENAI_KEY), None);
    }
}
