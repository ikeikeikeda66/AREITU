use areitu_google::auth::{client_credentials_from_env, AuthStatus, GoogleAuth, SystemBrowser};
use areitu_google::drive::DriveClient;
use areitu_google::keychain::KeyringStore;
use areitu_google::oauth::TokenClient;
use areitu_google::sync::{sync_now, DbSwapGuard, SyncContext, SyncOutcome};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

use crate::config::AppConfig;

/// Closes the shared connection while `f` runs so `f` may replace the DB file, then reopens it.
pub fn with_db_closed<T>(
    conn: &Mutex<Connection>,
    db_path: &Path,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = conn.lock().map_err(|e| e.to_string())?;
    *guard = Connection::open_in_memory().map_err(|e| e.to_string())?;
    let result = f();
    *guard = areitu_core::db::open(db_path).map_err(|e| e.to_string())?;
    result
}

fn build_auth() -> Result<GoogleAuth<KeyringStore, SystemBrowser>, String> {
    let creds = client_credentials_from_env().map_err(|e| e.to_string())?;
    let token_client = TokenClient::new().map_err(|e| e.to_string())?;
    Ok(GoogleAuth::new(KeyringStore, SystemBrowser, token_client, creds))
}

#[cfg_attr(not(test), allow(dead_code))]
const CALENDAR_ID: &str = "primary";

pub fn calendar_scope_for(config: &AppConfig) -> String {
    if config.calendar_enabled {
        format!("{} {}", areitu_google::SCOPE_DRIVE_APPDATA, areitu_google::SCOPE_CALENDAR_READONLY)
    } else {
        areitu_google::SCOPE_DRIVE_APPDATA.to_owned()
    }
}

/// カレンダー取り込みの唯一の入口。`config.calendar_enabled` が false なら何もしない。
/// サインインしていない・クライアント資格情報が未設定などの理由でアクセストークンが
/// 取れない場合も、エラーを `summary.errors` に積んで返すだけで、呼び出し元の
/// 写真同期・visit 構築は止めない。
#[cfg_attr(not(test), allow(dead_code))]
pub fn ingest_calendar(conn: &Connection, config: &AppConfig, calendar_state_path: &Path) -> CalendarIngestSummary {
    let mut summary = CalendarIngestSummary::default();
    if !config.calendar_enabled {
        return summary;
    }
    let auth = match build_auth() {
        Ok(a) => a,
        Err(e) => {
            summary.errors.push(e);
            return summary;
        }
    };
    let access_token = match auth.access_token() {
        Ok(t) => t,
        Err(e) => {
            summary.errors.push(format!("Google カレンダーに接続できません: {e}"));
            return summary;
        }
    };
    let client = match areitu_google::calendar::CalendarClient::new() {
        Ok(c) => c,
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };
    let mut state = match areitu_google::state::load_calendar_state(calendar_state_path) {
        Ok(s) => s,
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };

    summary = ingest_calendar_page_bodies(conn, &client, &access_token, CALENDAR_ID, &mut state);
    if let Err(e) = areitu_google::state::save_calendar_state(calendar_state_path, &state) {
        summary.errors.push(e.to_string());
    }
    summary
}

