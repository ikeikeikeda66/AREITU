use rusqlite::{params, Connection};

use crate::geo::haversine_m;
use crate::Result;

pub const DICT_RADIUS_M: f64 = 50.0;
const PREFILTER_DEG: f64 = 0.005;

pub fn lookup(conn: &Connection, lat: f64, lon: f64) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT lat, lon, corrected_name FROM user_dictionary
         WHERE lat BETWEEN ?1 AND ?2 AND lon BETWEEN ?3 AND ?4
         ORDER BY id DESC",
    )?;
    let rows = stmt.query_map(
        params![lat - PREFILTER_DEG, lat + PREFILTER_DEG, lon - PREFILTER_DEG, lon + PREFILTER_DEG],
        |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, String>(2)?)),
    )?;
    let mut best: Option<(f64, String)> = None;
    for row in rows {
        let (la, lo, name) = row?;
        let d = haversine_m((lat, lon), (la, lo));
        if d <= DICT_RADIUS_M && best.as_ref().is_none_or(|(b, _)| d < *b) {
            best = Some((d, name));
        }
    }
    Ok(best.map(|(_, n)| n))
}

pub fn record_correction(
    conn: &Connection,
    lat: f64,
    lon: f64,
    guessed: &str,
    corrected: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO user_dictionary (lat, lon, guessed_name, corrected_name) VALUES (?1, ?2, ?3, ?4)",
        params![lat, lon, guessed, corrected],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn empty_dictionary_returns_none() {
        let c = open_in_memory().unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap(), None);
    }

    #[test]
    fn nearby_correction_is_found() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.6812, 139.7671, "丸の内ビルディング", "カフェ丸の内").unwrap();
        assert_eq!(
            lookup(&c, 35.6813, 139.7671).unwrap().as_deref(),
            Some("カフェ丸の内")
        );
    }

    #[test]
    fn far_correction_is_ignored() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.6812, 139.7671, "A", "B").unwrap();
        assert_eq!(lookup(&c, 35.6830, 139.7671).unwrap(), None);
    }

    #[test]
    fn latest_correction_wins_at_same_point() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.0, 139.0, "x", "古い名前").unwrap();
        record_correction(&c, 35.0, 139.0, "x", "新しい名前").unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap().as_deref(), Some("新しい名前"));
    }

    #[test]
    fn nearest_correction_wins() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.00030, 139.0, "x", "遠い方").unwrap();
        record_correction(&c, 35.00005, 139.0, "x", "近い方").unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap().as_deref(), Some("近い方"));
    }
}
