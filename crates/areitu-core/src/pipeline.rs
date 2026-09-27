use rusqlite::Connection;

use crate::cluster::cluster;
use crate::resolve::Resolver;
use crate::store::{assign_logs, find_or_create_place, insert_visit, unassigned_raw_logs};
use crate::Result;

#[derive(Debug, Default, PartialEq)]
pub struct BuildReport {
    pub visits: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

pub fn build_visits(conn: &mut Connection, resolver: &Resolver) -> Result<BuildReport> {
    let logs = unassigned_raw_logs(conn)?;
    let mut report = BuildReport::default();
    for cand in cluster(&logs) {
        let res = match resolver.resolve(conn, &cand) {
            Ok(r) => r,
            Err(e) => {
                report.failed += 1;
                report.errors.push(e.to_string());
                continue;
            }
        };
        let tx = conn.transaction()?;
        let place = find_or_create_place(&tx, &res.name, cand.lat, cand.lon)?;
        let visit = insert_visit(&tx, place, &cand, res.method.as_str())?;
        assign_logs(&tx, visit, &cand.log_ids)?;
        tx.commit()?;
        report.visits += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{RawLog, Source};
    use crate::resolve::geocode::PoiGuess;
    use crate::store::{unassigned_raw_logs, upsert_raw_log};
    use crate::testutil::{FailingGeocoder, FakeGeocoder};
    use chrono::NaiveDateTime;

    fn photo(id: &str, at: &str, lat: f64, lon: f64) -> RawLog {
        RawLog {
            source: Source::Photo,
            source_id: id.into(),
            occurred_at: NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap(),
            ended_at: None,
            lat: Some(lat),
            lon: Some(lon),
            text: None,
        }
    }

    fn geo(name: &str) -> FakeGeocoder {
        FakeGeocoder(Some(PoiGuess { name: Some(name.into()), display_name: name.into(), category: None }))
    }

    fn count(c: &rusqlite::Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn builds_visits_and_assigns_logs() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        upsert_raw_log(&c, &photo("b", "2026-09-08 12:00", 35.6812, 139.7671)).unwrap();
        let g = geo("カフェ丸の内");
        let r = build_visits(&mut c, &Resolver { geocoder: &g, llm: None, min_confidence: 0.6 }).unwrap();
        assert_eq!((r.visits, r.failed), (2, 0));
        assert_eq!(count(&c, "places"), 1);
        assert_eq!(count(&c, "visits"), 2);
        assert!(unassigned_raw_logs(&c).unwrap().is_empty());
    }

    #[test]
    fn rerun_creates_nothing_new() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        let g = geo("カフェ丸の内");
        let res = Resolver { geocoder: &g, llm: None, min_confidence: 0.6 };
        build_visits(&mut c, &res).unwrap();
        let r = build_visits(&mut c, &res).unwrap();
        assert_eq!(r.visits, 0);
        assert_eq!(count(&c, "visits"), 1);
    }

    #[test]
    fn offline_geocoder_reports_failure_and_keeps_logs_for_retry() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        let r = build_visits(&mut c, &Resolver { geocoder: &FailingGeocoder, llm: None, min_confidence: 0.6 }).unwrap();
        assert_eq!((r.visits, r.failed), (0, 1));
        assert_eq!(r.errors, vec!["http: offline"]);
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }
}