/// ブラウザでの認可完了までブロックするため、`spawn_blocking` で Tauri の
/// 非同期ランタイム上のワーカースレッドに逃がす。こうしないと呼び出し中
/// フロントエンドの他の `invoke` 呼び出しがすべて詰まってしまう。
#[tauri::command]
pub async fn google_sign_in(state: State<'_, crate::AppState>) -> Result<(), String> {
    let config = crate::config::load_config(&state.config_path);
    tauri::async_runtime::spawn_blocking(move || {
        let scope = calendar_scope_for(&config);
        build_auth()?.sign_in(&scope).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn google_sign_out() -> Result<(), String> {
    build_auth()?.sign_out().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn google_status() -> Result<String, String> {
    let status = build_auth()?.status().map_err(|e| e.to_string())?;
    Ok(match status {
        AuthStatus::SignedIn => "signed_in".to_owned(),
        AuthStatus::SignedOut => "signed_out".to_owned(),
    })
}

/// `areitu_google::sync::DbSwapGuard` をデスクトップアプリの共有 DB コネクション
/// に対して実装するアダプタ。`sync_now` はダウンロード経路で実際にファイルを
/// 入れ替える瞬間だけこれを呼ぶので、トークン取得・Drive の list/upload/download・
/// ハッシュ計算の間は `conn` を開けたままにでき、`AppState.conn` をロックする
/// UI コマンド（`list_places` 等）が Drive 同期のネットワーク往復の間ずっと
/// ブロックされることがなくなる。
struct ConnSwapGuard<'a> {
    conn: &'a Mutex<Connection>,
    db_path: &'a Path,
}

impl DbSwapGuard for ConnSwapGuard<'_> {
    fn with_db_closed(&self, f: &mut dyn FnMut() -> areitu_google::Result<()>) -> areitu_google::Result<()> {
        with_db_closed(self.conn, self.db_path, || f().map_err(|e| e.to_string()))
            .map_err(areitu_google::Error::Invalid)
    }
}

/// Drive 同期の唯一の入口。`sync_lock` を最初に取り、そのロックを関数全体で
/// 保持したまま `sync_now` を呼ぶ。共有コネクション（`conn`）は
/// `ConnSwapGuard` 経由で `swap_db_files` の間だけ閉じられるので、ロック順序
/// （sync_lock → conn）はこれまでと変わらない — ポーリングスレッド・トレイの
/// 「今すぐ同期」・`sync_now` コマンドが握る `sync_lock` と衝突すると、写真/
/// カレンダー同期が書き込み中に DB ファイルの入れ替えが走ってしまう。
pub fn drive_sync_locked(
    sync_lock: &Mutex<()>,
    conn: &Mutex<Connection>,
    db_path: &Path,
    state_path: &Path,
) -> Result<String, String> {
    let _sync_guard = sync_lock.lock().map_err(|e| e.to_string())?;
    let auth = build_auth()?;
    let access_token = auth.access_token().map_err(|e| e.to_string())?;
    let drive = DriveClient::new().map_err(|e| e.to_string())?;
    let (db_path, state_path) = (PathBuf::from(db_path), PathBuf::from(state_path));
    let swap_guard = ConnSwapGuard { conn, db_path: &db_path };
    let outcome = sync_now(&SyncContext {
        drive: &drive,
        access_token: &access_token,
        db_path: &db_path,
        state_path: &state_path,
        swap_guard: &swap_guard,
    })
    .map_err(|e| e.to_string())?;
    Ok(match outcome {
        SyncOutcome::NoOp => "no_op".to_owned(),
        SyncOutcome::Uploaded => "uploaded".to_owned(),
        SyncOutcome::Downloaded => "downloaded".to_owned(),
        SyncOutcome::Conflict { conflict_backup_name } => format!("conflict:{conflict_backup_name}"),
    })
}

pub fn sync_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok((dir.join("areitu.db"), dir.join("google-sync-state.json")))
}

#[tauri::command]
pub fn drive_sync_now(app: AppHandle) -> Result<String, String> {
    let (db_path, state_path) = sync_paths(&app)?;
    let state = app.state::<crate::AppState>();
    drive_sync_locked(&state.sync_lock, &state.conn, &db_path, &state_path)
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct CalendarIngestSummary {
    pub events_synced: usize,
    pub events_removed: usize,
    pub errors: Vec<String>,
}

/// カレンダーの1同期サイクル分のページ本文を raw_logs に反映する純粋なロジック。
/// Google 認証・アクセストークン取得・状態ファイルの読み書きは呼び出し元（`ingest_calendar`）
/// の責務とし、ここでは渡された `api`/`access_token`/`state` だけを使う。
#[cfg_attr(not(test), allow(dead_code))]
pub fn ingest_calendar_page_bodies(
    conn: &Connection,
    api: &dyn areitu_google::calendar::CalendarApi,
    access_token: &str,
    calendar_id: &str,
    state: &mut areitu_google::state::CalendarSyncState,
) -> CalendarIngestSummary {
    let mut summary = CalendarIngestSummary::default();
    let fetched = match areitu_google::calendar::fetch_all_pages(api, access_token, calendar_id, state.sync_token.as_deref()) {
        Ok(f) => f,
        Err(areitu_google::Error::SyncTokenExpired) => {
            state.sync_token = None;
            match areitu_google::calendar::fetch_all_pages(api, access_token, calendar_id, None) {
                Ok(f) => f,
                Err(e) => {
                    summary.errors.push(e.to_string());
                    return summary;
                }
            }
        }
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };

    for body in &fetched.bodies {
        match areitu_core::calendar::parse_events(body) {
            Ok(events) => {
                for e in &events {
                    match areitu_core::store::upsert_raw_log(conn, &areitu_core::calendar::to_raw_log(e)) {
                        Ok(()) => summary.events_synced += 1,
                        Err(err) => summary.errors.push(err.to_string()),
                    }
                }
            }
            Err(e) => summary.errors.push(e.to_string()),
        }
        match areitu_core::calendar::parse_cancelled_source_ids(body) {
            Ok(ids) => {
                for id in ids {
                    match areitu_core::store::delete_unassigned_raw_log(conn, areitu_core::model::Source::Calendar, &id) {
                        Ok(true) => summary.events_removed += 1,
                        Ok(false) => {}
                        Err(err) => summary.errors.push(err.to_string()),
                    }
                }
            }
            Err(e) => summary.errors.push(e.to_string()),
        }
    }

    if fetched.next_sync_token.is_some() {
        state.sync_token = fetched.next_sync_token;
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(conn: &Mutex<Connection>) -> i64 {
        conn.lock().unwrap().query_row("SELECT COUNT(*) FROM places", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn reopens_db_after_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("areitu.db");
        let remote = dir.path().join("remote.db");
        let conn = Mutex::new(areitu_core::db::open(&live).unwrap());
        let other = areitu_core::db::open(&remote).unwrap();
        other.execute("INSERT INTO places (name, lat, lon) VALUES ('X', 35.0, 139.0)", []).unwrap();
        drop(other);

        let out = with_db_closed(&conn, &live, || {
            std::fs::copy(&remote, &live).map_err(|e| e.to_string())?;
            Ok("downloaded")
        })
        .unwrap();

        assert_eq!(out, "downloaded");
        assert_eq!(count(&conn), 1);
    }

    #[test]
    fn reopens_db_even_when_sync_fails() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("areitu.db");
        let conn = Mutex::new(areitu_core::db::open(&live).unwrap());
        let r: Result<(), String> = with_db_closed(&conn, &live, || Err("offline".into()));
        assert_eq!(r.unwrap_err(), "offline");
        assert_eq!(count(&conn), 0);
    }

    /// `sync_lock` を握ったまま `drive_sync_locked` を別スレッドから呼ぶと、
    /// ロックが解放されるまでその呼び出しは完了しない（＝写真/カレンダー同期の
    /// 書き込み中に Drive 側の DB ファイル入れ替えが割り込めない）ことを示す。
    /// bounded timeout を使い、失敗時にテストがハングしないようにする。
    #[test]
    fn drive_sync_blocks_until_sync_lock_is_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("areitu.db");
        let state_path = dir.path().join("google-sync-state.json");
        let conn = Mutex::new(areitu_core::db::open(&live).unwrap());
        let sync_lock = Mutex::new(());

        // 他の同期（写真/カレンダー同期を模す）が sync_lock を握っている状態を作る。
        let held = sync_lock.lock().unwrap();

        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _ = drive_sync_locked(&sync_lock, &conn, &live, &state_path);
                tx.send(()).unwrap();
            });

            // ロックが握られている間は完了しないはず。
            assert!(
                rx.recv_timeout(Duration::from_millis(300)).is_err(),
                "drive_sync_locked completed while sync_lock was held by another sync"
            );

            drop(held);

            // 解放後は（build_auth が env 変数なしで即座に失敗するので）速やかに完了する。
            rx.recv_timeout(Duration::from_secs(5)).expect("drive_sync_locked never completed after sync_lock was released");
        });
    }

    #[test]
    fn calendar_scope_is_drive_only_when_calendar_import_is_disabled() {
        let config = crate::config::AppConfig { calendar_enabled: false, ..crate::config::AppConfig::default() };
        assert_eq!(calendar_scope_for(&config), areitu_google::SCOPE_DRIVE_APPDATA);
    }

    #[test]
    fn calendar_scope_adds_calendar_readonly_when_enabled() {
        let config = crate::config::AppConfig { calendar_enabled: true, ..crate::config::AppConfig::default() };
        assert_eq!(
            calendar_scope_for(&config),
            format!("{} {}", areitu_google::SCOPE_DRIVE_APPDATA, areitu_google::SCOPE_CALENDAR_READONLY)
        );
    }

    #[test]
    fn ingest_calendar_is_a_silent_noop_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let c = areitu_core::db::open_in_memory().unwrap();
        let config = crate::config::AppConfig { calendar_enabled: false, ..crate::config::AppConfig::default() };
        let summary = ingest_calendar(&c, &config, &dir.path().join("google-calendar-state.json"));
        assert_eq!(summary, CalendarIngestSummary::default());
    }

    #[test]
    fn ingest_calendar_enabled_without_client_credentials_reports_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let c = areitu_core::db::open_in_memory().unwrap();
        let config = crate::config::AppConfig { calendar_enabled: true, ..crate::config::AppConfig::default() };
        // AREITU_GOOGLE_CLIENT_ID / AREITU_GOOGLE_CLIENT_SECRET はビルド時の
        // option_env! で埋め込まれるため、このテスト環境で未設定なら build_auth() が
        // 即座に失敗する（drive_sync_blocks_until_sync_lock_is_released と同じ前提）。
        let summary = ingest_calendar(&c, &config, &dir.path().join("google-calendar-state.json"));
        assert!(!summary.errors.is_empty());
        assert_eq!(summary.events_synced, 0);
    }
}

