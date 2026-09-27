use std::path::Path;

use areitu_core::pipeline::build_visits;
use areitu_core::resolve::geocode::ReverseGeocoder;
use areitu_core::resolve::llm::LlmClient;
use areitu_core::resolve::Resolver;
use areitu_core::scan::scan_photos;
use rusqlite::Connection;

use areitu_core::resolve::geocode::{GooglePlaces, Nominatim};
use areitu_core::resolve::llm::{Gemini, OpenAi, Ollama};
use crate::config::{AppConfig, LlmProvider, SecretStore, GEMINI_KEY, GOOGLE_PLACES_KEY, OPENAI_KEY};

const USER_AGENT: &str = concat!(
    "AREITU-desktop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct SyncSummary {
    pub scanned: usize,
    pub scan_errors: Vec<String>,
    pub visits_created: usize,
    pub resolve_failed: usize,
}

pub fn run_sync(
    conn: &mut Connection,
    dirs: &[String],
    geocoder: &dyn ReverseGeocoder,
    llm: Option<&dyn LlmClient>,
    min_confidence: f64,
) -> Result<SyncSummary, String> {
    let mut summary = SyncSummary::default();
    for dir in dirs {
        match scan_photos(conn, Path::new(dir)) {
            Ok(report) => summary.scanned += report.inserted,
            Err(e) => summary.scan_errors.push(format!("{dir}: {e}")),
        }
    }
    let resolver = Resolver { geocoder, llm, min_confidence };
    let report = build_visits(conn, &resolver).map_err(|e| e.to_string())?;
    summary.visits_created = report.visits;
    summary.resolve_failed = report.failed;
    Ok(summary)
}

pub fn build_geocoder(config: &AppConfig, secrets: &dyn SecretStore) -> Box<dyn ReverseGeocoder> {
    if config.google_places_enabled {
        if let Some(key) = secrets.get(GOOGLE_PLACES_KEY) {
            if let Ok(g) = GooglePlaces::new(&key) {
                return Box::new(g);
            }
        }
    }
    Box::new(Nominatim::new(USER_AGENT).expect("building a Nominatim client never fails"))
}

