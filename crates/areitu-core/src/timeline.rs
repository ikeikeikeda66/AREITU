use chrono::{DateTime, NaiveDateTime};
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineVisit {
    pub lat: f64,
    pub lon: f64,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub name: Option<String>,
}

/// パース結果。`skipped` は visit として存在するが必須項目が読めなかった要素の数。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTimeline {
    pub visits: Vec<TimelineVisit>,
    pub skipped: usize,
}

fn wall_clock(s: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.naive_local())
}

/// `"35.681236°, 139.767125°"` と `"geo:35.681236,139.767125"` のどちらも読む。
fn parse_lat_lng(s: &str) -> Option<(f64, f64)> {
    let s = s.trim().strip_prefix("geo:").unwrap_or(s.trim());
    let mut parts = s.split(',');
    let lat: f64 = parts.next()?.trim().trim_end_matches('°').parse().ok()?;
    let lon: f64 = parts.next()?.trim().trim_end_matches('°').parse().ok()?;
    Some((lat, lon))
}

fn clean_name(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn on_device_visit(seg: &Value) -> Option<TimelineVisit> {
    let candidate = seg.get("visit")?.get("topCandidate")?;
    let (lat, lon) = parse_lat_lng(candidate.get("placeLocation")?.get("latLng")?.as_str()?)?;
    let start = wall_clock(seg.get("startTime")?.as_str()?)?;
    let end = wall_clock(seg.get("endTime")?.as_str()?)?;
    Some(TimelineVisit { lat, lon, start, end, name: clean_name(candidate.get("name")) })
}

fn takeout_visit(pv: &Value) -> Option<TimelineVisit> {
    let loc = pv.get("location")?;
    let lat = loc.get("latitudeE7")?.as_i64()? as f64 / 1e7;
    let lon = loc.get("longitudeE7")?.as_i64()? as f64 / 1e7;
    let duration = pv.get("duration")?;
    let start = wall_clock(duration.get("startTimestamp")?.as_str()?)?;
    let end = wall_clock(duration.get("endTimestamp")?.as_str()?)?;
    Some(TimelineVisit { lat, lon, start, end, name: clean_name(loc.get("name")) })
}

/// `key` 配下の各要素から、`visit_key` を持つものだけを対象に `read` で読む。
/// `visit_key` を持たない要素は visit ではないので数えず、持つのに読めない要素は skipped に数える。
fn collect(
    entries: &Value,
    what: &str,
    visit_key: &str,
    read: fn(&Value) -> Option<TimelineVisit>,
) -> Result<ParsedTimeline> {
    let entries = entries.as_array().ok_or_else(|| Error::Invalid(format!("timeline export: {what} is not an array")))?;
    let mut visits = Vec::new();
    let mut skipped = 0;
    for entry in entries {
        let Some(candidate) = entry.get(visit_key).filter(|v| !v.is_null()) else { continue };
        // Takeout は placeVisit の中身、オンデバイスは segment 全体を読む。
        let parsed = if visit_key == "placeVisit" { read(candidate) } else { read(entry) };
        match parsed {
            Some(v) => visits.push(v),
            None => skipped += 1,
        }
    }
    Ok(ParsedTimeline { visits, skipped })
}

pub fn parse_timeline_export_counted(json: &str) -> Result<ParsedTimeline> {
    let value: Value = serde_json::from_str(json)?;
    let obj = value.as_object().ok_or_else(|| Error::Invalid("timeline export is not a JSON object".into()))?;
    if let Some(segments) = obj.get("semanticSegments") {
        collect(segments, "semanticSegments", "visit", on_device_visit)
    } else if let Some(objects) = obj.get("timelineObjects") {
        collect(objects, "timelineObjects", "placeVisit", takeout_visit)
    } else {
        Err(Error::Invalid("unrecognized timeline export: expected semanticSegments or timelineObjects".into()))
    }
}

pub fn parse_timeline_export(json: &str) -> Result<Vec<TimelineVisit>> {
    Ok(parse_timeline_export_counted(json)?.visits)
}

pub fn to_raw_log(v: &TimelineVisit) -> RawLog {
    let source_id = format!("{:.6},{:.6}@{}", v.lat, v.lon, v.start.format("%Y%m%dT%H%M%S"));
    RawLog {
        source: Source::Timeline,
        source_id,
        occurred_at: v.start,
        ended_at: Some(v.end),
        lat: Some(v.lat),
        lon: Some(v.lon),
        text: v.name.clone(),
    }
}

pub fn ingest_timeline_file(conn: &Connection, path: &Path) -> Result<usize> {
    let visits = parse_timeline_export(&std::fs::read_to_string(path)?)?;
    for v in &visits {
        upsert_raw_log(conn, &to_raw_log(v))?;
    }
    Ok(visits.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON_DEVICE_JSON: &str = r#"{
      "semanticSegments": [
        {
          "startTime": "2026-09-01T12:00:00.000+09:00",
          "endTime": "2026-09-01T13:00:00.000+09:00",
          "visit": {
            "topCandidate": {
              "placeLocation": {"latLng": "35.681236°, 139.767125°"},
              "name": "カフェ丸の内"
            }
          }
        },
        {
          "startTime": "2026-09-02T09:00:00.000+09:00",
          "endTime": "2026-09-02T09:30:00.000+09:00",
          "visit": {
            "topCandidate": {
              "placeLocation": {"latLng": "geo:35.714800,139.774500"}
            }
          }
        },
        {
          "startTime": "2026-09-02T10:00:00.000+09:00",
          "endTime": "2026-09-02T10:40:00.000+09:00",
          "activity": {"start": {"latLng": "35.7°, 139.7°"}}
        }
      ]
    }"#;

    const TAKEOUT_JSON: &str = r#"{
      "timelineObjects": [
        {
          "placeVisit": {
            "location": {"latitudeE7": 356812360, "longitudeE7": 1397671250, "name": "カフェ丸の内"},
            "duration": {"startTimestamp": "2026-09-01T03:00:00.000Z", "endTimestamp": "2026-09-01T04:00:00.000Z"}
          }
        },
        {
          "placeVisit": {
            "location": {"latitudeE7": 357148000, "longitudeE7": 1397745000},
            "duration": {"startTimestamp": "2026-09-02T00:00:00.000Z", "endTimestamp": "2026-09-02T00:30:00.000Z"}
          }
        },
        {
          "activitySegment": {"distance": 1200}
        }
      ]
    }"#;

    #[test]
    fn on_device_export_reads_degree_symbol_lat_lng_and_name() {
        let visits = parse_timeline_export(ON_DEVICE_JSON).unwrap();
        assert_eq!(visits.len(), 2, "the activity segment without a visit must be skipped");
        assert_eq!(visits[0].name.as_deref(), Some("カフェ丸の内"));
        assert!((visits[0].lat - 35.681236).abs() < 1e-6);
        assert!((visits[0].lon - 139.767125).abs() < 1e-6);
    }

    #[test]
    fn on_device_export_reads_geo_uri_lat_lng_and_allows_no_name() {
        let visits = parse_timeline_export(ON_DEVICE_JSON).unwrap();
        assert_eq!(visits[1].name, None, "a visit without a name must still be imported");
        assert!((visits[1].lat - 35.7148).abs() < 1e-6);
        assert!((visits[1].lon - 139.7745).abs() < 1e-6);
    }

    #[test]
    fn takeout_export_reads_e7_coordinates_and_skips_non_place_visit_entries() {
        let visits = parse_timeline_export(TAKEOUT_JSON).unwrap();
        assert_eq!(visits.len(), 2, "the activitySegment entry must be skipped");
        assert_eq!(visits[0].name.as_deref(), Some("カフェ丸の内"));
        assert!((visits[0].lat - 35.681236).abs() < 1e-6);
        assert!((visits[0].lon - 139.767125).abs() < 1e-6);
        assert_eq!(visits[1].name, None);
    }

    #[test]
    fn unrecognized_format_is_an_error() {
        let err = parse_timeline_export(r#"{"somethingElse": []}"#).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_timeline_export("{").is_err());
    }

    #[test]
    fn to_raw_log_carries_source_timeline_and_name_as_text() {
        let v = TimelineVisit {
            lat: 35.6812,
            lon: 139.7671,
            start: wall_clock("2026-09-01T12:00:00+09:00").unwrap(),
            end: wall_clock("2026-09-01T13:00:00+09:00").unwrap(),
            name: Some("カフェ丸の内".to_owned()),
        };
        let log = to_raw_log(&v);
        assert_eq!(log.source, Source::Timeline);
        assert_eq!(log.text.as_deref(), Some("カフェ丸の内"));
        assert_eq!(log.lat, Some(35.6812));
    }

    #[test]
    fn ingest_timeline_file_inserts_raw_logs_and_returns_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Timeline.json");
        std::fs::write(&path, ON_DEVICE_JSON).unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let inserted = ingest_timeline_file(&conn, &path).unwrap();
        assert_eq!(inserted, 2);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'timeline'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn reimporting_the_same_file_does_not_duplicate_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Timeline.json");
        std::fs::write(&path, ON_DEVICE_JSON).unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        ingest_timeline_file(&conn, &path).unwrap();
        ingest_timeline_file(&conn, &path).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'timeline'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn malformed_entries_are_skipped_and_counted_not_fatal() {
        let json = r#"{"semanticSegments": [
          {"startTime": "2026-09-01T12:00:00+09:00", "endTime": "2026-09-01T13:00:00+09:00",
           "visit": {"topCandidate": {"placeLocation": {"latLng": "35.68°, 139.76°"}}}},
          {"startTime": "not a time", "endTime": "2026-09-01T13:00:00+09:00",
           "visit": {"topCandidate": {"placeLocation": {"latLng": "35.68°, 139.76°"}}}},
          {"startTime": "2026-09-01T14:00:00+09:00", "endTime": "2026-09-01T15:00:00+09:00",
           "visit": {"topCandidate": {"placeLocation": {"latLng": "garbage"}}}},
          {"startTime": "2026-09-01T16:00:00+09:00", "endTime": "2026-09-01T17:00:00+09:00",
           "visit": {"topCandidate": 5}},
          42
        ]}"#;
        let parsed = parse_timeline_export_counted(json).unwrap();
        assert_eq!(parsed.visits.len(), 1);
        assert_eq!(parsed.skipped, 3, "the non-visit entry (42) is not a malformed visit");
        assert_eq!(parse_timeline_export(json).unwrap().len(), 1);
    }

    #[test]
    fn takeout_malformed_place_visits_are_skipped_and_counted() {
        let json = r#"{"timelineObjects": [
          {"placeVisit": {"location": {"latitudeE7": 356812360, "longitudeE7": 1397671250},
                          "duration": {"startTimestamp": "2026-09-01T03:00:00Z", "endTimestamp": "2026-09-01T04:00:00Z"}}},
          {"placeVisit": {"location": {"latitudeE7": "x"}, "duration": {}}},
          {"placeVisit": {"location": {"latitudeE7": 1, "longitudeE7": 2},
                          "duration": {"startTimestamp": "bad", "endTimestamp": "bad"}}}
        ]}"#;
        let parsed = parse_timeline_export_counted(json).unwrap();
        assert_eq!(parsed.visits.len(), 1);
        assert_eq!(parsed.skipped, 2);
    }

    #[test]
    fn offset_timestamps_keep_local_wall_clock_time() {
        let visits = parse_timeline_export(ON_DEVICE_JSON).unwrap();
        assert_eq!(visits[0].start.format("%Y-%m-%d %H:%M").to_string(), "2026-09-01 12:00");
        let takeout = parse_timeline_export(TAKEOUT_JSON).unwrap();
        assert_eq!(takeout[0].start.format("%Y-%m-%d %H:%M").to_string(), "2026-09-01 03:00");
    }

    #[test]
    fn non_object_top_level_and_non_array_segments_are_errors() {
        assert!(matches!(parse_timeline_export("[]").unwrap_err(), Error::Invalid(_)));
        assert!(matches!(parse_timeline_export(r#"{"semanticSegments": 3}"#).unwrap_err(), Error::Invalid(_)));
    }

    #[test]
    fn ingest_reports_a_missing_file_as_an_error() {
        let conn = crate::db::open_in_memory().unwrap();
        assert!(ingest_timeline_file(&conn, Path::new("/nonexistent/Timeline.json")).is_err());
    }
}