#[cfg(test)]
mod ingest_calendar_page_bodies_tests {
    use super::*;
    use areitu_google::calendar::{CalendarApi, EventsListParams, EventsPage};
    use areitu_google::state::CalendarSyncState;
    use std::sync::Mutex;

    struct FakeCalendarApi {
        pages: Mutex<Vec<areitu_google::Result<EventsPage>>>,
        seen_sync_tokens: Mutex<Vec<Option<String>>>,
    }

    impl FakeCalendarApi {
        fn new(pages: Vec<areitu_google::Result<EventsPage>>) -> Self {
            FakeCalendarApi { pages: Mutex::new(pages), seen_sync_tokens: Mutex::new(Vec::new()) }
        }
    }

    impl CalendarApi for FakeCalendarApi {
        fn list_events_page(&self, _access_token: &str, params: &EventsListParams) -> areitu_google::Result<EventsPage> {
            self.seen_sync_tokens.lock().unwrap().push(params.sync_token.map(str::to_owned));
            let mut pages = self.pages.lock().unwrap();
            assert!(!pages.is_empty(), "FakeCalendarApi called more times than pages were queued");
            pages.remove(0)
        }
    }

    fn events_json(id: &str, status: &str) -> String {
        format!(
            r#"{{"items":[{{"id":"{id}","status":"{status}","summary":"ランチ","start":{{"dateTime":"2026-09-01T12:00:00+09:00"}},"end":{{"dateTime":"2026-09-01T13:00:00+09:00"}}}}]}}"#
        )
    }

