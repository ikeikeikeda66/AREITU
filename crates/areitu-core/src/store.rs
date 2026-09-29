use rusqlite::{params, Connection, OptionalExtension};

use crate::cluster::VisitCandidate;
use crate::geo::haversine_m;
use crate::model::{RawLog, Source};
use crate::resolve::dictionary::record_correction;
use crate::{Error, Result};

pub const PLACE_MERGE_M: f64 = 200.0;

fn find_place(conn: &Connection, name: &str, lat: f64, lon: f64, exclude: Option<i64>) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id, lat, lon FROM places WHERE name = ?1 ORDER BY id")?;
    let rows = stmt.query_map([name], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)))?;
    for row in rows {
        let (id, la, lo) = row?;
        if Some(id) != exclude && haversine_m((lat, lon), (la, lo)) <= PLACE_MERGE_M {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub fn find_or_create_place(conn: &Connection, name: &str, lat: f64, lon: f64) -> Result<i64> {
    if let Some(id) = find_place(conn, name, lat, lon, None)? {
        return Ok(id);
    }
    conn.execute("INSERT INTO places (name, lat, lon) VALUES (?1, ?2, ?3)", params![name, lat, lon])?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_visit(conn: &Connection, place_id: i64, cand: &VisitCandidate, method: &str) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO visits (place_id, started_at, ended_at, method) VALUES (?1, ?2, ?3, ?4)",
        params![place_id, cand.started_at, cand.ended_at, method],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM visits WHERE place_id = ?1 AND started_at = ?2",
        params![place_id, cand.started_at],
        |r| r.get(0),
    )?)
}

pub fn assign_logs(conn: &Connection, visit_id: i64, log_ids: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare("UPDATE raw_logs SET visit_id = ?1 WHERE id = ?2")?;
    for id in log_ids {
        stmt.execute(params![visit_id, id])?;
    }
    Ok(())
}

pub fn rename_place(conn: &mut Connection, place_id: i64, new_name: &str) -> Result<i64> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(Error::Invalid("place name must not be empty".into()));
    }
    let tx = conn.transaction()?;
    let (old, lat, lon): (String, f64, f64) = tx
        .query_row("SELECT name, lat, lon FROM places WHERE id = ?1", [place_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or_else(|| Error::Invalid(format!("place {place_id} not found")))?;
    record_correction(&tx, lat, lon, &old, new_name)?;
    let result = match find_place(&tx, new_name, lat, lon, Some(place_id))? {
        Some(target) => {
            tx.execute("UPDATE OR IGNORE visits SET place_id = ?1 WHERE place_id = ?2", params![target, place_id])?;
            tx.execute("DELETE FROM places WHERE id = ?1", [place_id])?;
            target
        }
        None => {
            tx.execute("UPDATE places SET name = ?1 WHERE id = ?2", params![new_name, place_id])?;
            place_id
        }
    };
    tx.commit()?;
    Ok(result)
}

pub fn upsert_raw_log(conn: &Connection, log: &RawLog) -> Result<()> {
    conn.execute(
        "INSERT INTO raw_logs (source, source_id, occurred_at, ended_at, lat, lon, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(source, source_id) DO UPDATE SET
           occurred_at = excluded.occurred_at,
           ended_at = excluded.ended_at,
           lat = excluded.lat,
           lon = excluded.lon,
           text = excluded.text
         WHERE raw_logs.visit_id IS NULL",
        params![
            log.source.as_str(),
            log.source_id,
            log.occurred_at,
            log.ended_at,
            log.lat,
            log.lon,
            log.text
        ],
    )?;
    Ok(())
}

/// 対象の raw_log がまだ visit に割り当てられていない場合だけ削除する。
/// 既に visit に組み込まれている raw_log は、後からカレンダー側でキャンセルされても
/// 過去に確定した訪問履歴を壊さないよう、削除しない。
pub fn delete_unassigned_raw_log(conn: &Connection, source: Source, source_id: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM raw_logs WHERE source = ?1 AND source_id = ?2 AND visit_id IS NULL",
        params![source.as_str(), source_id],
    )?;
    Ok(changed > 0)
}

