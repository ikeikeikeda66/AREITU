use std::path::Path;

use areitu_core::pipeline::build_visits;
use areitu_core::resolve::geocode::ReverseGeocoder;
use areitu_core::resolve::llm::LlmClient;
use areitu_core::resolve::Resolver;
use areitu_core::scan::scan_photos;
use rusqlite::Connection;

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

            let geocoder = match areitu_core::resolve::geocode::Nominatim::new(
                "AREITU-desktop-poll/0.1 (+https://github.com/ikeikeikeda66/AREITU)",
            ) {
                Ok(g) => g,
                Err(_) => continue,
            };
            if let Ok(mut conn) = state.conn.lock() {
                let _ = run_sync(&mut conn, &config.watched_dirs, &geocoder, None, config.min_confidence);
            };
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
