use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyncState {
    pub remote_file_id: Option<String>,
    pub remote_modified_time: Option<String>,
    pub local_content_hash: Option<String>,
}

pub fn load_state(path: &Path) -> crate::Result<SyncState> {
    if !path.exists() {
        return Ok(SyncState::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// 書き込みは同一ディレクトリの一時ファイルに行い、`rename` で置き換える。
/// クラッシュで半端な内容のファイルが残らないようにするため。
pub fn save_state(path: &Path, state: &SyncState) -> crate::Result<()> {
    let raw = serde_json::to_string_pretty(state)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_path = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sync-state"),
        std::process::id()
    ));
    // 念のため既存の同名一時ファイルを避ける（実運用では PID で十分だがテストの再実行を考慮）。
    let mut suffix = 0u32;
    while tmp_path.exists() {
        suffix += 1;
        tmp_path = dir.join(format!(
            ".{}.tmp-{}-{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sync-state"),
            std::process::id(),
            suffix
        ));
    }
    std::fs::write(&tmp_path, raw)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
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
}
