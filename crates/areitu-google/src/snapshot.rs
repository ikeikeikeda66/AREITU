use std::path::Path;

pub fn vacuum_into(source_db: &Path, dest_path: &Path) -> crate::Result<()> {
    if dest_path.exists() {
        std::fs::remove_file(dest_path)?;
    }
    let conn = rusqlite::Connection::open(source_db)?;
    conn.execute("VACUUM INTO ?1", rusqlite::params![dest_path.to_string_lossy()])?;
    Ok(())
}

pub fn sha256_hex(path: &Path) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(digest.as_slice().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t (v) VALUES ('hello');").unwrap();
    }

    #[test]
    fn snapshot_contains_same_rows_as_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("areitu.db");
        let dest = dir.path().join("areitu.snapshot");
        make_db(&source);
        vacuum_into(&source, &dest).unwrap();
        let conn = rusqlite::Connection::open(&dest).unwrap();
        let v: String = conn.query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hello");
    }

    #[test]
    fn snapshot_overwrites_a_leftover_file_from_a_crashed_previous_run() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("areitu.db");
        let dest = dir.path().join("areitu.snapshot");
        make_db(&source);
        std::fs::write(&dest, b"stale leftover bytes from a crashed sync").unwrap();
        vacuum_into(&source, &dest).unwrap();
        let conn = rusqlite::Connection::open(&dest).unwrap();
        let v: String = conn.query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hello");
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        let hash = sha256_hex(&path).unwrap();
        assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha256_hex_differs_for_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"content-a").unwrap();
        std::fs::write(&b, b"content-b").unwrap();
        assert_ne!(sha256_hex(&a).unwrap(), sha256_hex(&b).unwrap());
    }
}
