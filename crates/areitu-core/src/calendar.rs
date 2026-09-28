use chrono::{DateTime, NaiveDateTime};
use rusqlite::Connection;
use serde::Deserialize;
use std::path::Path;

use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::Result;

#[derive(Deserialize)]
struct EventsResponse {
    #[serde(default)]
    items: Vec<Event>,
}

#[derive(Deserialize)]
struct Event {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    location: Option<String>,
    start: When,
    end: When,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct When {
    #[serde(default)]
    date_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub location: Option<String>,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

fn wall_clock(s: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.naive_local())
}

pub fn parse_events(json: &str) -> Result<Vec<CalendarEvent>> {
    let resp: EventsResponse = serde_json::from_str(json)?;
    Ok(resp
        .items
        .into_iter()
        .filter(|e| e.status.as_deref() != Some("cancelled"))
        .filter_map(|e| {
            let start = wall_clock(e.start.date_time.as_deref()?)?;
            let end = wall_clock(e.end.date_time.as_deref()?)?;
            let title = e
                .summary
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "(無題)".to_owned());
            let location = e.location.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            Some(CalendarEvent { id: e.id, title, location, start, end })
        })
        .collect())
}

pub fn to_raw_log(e: &CalendarEvent) -> RawLog {
    let text = match &e.location {
        Some(loc) => format!("{}\n{}", e.title, loc),
        None => e.title.clone(),
    };
    RawLog {
        source: Source::Calendar,
        source_id: e.id.clone(),
        occurred_at: e.start,
        ended_at: Some(e.end),
        lat: None,
        lon: None,
        text: Some(text),
    }
}

pub fn parse_cancelled_source_ids(json: &str) -> Result<Vec<String>> {
    let resp: EventsResponse = serde_json::from_str(json)?;
    Ok(resp
        .items
        .into_iter()
        .filter(|e| e.status.as_deref() == Some("cancelled"))
        .map(|e| e.id)
        .collect())
}

pub fn ingest_calendar_file(conn: &Connection, path: &Path) -> Result<usize> {
    let events = parse_events(&std::fs::read_to_string(path)?)?;
    for e in &events {
        upsert_raw_log(conn, &to_raw_log(e))?;
    }
    Ok(events.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
      "kind": "calendar#events",
      "items": [
        {"id": "a1", "status": "confirmed", "summary": "ランチ",
         "location": "丸の内ビルディング 5F",
         "start": {"dateTime": "2026-09-01T12:00:00+09:00"},
         "end": {"dateTime": "2026-09-01T13:00:00+09:00"}},
        {"id": "a2", "status": "confirmed", "summary": "休暇",
         "start": {"date": "2026-09-02"}, "end": {"date": "2026-09-03"}},
        {"id": "a3", "status": "cancelled", "summary": "中止",
         "start": {"dateTime": "2026-09-04T10:00:00+09:00"},
         "end": {"dateTime": "2026-09-04T11:00:00+09:00"}},
        {"id": "a4", "status": "confirmed",
         "start": {"dateTime": "2026-09-05T03:00:00Z"},
         "end": {"dateTime": "2026-09-05T04:00:00Z"}}
      ]
    }"#;

    #[test]
    fn keeps_timed_confirmed_events_only() {
        let ids: Vec<String> = parse_events(JSON).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["a1", "a4"]);
    }

    #[test]
    fn keeps_wall_clock_time_of_offset() {
        let e = &parse_events(JSON).unwrap()[0];
        assert_eq!(e.start.to_string(), "2026-09-01 12:00:00");
        assert_eq!(e.end.to_string(), "2026-09-01 13:00:00");
    }

    #[test]
    fn untitled_event_gets_placeholder() {
        assert_eq!(parse_events(JSON).unwrap()[1].title, "(無題)");
    }

    #[test]
    fn raw_log_text_contains_title_and_location() {
        let log = to_raw_log(&parse_events(JSON).unwrap()[0]);
        assert_eq!(log.source, Source::Calendar);
        assert_eq!(log.text.as_deref(), Some("ランチ\n丸の内ビルディング 5F"));
        assert_eq!(log.lat, None);
    }

    #[test]
    fn empty_response_is_ok() {
        assert!(parse_events(r#"{"kind":"calendar#events"}"#).unwrap().is_empty());
    }

    #[test]
    fn broken_json_is_error() {
        assert!(parse_events("{").is_err());
    }

    #[test]
    fn cancelled_events_are_extracted_by_id() {
        assert_eq!(parse_cancelled_source_ids(JSON).unwrap(), vec!["a3"]);
    }

    #[test]
    fn no_cancelled_events_is_an_empty_list() {
        assert!(parse_cancelled_source_ids(r#"{"items":[{"id":"a1","status":"confirmed","start":{"dateTime":"2026-09-01T12:00:00+09:00"},"end":{"dateTime":"2026-09-01T13:00:00+09:00"}}]}"#).unwrap().is_empty());
    }
}
