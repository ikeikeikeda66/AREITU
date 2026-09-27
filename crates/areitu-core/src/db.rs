use rusqlite::Connection;
use std::path::Path;

use crate::Result;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS places (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS visits (
  id INTEGER PRIMARY KEY,
  place_id INTEGER NOT NULL REFERENCES places(id) ON DELETE CASCADE,
  started_at TEXT NOT NULL,
  ended_at TEXT NOT NULL,
  method TEXT NOT NULL,
  UNIQUE(place_id, started_at)
);
CREATE TABLE IF NOT EXISTS raw_logs (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL,
  source_id TEXT NOT NULL,
  occurred_at TEXT NOT NULL,
  ended_at TEXT,
  lat REAL,
  lon REAL,
  text TEXT,
  visit_id INTEGER REFERENCES visits(id) ON DELETE SET NULL,
  UNIQUE(source, source_id)
);
CREATE TABLE IF NOT EXISTS user_dictionary (
  id INTEGER PRIMARY KEY,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  guessed_name TEXT NOT NULL,
  corrected_name TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_visits_place ON visits(place_id);
CREATE INDEX IF NOT EXISTS idx_raw_logs_time ON raw_logs(occurred_at);
"#;

pub fn open(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    init(&c)?;
    Ok(c)
}

pub fn open_in_memory() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    init(&c)?;
    Ok(c)
}

fn init(c: &Connection) -> Result<()> {
    c.pragma_update(None, "foreign_keys", true)?;
    c.execute_batch(SCHEMA)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(c: &rusqlite::Connection) -> Vec<String> {
        let mut stmt = c
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn creates_all_tables() {
        let c = open_in_memory().unwrap();
        assert_eq!(
            table_names(&c),
            vec!["places", "raw_logs", "user_dictionary", "visits"]
        );
    }

    #[test]
    fn open_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("areitu.db");
        open(&path).unwrap();
        let c = open(&path).unwrap();
        assert_eq!(table_names(&c).len(), 4);
    }

    #[test]
    fn foreign_keys_enabled() {
        let c = open_in_memory().unwrap();
        let on: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(on, 1);
    }
}
