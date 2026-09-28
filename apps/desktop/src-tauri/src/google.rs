use areitu_google::auth::{client_credentials_from_env, AuthStatus, GoogleAuth, SystemBrowser};
use areitu_google::drive::DriveClient;
use areitu_google::keychain::KeyringStore;
use areitu_google::oauth::TokenClient;
use areitu_google::sync::{sync_now, DbSwapGuard, SyncContext, SyncOutcome};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

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

#[tauri::command]
pub fn google_sign_in() -> Result<(), String> {
    build_auth()?
        .sign_in(areitu_google::SCOPE_DRIVE_APPDATA)
        .map_err(|e| e.to_string())
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
}
