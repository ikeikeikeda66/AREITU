use crate::decision::{decide, Decision};
use crate::drive::{DriveApi, DriveFile};
use crate::snapshot::{sha256_hex, vacuum_into};
use crate::state::{load_state, save_state, SyncState};
use crate::swap::swap_db_files;
use std::path::Path;

pub const DB_FILE_NAME: &str = "areitu.db";

#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    NoOp,
    Uploaded,
    Downloaded,
    Conflict { conflict_backup_name: String },
}

pub struct SyncContext<'a> {
    pub drive: &'a dyn DriveApi,
    pub access_token: &'a str,
    pub db_path: &'a Path,
    pub state_path: &'a Path,
}

/// リモートに向けたダウンロード＆スワップの直前に呼ぶ。ジャーナルファイル
/// (`<db_path>-journal`) は「現在の（古い）DB」に属するもので、その下で
/// DB をスワップすると新しい DB を壊してしまうため、それが存在する間は
/// ダウンロードもスワップも行わずエラーを返す。呼び出し元は state を保存
/// していない状態でこのエラーを伝播させるため、ローカル DB・リモート・
/// 同期状態のいずれも変更されないまま次回のリトライに委ねられる。
fn ensure_no_journal(db_path: &Path) -> crate::Result<()> {
    let mut journal_path = db_path.as_os_str().to_owned();
    journal_path.push("-journal");
    if Path::new(&journal_path).exists() {
        return Err(crate::Error::Invalid("database is busy (journal present); retry sync later".into()));
    }
    Ok(())
}

pub fn sync_now(ctx: &SyncContext) -> crate::Result<SyncOutcome> {
    // vacuum_into はこの後すぐ db_path にコネクションを開く。SQLite はコネクション
    // オープン時にロールバックジャーナルを見つけると自動でクラッシュリカバリを行い、
    // ジャーナルファイルを消してしまう。そのため判定はコネクションを開く前に行う必要がある。
    ensure_no_journal(ctx.db_path)?;
    let mut state = load_state(ctx.state_path)?;
    let dir = ctx
        .db_path
        .parent()
        .ok_or_else(|| crate::Error::Invalid("db_path has no parent directory".into()))?;
    let snapshot_path = dir.join(format!("{DB_FILE_NAME}.snapshot"));
    vacuum_into(ctx.db_path, &snapshot_path)?;
    let local_hash = sha256_hex(&snapshot_path)?;
    let remote = ctx.drive.find_db_file(ctx.access_token, DB_FILE_NAME)?;

    let outcome = if state.remote_file_id.is_none() {
        bootstrap(ctx, &mut state, remote, &snapshot_path, &local_hash)?
    } else {
        let remote_file = remote.ok_or_else(|| {
            crate::Error::Invalid("remote areitu.db disappeared from appDataFolder since the last sync".into())
        })?;
        steady_state(ctx, &mut state, remote_file, &snapshot_path, &local_hash)?
    };

    save_state(ctx.state_path, &state)?;
    let _ = std::fs::remove_file(&snapshot_path);
    Ok(outcome)
}

fn bootstrap(
    ctx: &SyncContext,
    state: &mut SyncState,
    remote: Option<DriveFile>,
    snapshot_path: &Path,
    local_hash: &str,
) -> crate::Result<SyncOutcome> {
    match remote {
        None => {
            let content = std::fs::read(snapshot_path)?;
            let uploaded = ctx.drive.upload_create(ctx.access_token, DB_FILE_NAME, &content)?;
            state.remote_file_id = Some(uploaded.id);
            state.remote_modified_time = uploaded.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Uploaded)
        }
        Some(remote_file) => {
            let bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let downloaded_path = snapshot_path.with_extension("downloaded");
            std::fs::write(&downloaded_path, &bytes)?;
            swap_db_files(ctx.db_path, &downloaded_path)?;
            state.remote_file_id = Some(remote_file.id);
            state.remote_modified_time = remote_file.modified_time;
            state.local_content_hash = Some(sha256_hex(ctx.db_path)?);
            Ok(SyncOutcome::Downloaded)
        }
    }
}

