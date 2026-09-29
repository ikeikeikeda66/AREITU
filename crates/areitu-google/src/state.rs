use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyncState {
    pub remote_file_id: Option<String>,
    pub remote_modified_time: Option<String>,
    pub local_content_hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalendarSyncState {
    pub sync_token: Option<String>,
    /// 直近のカレンダー同期のエラーメッセージ。成功したら None。
    /// このフィールド追加前に保存された状態ファイルも読めるよう `default` を付ける。
    #[serde(default)]
    pub last_error: Option<String>,
}

fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> crate::Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// 書き込みは同一ディレクトリの一時ファイルに行い、`rename` で置き換える。
/// クラッシュで半端な内容のファイルが残らないようにするため。
fn write_json_atomically<T: serde::Serialize>(path: &Path, value: &T) -> crate::Result<()> {
    let raw = serde_json::to_string_pretty(value)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_path = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("state"),
        std::process::id()
    ));
    // 念のため既存の同名一時ファイルを避ける（実運用では PID で十分だがテストの再実行を考慮）。
    let mut suffix = 0u32;
    while tmp_path.exists() {
        suffix += 1;
        tmp_path = dir.join(format!(
            ".{}.tmp-{}-{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("state"),
            std::process::id(),
            suffix
        ));
    }
    std::fs::write(&tmp_path, raw)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn load_state(path: &Path) -> crate::Result<SyncState> {
    read_json_or_default(path)
}

pub fn save_state(path: &Path, state: &SyncState) -> crate::Result<()> {
    write_json_atomically(path, state)
}

pub fn load_calendar_state(path: &Path) -> crate::Result<CalendarSyncState> {
    read_json_or_default(path)
}

pub fn save_calendar_state(path: &Path, state: &CalendarSyncState) -> crate::Result<()> {
    write_json_atomically(path, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_state_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        assert_eq!(load_state(&path).unwrap(), SyncState::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        let state = SyncState {
            remote_file_id: Some("file-1".to_owned()),
            remote_modified_time: Some("2026-09-27T00:00:00Z".to_owned()),
            local_content_hash: Some("abc123".to_owned()),
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path).unwrap(), state);
    }

    #[test]
    fn corrupt_state_file_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_state(&path).is_err());
    }

    #[test]
    fn calendar_state_missing_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        assert_eq!(load_calendar_state(&path).unwrap(), CalendarSyncState::default());
    }

    #[test]
    fn calendar_state_save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        let state = CalendarSyncState { sync_token: Some("token-1".to_owned()), last_error: Some("boom".to_owned()) };
        save_calendar_state(&path, &state).unwrap();
        assert_eq!(load_calendar_state(&path).unwrap(), state);
    }

    #[test]
    fn calendar_state_corrupt_file_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_calendar_state(&path).is_err());
    }

    #[test]
    fn calendar_state_file_written_before_last_error_existed_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        std::fs::write(&path, r#"{"sync_token":"old-token"}"#).unwrap();
        let state = load_calendar_state(&path).unwrap();
        assert_eq!(state.sync_token.as_deref(), Some("old-token"));
        assert_eq!(state.last_error, None);
    }
}
