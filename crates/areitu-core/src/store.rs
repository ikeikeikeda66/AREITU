use rusqlite::{params, Connection};

use crate::model::{RawLog, Source};
use crate::{Error, Result};

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
}