    fn calendar_row_count(c: &rusqlite::Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'calendar'", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn upserts_confirmed_events_and_advances_sync_token() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let api = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "confirmed"),
            next_page_token: None,
            next_sync_token: Some("token-1".to_owned()),
        })]);
        let mut state = CalendarSyncState::default();
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 1);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(state.sync_token.as_deref(), Some("token-1"));
        assert_eq!(calendar_row_count(&c), 1);
    }

    #[test]
    fn cancelled_event_removes_unassigned_raw_log() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let mut state = CalendarSyncState::default();
        let api1 = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "confirmed"),
            next_page_token: None,
            next_sync_token: Some("token-1".to_owned()),
        })]);
        ingest_calendar_page_bodies(&c, &api1, "access-token", "primary", &mut state);
        assert_eq!(calendar_row_count(&c), 1);

        let api2 = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "cancelled"),
            next_page_token: None,
            next_sync_token: Some("token-2".to_owned()),
        })]);
        let summary = ingest_calendar_page_bodies(&c, &api2, "access-token", "primary", &mut state);
        assert_eq!(summary.events_removed, 1);
        assert_eq!(calendar_row_count(&c), 0);
    }

    #[test]
    fn sync_token_expired_clears_token_and_retries_full_sync_once() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let mut state = CalendarSyncState { sync_token: Some("stale-token".to_owned()) };
        // The retry page deliberately carries no next_sync_token. If it did, the final
        // `state.sync_token = fetched.next_sync_token` overwrite at the end of
        // `ingest_calendar_page_bodies` would mask whether the stale token was actually
        // cleared before the retry — it would overwrite state.sync_token either way,
        // regardless of what happened in between. With no fresh token to fall back on,
        // the only way `state.sync_token` ends up `None` is if the 410 branch itself
        // cleared it before issuing the retry, which is the behavior this test exists
        // to prove (and which a `seen_sync_tokens` check alone cannot: the retry call
        // passes a literal `None`, not `state.sync_token.as_deref()`, so it stays
        // `None` even if the clearing line is mutated away).
        let api = FakeCalendarApi::new(vec![
            Err(areitu_google::Error::SyncTokenExpired),
            Ok(EventsPage {
                body: events_json("e1", "confirmed"),
                next_page_token: None,
                next_sync_token: None,
            }),
        ]);
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 1);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(
            state.sync_token, None,
            "the stale sync_token must be cleared before the full-sync retry, and must stay \
             cleared (not resurface) when the retry itself doesn't hand back a fresh token"
        );
        let seen = api.seen_sync_tokens.lock().unwrap();
        assert_eq!(seen.len(), 2, "expected one failed attempt with the stale token and one full-sync retry");
        assert_eq!(seen[0].as_deref(), Some("stale-token"));
        assert_eq!(seen[1], None, "the retry after a 410 must not resend the stale sync token");
    }

    #[test]
    fn pagination_across_two_pages_syncs_both_events() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let api = FakeCalendarApi::new(vec![
            Ok(EventsPage { body: events_json("e1", "confirmed"), next_page_token: Some("p2".to_owned()), next_sync_token: None }),
            Ok(EventsPage { body: events_json("e2", "confirmed"), next_page_token: None, next_sync_token: Some("final-token".to_owned()) }),
        ]);
        let mut state = CalendarSyncState::default();
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 2);
        assert_eq!(state.sync_token.as_deref(), Some("final-token"));
    }
}