pub fn unassigned_raw_logs(conn: &Connection) -> Result<Vec<(i64, RawLog)>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, occurred_at, ended_at, lat, lon, text
         FROM raw_logs WHERE visit_id IS NULL ORDER BY occurred_at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        let source: String = r.get(1)?;
        Ok((
            r.get::<_, i64>(0)?,
            source,
            RawLog {
                source: Source::Photo,
                source_id: r.get(2)?,
                occurred_at: r.get(3)?,
                ended_at: r.get(4)?,
                lat: r.get(5)?,
                lon: r.get(6)?,
                text: r.get(7)?,
            },
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, source, mut log) = row?;
        log.source = Source::parse(&source)
            .ok_or_else(|| Error::Invalid(format!("unknown source: {source}")))?;
        out.push((id, log));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{RawLog, Source};
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn event(id: &str, at: &str, text: &str) -> RawLog {
        RawLog {
            source: Source::Calendar,
            source_id: id.into(),
            occurred_at: t(at),
            ended_at: Some(t(at)),
            lat: None,
            lon: None,
            text: Some(text.into()),
        }
    }

    #[test]
    fn upsert_twice_keeps_one_row() {
        let c = open_in_memory().unwrap();
        let log = event("e1", "2026-09-01 12:00", "ランチ");
        upsert_raw_log(&c, &log).unwrap();
        upsert_raw_log(&c, &log).unwrap();
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_changed_fields() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 13:00", "ランチ（変更）")).unwrap();
        let logs = unassigned_raw_logs(&c).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1.occurred_at, t("2026-09-01 13:00"));
        assert_eq!(logs[0].1.text.as_deref(), Some("ランチ（変更）"));
    }

    #[test]
    fn unassigned_is_sorted_by_time() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("b", "2026-09-02 10:00", "B")).unwrap();
        upsert_raw_log(&c, &event("a", "2026-09-01 10:00", "A")).unwrap();
        let ids: Vec<String> = unassigned_raw_logs(&c)
            .unwrap()
            .into_iter()
            .map(|(_, l)| l.source_id)
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn deletes_an_unassigned_raw_log() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        let deleted = delete_unassigned_raw_log(&c, Source::Calendar, "e1").unwrap();
        assert!(deleted);
        assert!(unassigned_raw_logs(&c).unwrap().is_empty());
    }

    #[test]
    fn leaves_an_already_assigned_raw_log_untouched() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        let log_id = unassigned_raw_logs(&c).unwrap()[0].0;
        let place_id = find_or_create_place(&c, "カフェ丸の内", 35.0, 139.0).unwrap();
        c.execute(
            "INSERT INTO visits (place_id, started_at, ended_at, method) VALUES (?1, '2026-09-01 12:00', '2026-09-01 12:30', 'nominatim')",
            params![place_id],
        )
        .unwrap();
        let visit_id: i64 = c
            .query_row("SELECT id FROM visits WHERE place_id = ?1", params![place_id], |r| r.get(0))
            .unwrap();
        c.execute("UPDATE raw_logs SET visit_id = ?1 WHERE id = ?2", params![visit_id, log_id]).unwrap();

        let deleted = delete_unassigned_raw_log(&c, Source::Calendar, "e1").unwrap();
        assert!(!deleted, "an already-assigned raw log must not be deleted");
        let count: i64 = c.query_row("SELECT COUNT(*) FROM raw_logs", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn deleting_a_nonexistent_raw_log_is_not_an_error() {
        let c = open_in_memory().unwrap();
        assert!(!delete_unassigned_raw_log(&c, Source::Calendar, "missing").unwrap());
    }
}
