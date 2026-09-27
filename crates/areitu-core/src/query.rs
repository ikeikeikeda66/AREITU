use chrono::NaiveDateTime;
use rusqlite::{params, Connection};

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Count,
    Recent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceSummary {
    pub id: i64,
    pub name: String,
    pub visit_count: i64,
    pub last_visit: NaiveDateTime,
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub fn list_places(conn: &Connection, sort: SortBy, keyword: Option<&str>) -> Result<Vec<PlaceSummary>> {
    let order = match sort {
        SortBy::Count => "visit_count DESC, last_visit DESC",
        SortBy::Recent => "last_visit DESC, visit_count DESC",
    };
    let sql = format!(
        "SELECT p.id, p.name, COUNT(v.id) AS visit_count, MAX(v.started_at) AS last_visit
         FROM places p JOIN visits v ON v.place_id = p.id
         WHERE ?1 IS NULL OR p.name LIKE ?1 ESCAPE '\\'
         GROUP BY p.id
         ORDER BY {order}, p.id"
    );
    let pattern = keyword
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| format!("%{}%", escape_like(k)));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![pattern], |r| {
        Ok(PlaceSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            visit_count: r.get(2)?,
            last_visit: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn visits_of(conn: &Connection, place_id: i64) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>> {
    let mut stmt = conn.prepare(
        "SELECT started_at, ended_at FROM visits WHERE place_id = ?1 ORDER BY started_at DESC",
    )?;
    let rows = stmt.query_map([place_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::store::{find_or_create_place, insert_visit, rename_place};
    use crate::testutil::candidate;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn seed(c: &Connection, name: &str, lat: f64, starts: &[&str]) -> i64 {
        let id = find_or_create_place(c, name, lat, 139.0).unwrap();
        for s in starts {
            let mut cand = candidate(&[]);
            cand.started_at = at(s);
            cand.ended_at = at(s);
            insert_visit(c, id, &cand, "nominatim").unwrap();
        }
        id
    }

    fn names(v: &[PlaceSummary]) -> Vec<&str> {
        v.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn sorts_by_count_and_by_recent() {
        let c = open_in_memory().unwrap();
        seed(&c, "よく行く店", 35.0, &["2026-01-01 12:00", "2026-02-01 12:00", "2026-03-01 12:00"]);
        seed(&c, "最近行った店", 35.1, &["2026-09-01 12:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, None).unwrap()), vec!["よく行く店", "最近行った店"]);
        let recent = list_places(&c, SortBy::Recent, None).unwrap();
        assert_eq!(names(&recent), vec!["最近行った店", "よく行く店"]);
        assert_eq!(recent[1].visit_count, 3);
        assert_eq!(recent[1].last_visit, at("2026-03-01 12:00"));
    }

    #[test]
    fn search_matches_substring() {
        let c = open_in_memory().unwrap();
        seed(&c, "ブルーボトルコーヒー", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "スターバックス", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, Some("ボトル")).unwrap()), vec!["ブルーボトルコーヒー"]);
    }

    #[test]
    fn search_treats_percent_and_underscore_literally() {
        let c = open_in_memory().unwrap();
        seed(&c, "100%果汁スタンド", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "普通の店", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, Some("%")).unwrap()), vec!["100%果汁スタンド"]);
        assert!(list_places(&c, SortBy::Count, Some("_")).unwrap().is_empty());
    }

    #[test]
    fn blank_search_returns_all() {
        let c = open_in_memory().unwrap();
        seed(&c, "A", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "B", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(list_places(&c, SortBy::Count, Some("   ")).unwrap().len(), 2);
    }

    #[test]
    fn visits_of_is_newest_first() {
        let c = open_in_memory().unwrap();
        let id = seed(&c, "A", 35.0, &["2026-01-01 12:00", "2026-09-01 12:00"]);
        let v = visits_of(&c, id).unwrap();
        assert_eq!(v[0].0, at("2026-09-01 12:00"));
    }

    #[test]
    fn rename_updates_name_and_records_dictionary() {
        let mut c = open_in_memory().unwrap();
        let id = seed(&c, "丸の内ビルディング", 35.0, &["2026-09-01 12:00"]);
        assert_eq!(rename_place(&mut c, id, " カフェ丸の内 ").unwrap(), id);
        assert_eq!(names(&list_places(&c, SortBy::Count, None).unwrap()), vec!["カフェ丸の内"]);
        assert_eq!(
            crate::resolve::dictionary::lookup(&c, 35.0, 139.0).unwrap().as_deref(),
            Some("カフェ丸の内")
        );
    }

    #[test]
    fn rename_to_existing_nearby_name_merges_places() {
        let mut c = open_in_memory().unwrap();
        let keep = seed(&c, "カフェ丸の内", 35.0, &["2026-01-01 12:00"]);
        let dup = seed(&c, "丸の内ビルディング", 35.0005, &["2026-02-01 12:00"]);
        assert_eq!(rename_place(&mut c, dup, "カフェ丸の内").unwrap(), keep);
        let all = list_places(&c, SortBy::Count, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].visit_count, 2);
    }

    #[test]
    fn rename_to_blank_is_rejected() {
        let mut c = open_in_memory().unwrap();
        let id = seed(&c, "A", 35.0, &["2026-09-01 12:00"]);
        assert!(rename_place(&mut c, id, "  ").is_err());
    }

    #[test]
    fn rename_unknown_place_is_error() {
        let mut c = open_in_memory().unwrap();
        assert!(rename_place(&mut c, 999, "X").is_err());
    }
}
