use chrono::{Duration, NaiveDateTime};

use crate::geo::haversine_m;
use crate::model::{RawLog, Source};

pub const RADIUS_M: f64 = 150.0;
pub const MAX_GAP_MINUTES: i64 = 120;
pub const HINT_MARGIN_MINUTES: i64 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct VisitCandidate {
    pub started_at: NaiveDateTime,
    pub ended_at: NaiveDateTime,
    pub lat: f64,
    pub lon: f64,
    pub log_ids: Vec<i64>,
    pub hints: Vec<String>,
}

pub fn cluster(logs: &[(i64, RawLog)]) -> Vec<VisitCandidate> {
    let mut points: Vec<(i64, &RawLog, (f64, f64))> = logs
        .iter()
        .filter_map(|(id, l)| Some((*id, l, (l.lat?, l.lon?))))
        .collect();
    points.sort_by_key(|(id, l, _)| (l.occurred_at, *id));

    let mut out: Vec<VisitCandidate> = Vec::new();
    for (id, l, p) in points {
        let end = l.ended_at.unwrap_or(l.occurred_at);
        if let Some(c) = out.last_mut() {
            let near = haversine_m((c.lat, c.lon), p) <= RADIUS_M;
            let soon = l.occurred_at - c.ended_at <= Duration::minutes(MAX_GAP_MINUTES);
            if near && soon {
                let n = c.log_ids.len() as f64;
                c.lat = (c.lat * n + p.0) / (n + 1.0);
                c.lon = (c.lon * n + p.1) / (n + 1.0);
                c.ended_at = c.ended_at.max(end);
                c.log_ids.push(id);
                continue;
            }
        }
        out.push(VisitCandidate {
            started_at: l.occurred_at,
            ended_at: end,
            lat: p.0,
            lon: p.1,
            log_ids: vec![id],
            hints: Vec::new(),
        });
    }
    attach_calendar(&mut out, logs);
    out
}

fn attach_calendar(cands: &mut [VisitCandidate], logs: &[(i64, RawLog)]) {
    let margin = Duration::minutes(HINT_MARGIN_MINUTES);
    for (id, l) in logs.iter().filter(|(_, l)| l.source == Source::Calendar) {
        let start = l.occurred_at;
        let end = l.ended_at.unwrap_or(start);
        let mut assigned = false;
        for c in cands.iter_mut() {
            if start <= c.ended_at + margin && end >= c.started_at - margin {
                if let Some(text) = &l.text {
                    c.hints.extend(
                        text.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned),
                    );
                }
                if !assigned {
                    c.log_ids.push(*id);
                    assigned = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RawLog, Source};
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn photo(id: i64, at: &str, lat: f64, lon: f64) -> (i64, RawLog) {
        (id, RawLog {
            source: Source::Photo,
            source_id: format!("p{id}"),
            occurred_at: t(at),
            ended_at: None,
            lat: Some(lat),
            lon: Some(lon),
            text: None,
        })
    }

    fn event(id: i64, from: &str, to: &str, text: &str) -> (i64, RawLog) {
        (id, RawLog {
            source: Source::Calendar,
            source_id: format!("e{id}"),
            occurred_at: t(from),
            ended_at: Some(t(to)),
            lat: None,
            lon: None,
            text: Some(text.into()),
        })
    }

    const CAFE: (f64, f64) = (35.6812, 139.7671);
    const PARK: (f64, f64) = (35.7148, 139.7745);

    #[test]
    fn nearby_photos_close_in_time_form_one_visit() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
            photo(2, "2026-09-01 12:40", CAFE.0 + 0.0003, CAFE.1),
        ]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].log_ids, vec![1, 2]);
        assert_eq!(v[0].started_at, t("2026-09-01 12:00"));
        assert_eq!(v[0].ended_at, t("2026-09-01 12:40"));
    }

    #[test]
    fn same_place_after_long_gap_is_two_visits() {
        let v = cluster(&[
            photo(1, "2026-09-01 09:00", CAFE.0, CAFE.1),
            photo(2, "2026-09-01 18:00", CAFE.0, CAFE.1),
        ]);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn two_places_same_day_are_kept_in_order() {
        let v = cluster(&[
            photo(2, "2026-09-01 15:00", PARK.0, PARK.1),
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
        ]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].log_ids, vec![1]);
        assert_eq!(v[1].log_ids, vec![2]);
    }

    #[test]
    fn photos_without_gps_are_ignored() {
        let mut p = photo(1, "2026-09-01 12:00", 0.0, 0.0);
        p.1.lat = None;
        p.1.lon = None;
        assert!(cluster(&[p]).is_empty());
    }

    #[test]
    fn overlapping_event_adds_hints_and_log_id() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:10", CAFE.0, CAFE.1),
            event(9, "2026-09-01 12:00", "2026-09-01 13:00", "ランチ\n丸の内ビルディング 5F\n"),
        ]);
        assert_eq!(v[0].hints, vec!["ランチ", "丸の内ビルディング 5F"]);
        assert_eq!(v[0].log_ids, vec![1, 9]);
    }

    #[test]
    fn event_within_margin_matches_but_far_event_does_not() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
            event(8, "2026-09-01 12:20", "2026-09-01 12:50", "近い"),
            event(9, "2026-09-01 15:00", "2026-09-01 16:00", "遠い"),
        ]);
        assert_eq!(v[0].hints, vec!["近い"]);
    }
}