pub fn build_llm(config: &AppConfig, secrets: &dyn SecretStore) -> Option<Box<dyn LlmClient>> {
    match config.llm_provider {
        LlmProvider::None => None,
        LlmProvider::Ollama => {
            if config.ollama_model.trim().is_empty() {
                return None;
            }
            Ollama::new(&config.ollama_url, &config.ollama_model)
                .ok()
                .map(|c| Box::new(c) as Box<dyn LlmClient>)
        }
        LlmProvider::OpenAi => secrets
            .get(OPENAI_KEY)
            .and_then(|key| OpenAi::new(&key, &config.openai_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
        LlmProvider::Gemini => secrets
            .get(GEMINI_KEY)
            .and_then(|key| Gemini::new(&key, &config.gemini_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
    }
}

pub fn run_sync_with_config(
    conn: &mut Connection,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let geocoder = build_geocoder(config, secrets);
    let llm = build_llm(config, secrets);
    run_sync(conn, &config.watched_dirs, geocoder.as_ref(), llm.as_deref(), config.min_confidence)
}

/// バックグラウンド同期（ポーリングスレッド・トレイの「今すぐ同期」・sync_now コマンド）は
/// 必ずこの関数を通す。`AppState.conn` を保持したまま長時間のネットワーク呼び出しを行うと、
/// その間 list_places / visits_of / rename_place などの UI コマンドがブロックしてしまうため、
/// 同期専用に自分だけの Connection を新しく開いて実行する。
/// 書き込みは短命なトランザクション単位で行われ、`db::init` で設定した busy_timeout により
/// 同じ DB ファイルへの別接続とは待ち合わせで解決する。
pub fn sync_on_own_connection(
    db_path: &Path,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let mut conn = areitu_core::db::open(db_path).map_err(|e| e.to_string())?;
    run_sync_with_config(&mut conn, config, secrets)
}

use std::time::Duration;
use tauri::{AppHandle, Manager};

const POLL_CHECK_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn_poll_thread(app: AppHandle) {
    std::thread::spawn(move || {
        let mut elapsed = Duration::ZERO;
        loop {
            std::thread::sleep(POLL_CHECK_INTERVAL);
            elapsed += POLL_CHECK_INTERVAL;

            let state = app.state::<crate::AppState>();
            let config = crate::config::load_config(&state.config_path);
            let target = Duration::from_secs(u64::from(config.poll_interval_minutes.max(1)) * 60);
            if elapsed < target {
                continue;
            }
            elapsed = Duration::ZERO;

            let _ = sync_on_own_connection(&state.db_path, &config, &crate::config::KeyringSecretStore);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use areitu_core::db::open_in_memory;
    use areitu_core::resolve::geocode::PoiGuess;
    use areitu_core::testutil_ext::candidate;
    use areitu_core::Result;

    struct FakeGeocoder(Option<PoiGuess>);
    impl ReverseGeocoder for FakeGeocoder {
        fn reverse(&self, _lat: f64, _lon: f64) -> Result<Option<PoiGuess>> {
            Ok(self.0.clone())
        }
    }

    fn geo(name: &str) -> FakeGeocoder {
        FakeGeocoder(Some(PoiGuess {
            name: Some(name.to_owned()),
            display_name: name.to_owned(),
            category: None,
        }))
    }

    fn photo_dir_with_no_photos() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn missing_dir_is_reported_but_does_not_abort_other_dirs() {
        let mut c = open_in_memory().unwrap();
        let good = photo_dir_with_no_photos();
        let g = geo("カフェ丸の内");
        let dirs = vec!["/no/such/dir/areitu".to_owned(), good.path().to_string_lossy().into_owned()];
        let summary = run_sync(&mut c, &dirs, &g, None, 0.6).unwrap();
        assert_eq!(summary.scan_errors.len(), 1);
        assert!(summary.scan_errors[0].starts_with("/no/such/dir/areitu"));
        assert_eq!(summary.visits_created, 0);
    }

    #[test]
    fn empty_dirs_list_still_builds_visits_from_calendar_ingest() {
        let mut c = open_in_memory().unwrap();
        // カレンダー取り込み済みの未処理ログを1件だけ模擬する
        areitu_core::store::upsert_raw_log(
            &c,
            &areitu_core::model::RawLog {
                source: areitu_core::model::Source::Calendar,
                source_id: "e1".into(),
                occurred_at: candidate(&[]).started_at,
                ended_at: Some(candidate(&[]).ended_at),
                lat: Some(35.0),
                lon: Some(139.0),
                text: Some("ランチ".into()),
            },
        )
        .unwrap();
        let g = geo("カフェ丸の内");
        let summary = run_sync(&mut c, &[], &g, None, 0.6).unwrap();
        assert_eq!(summary.scanned, 0);
        assert_eq!(summary.visits_created, 1);
    }

    use crate::config::{FakeSecretStore, LlmProvider, OPENAI_KEY};

    #[test]
    fn openai_provider_without_api_key_degrades_to_nominatim_only() {
        let mut c = open_in_memory().unwrap();
        // キーは設定しない
        let config = AppConfig { llm_provider: LlmProvider::OpenAi, ..AppConfig::default() };
        let secrets = FakeSecretStore::new();
        // ネットワークに出る前提のテストは避け、build_llm が None を返すことだけを確認する
        assert!(build_llm(&config, &secrets).is_none());
        let summary = run_sync_with_config(&mut c, &config, &secrets);
        assert!(summary.is_ok());
    }

    #[test]
    fn openai_provider_with_api_key_builds_a_client() {
        let config = AppConfig {
            llm_provider: LlmProvider::OpenAi,
            openai_model: "gpt-4o-mini".to_owned(),
            ..AppConfig::default()
        };
        let secrets = FakeSecretStore::new();
        secrets.set(OPENAI_KEY, "sk-test").unwrap();
        assert!(build_llm(&config, &secrets).is_some());
    }

    #[test]
    fn ollama_provider_with_blank_model_degrades_to_none() {
        let config = AppConfig {
            llm_provider: LlmProvider::Ollama,
            ollama_model: String::new(),
            ..AppConfig::default()
        };
        let secrets = FakeSecretStore::new();
        assert!(build_llm(&config, &secrets).is_none());
    }

    #[test]
    fn sync_on_own_connection_does_not_need_the_shared_lock() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("areitu.db");
        // AppState が保持するのと同じ DB ファイルへの共有接続を用意し、
        // テストスレッドでロックを握ったまま sync_on_own_connection を呼ぶ。
        let shared = Arc::new(Mutex::new(areitu_core::db::open(&db_path).unwrap()));
        let _guard = shared.lock().unwrap();

        let config = AppConfig::default(); // watched_dirs は空
        let secrets = crate::config::FakeSecretStore::new();
        let summary = sync_on_own_connection(&db_path, &config, &secrets);

        assert!(summary.is_ok());
    }

    #[test]
    fn concurrent_calls_do_not_deadlock() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let shared = Arc::new(Mutex::new(open_in_memory().unwrap()));
        let g = Arc::new(geo("カフェ丸の内"));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let shared = Arc::clone(&shared);
            let g = Arc::clone(&g);
            handles.push(thread::spawn(move || {
                let mut conn = shared.lock().unwrap();
                run_sync(&mut conn, &[], g.as_ref(), None, 0.6).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // デッドロックせずに全スレッドが完了すればテストは成功
    }
}
