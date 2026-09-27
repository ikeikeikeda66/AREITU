use std::path::Path;

pub fn swap_db_files(current_db_path: &Path, downloaded_db_path: &Path) -> crate::Result<()> {
    let backup_path = current_db_path.with_extension("db.bak");
    if current_db_path.exists() {
        std::fs::rename(current_db_path, &backup_path)?;
    }
    match std::fs::rename(downloaded_db_path, current_db_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(e) => {
            if backup_path.exists() {
                let _ = std::fs::rename(&backup_path, current_db_path);
            }
            Err(crate::Error::Io(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_current_db_with_downloaded_one() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let downloaded = dir.path().join("areitu.db.downloaded");
        std::fs::write(&current, b"old content").unwrap();
        std::fs::write(&downloaded, b"new content").unwrap();
        swap_db_files(&current, &downloaded).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"new content");
        assert!(!downloaded.exists());
        assert!(!current.with_extension("db.bak").exists());
    }

    #[test]
    fn works_even_when_current_db_did_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let downloaded = dir.path().join("areitu.db.downloaded");
        std::fs::write(&downloaded, b"first sync content").unwrap();
        swap_db_files(&current, &downloaded).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"first sync content");
    }

    #[test]
    fn restores_backup_if_the_final_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let missing_downloaded = dir.path().join("does-not-exist.db");
        std::fs::write(&current, b"original content").unwrap();
        let err = swap_db_files(&current, &missing_downloaded).unwrap_err();
        assert!(matches!(err, crate::Error::Io(_)));
        assert_eq!(std::fs::read(&current).unwrap(), b"original content");
    }
}