fn steady_state(
    ctx: &SyncContext,
    state: &mut SyncState,
    remote_file: DriveFile,
    snapshot_path: &Path,
    local_hash: &str,
) -> crate::Result<SyncOutcome> {
    let remote_changed = remote_file.modified_time != state.remote_modified_time;
    let local_changed = Some(local_hash.to_owned()) != state.local_content_hash;

    match decide(remote_changed, local_changed) {
        Decision::NoOp => Ok(SyncOutcome::NoOp),
        Decision::Upload => {
            let content = std::fs::read(snapshot_path)?;
            let updated = ctx.drive.upload_update(ctx.access_token, &remote_file.id, &content)?;
            state.remote_file_id = Some(updated.id);
            state.remote_modified_time = updated.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Uploaded)
        }
        Decision::Download => {
            let bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let downloaded_path = snapshot_path.with_extension("downloaded");
            std::fs::write(&downloaded_path, &bytes)?;
            swap_db_files(ctx.db_path, &downloaded_path)?;
            state.remote_file_id = Some(remote_file.id);
            state.remote_modified_time = remote_file.modified_time;
            state.local_content_hash = Some(sha256_hex(ctx.db_path)?);
            Ok(SyncOutcome::Downloaded)
        }
        Decision::UploadWithConflictBackup => {
            let remote_bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let conflict_name = format!("areitu-conflict-{}.db", chrono::Local::now().format("%Y%m%dT%H%M%S"));
            ctx.drive.upload_create(ctx.access_token, &conflict_name, &remote_bytes)?;
            let content = std::fs::read(snapshot_path)?;
            let updated = ctx.drive.upload_update(ctx.access_token, &remote_file.id, &content)?;
            state.remote_file_id = Some(updated.id);
            state.remote_modified_time = updated.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Conflict { conflict_backup_name: conflict_name })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeDrive {
        files: Mutex<Vec<DriveFile>>,
        contents: Mutex<std::collections::HashMap<String, Vec<u8>>>,
        next_id: Mutex<u32>,
    }

    impl FakeDrive {
        fn empty() -> Self {
            FakeDrive { files: Mutex::new(vec![]), contents: Mutex::new(std::collections::HashMap::new()), next_id: Mutex::new(1) }
        }

        fn seeded(name: &str, modified_time: &str, content: &[u8]) -> Self {
            let d = Self::empty();
            let id = "seed-1".to_owned();
            d.files.lock().unwrap().push(DriveFile {
                id: id.clone(),
                name: name.to_owned(),
                modified_time: Some(modified_time.to_owned()),
                md5_checksum: None,
            });
            d.contents.lock().unwrap().insert(id, content.to_vec());
            d
        }

        fn next_file_id(&self) -> String {
            let mut n = self.next_id.lock().unwrap();
            *n += 1;
            format!("file-{n}")
        }
    }

    impl DriveApi for FakeDrive {
        fn find_db_file(&self, _access_token: &str, name: &str) -> crate::Result<Option<DriveFile>> {
            Ok(self.files.lock().unwrap().iter().find(|f| f.name == name).cloned())
        }

        fn upload_create(&self, _access_token: &str, name: &str, content: &[u8]) -> crate::Result<DriveFile> {
            let id = self.next_file_id();
            let file = DriveFile { id: id.clone(), name: name.to_owned(), modified_time: Some(format!("mtime-{id}")), md5_checksum: None };
            self.files.lock().unwrap().push(file.clone());
            self.contents.lock().unwrap().insert(id, content.to_vec());
            Ok(file)
        }

        fn upload_update(&self, _access_token: &str, file_id: &str, content: &[u8]) -> crate::Result<DriveFile> {
            let mut files = self.files.lock().unwrap();
            let file = files.iter_mut().find(|f| f.id == file_id).expect("file must exist");
            file.modified_time = Some(format!("mtime-updated-{file_id}"));
            let updated = file.clone();
            self.contents.lock().unwrap().insert(file_id.to_owned(), content.to_vec());
            Ok(updated)
        }

        fn download(&self, _access_token: &str, file_id: &str) -> crate::Result<Vec<u8>> {
            Ok(self.contents.lock().unwrap().get(file_id).cloned().expect("file content must exist"))
        }
    }

    fn make_local_db(dir: &Path, marker: &str) -> std::path::PathBuf {
        let path = dir.join("areitu.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS marker (v TEXT); DELETE FROM marker; INSERT INTO marker (v) VALUES ('{marker}');"
        ))
        .unwrap();
        path
    }

    #[test]
    fn bootstrap_with_no_remote_file_uploads_and_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "first-run");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Uploaded);
        assert_eq!(drive.files.lock().unwrap().len(), 1);
        let state = load_state(&state_path).unwrap();
        assert!(state.remote_file_id.is_some());
    }

    #[test]
    fn bootstrap_with_existing_remote_file_downloads_and_adopts_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "stale-local");
        let state_path = dir.path().join("sync-state.json");

        let remote_dir = tempfile::tempdir().unwrap();
        let remote_db_path = make_local_db(remote_dir.path(), "remote-content");
        let remote_bytes = std::fs::read(&remote_db_path).unwrap();
        let drive = FakeDrive::seeded(DB_FILE_NAME, "2026-09-27T00:00:00Z", &remote_bytes);

        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Downloaded);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "remote-content");
    }

    #[test]
    fn steady_state_neither_changed_is_noop_and_uploads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "unchanged");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        sync_now(&ctx).unwrap(); // 1回目: bootstrap upload
        let outcome = sync_now(&ctx).unwrap(); // 2回目: 何も変わっていない
        assert_eq!(outcome, SyncOutcome::NoOp);
        assert_eq!(drive.files.lock().unwrap().len(), 1, "no extra upload should have happened");
    }

    #[test]
    fn steady_state_local_change_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "v1");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        sync_now(&ctx).unwrap();
        make_local_db(dir.path(), "v2-changed-locally");
        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Uploaded);
    }

    #[test]
    fn steady_state_remote_change_downloads_and_replaces_local() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "local-unchanged");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        sync_now(&ctx).unwrap();

        // 別デバイスがリモートを更新したことを模す。
        let file_id = drive.files.lock().unwrap()[0].id.clone();
        let other_dir = tempfile::tempdir().unwrap();
        let other_db = make_local_db(other_dir.path(), "updated-elsewhere");
        let other_bytes = std::fs::read(&other_db).unwrap();
        drive.upload_update("token", &file_id, &other_bytes).unwrap();

        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Downloaded);
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "updated-elsewhere");
    }

    #[test]
    fn steady_state_remote_change_with_journal_present_is_an_error_and_leaves_everything_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "local-unchanged");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        sync_now(&ctx).unwrap();

        // 別デバイスがリモートを更新したことを模す。
        let file_id = drive.files.lock().unwrap()[0].id.clone();
        let other_dir = tempfile::tempdir().unwrap();
        let other_db = make_local_db(other_dir.path(), "updated-elsewhere");
        let other_bytes = std::fs::read(&other_db).unwrap();
        drive.upload_update("token", &file_id, &other_bytes).unwrap();

        // 現在の（古い）DB に属するロールバックジャーナルが残っている状態を模す。
        let mut journal_path = db_path.as_os_str().to_owned();
        journal_path.push("-journal");
        std::fs::write(&journal_path, b"sqlite rollback journal for the current db").unwrap();

        let db_bytes_before = std::fs::read(&db_path).unwrap();
        let state_bytes_before = std::fs::read(&state_path).unwrap();

        let err = sync_now(&ctx).unwrap_err();
        assert!(matches!(err, crate::Error::Invalid(_)));

        assert_eq!(std::fs::read(&db_path).unwrap(), db_bytes_before, "local db must be untouched while the journal is present");
        assert_eq!(std::fs::read(&state_path).unwrap(), state_bytes_before, "sync state must be untouched so the next cycle retries");
    }

    #[test]
    fn steady_state_both_changed_backs_up_remote_and_uploads_local() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "v1");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        sync_now(&ctx).unwrap();

        // 両方が変わる: リモートは別デバイスから、ローカルはこのマシンから。
        let file_id = drive.files.lock().unwrap()[0].id.clone();
        let other_dir = tempfile::tempdir().unwrap();
        let other_db = make_local_db(other_dir.path(), "updated-elsewhere");
        let other_bytes = std::fs::read(&other_db).unwrap();
        drive.upload_update("token", &file_id, &other_bytes).unwrap();
        make_local_db(dir.path(), "updated-here-too");

        let outcome = sync_now(&ctx).unwrap();
        match outcome {
            SyncOutcome::Conflict { conflict_backup_name } => {
                assert!(conflict_backup_name.starts_with("areitu-conflict-"));
                assert!(conflict_backup_name.ends_with(".db"));
                let backed_up = drive.files.lock().unwrap().iter().any(|f| f.name == conflict_backup_name);
                assert!(backed_up, "the previous remote content must be preserved under the conflict name");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        // ローカルの内容がアップロードされて残っていること。
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "updated-here-too");
    }
}
