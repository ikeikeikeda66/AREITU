use areitu_core::query::{list_places, visits_of, SortBy};
use areitu_core::store::rename_place;
use rusqlite::Connection;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceDto {
    pub id: i64,
    pub name: String,
    pub visit_count: i64,
    pub last_visit: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisitDto {
    pub started_at: String,
    pub ended_at: String,
}

const DATETIME_FMT: &str = "%Y-%m-%dT%H:%M:%S";

fn parse_sort(sort: &str) -> Result<SortBy, String> {
    match sort {
        "count" => Ok(SortBy::Count),
        "recent" => Ok(SortBy::Recent),
        other => Err(format!("unknown sort: {other}")),
    }
}

pub fn list_places_dto(
    conn: &Connection,
    sort: &str,
    keyword: Option<&str>,
) -> Result<Vec<PlaceDto>, String> {
    let sort_by = parse_sort(sort)?;
    list_places(conn, sort_by, keyword)
        .map(|places| {
            places
                .into_iter()
                .map(|p| PlaceDto {
                    id: p.id,
                    name: p.name,
                    visit_count: p.visit_count,
                    last_visit: p.last_visit.format(DATETIME_FMT).to_string(),
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

pub fn visits_of_dto(conn: &Connection, place_id: i64) -> Result<Vec<VisitDto>, String> {
    visits_of(conn, place_id)
        .map(|visits| {
            visits
                .into_iter()
                .map(|(started_at, ended_at)| VisitDto {
                    started_at: started_at.format(DATETIME_FMT).to_string(),
                    ended_at: ended_at.format(DATETIME_FMT).to_string(),
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

pub fn rename_place_dto(conn: &mut Connection, place_id: i64, name: &str) -> Result<i64, String> {
    rename_place(conn, place_id, name).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use areitu_core::db::open_in_memory;
    use areitu_core::store::{find_or_create_place, insert_visit};
    use areitu_core::testutil_ext::candidate;

    fn seed_place(conn: &Connection, name: &str, at: &str) -> i64 {
        let id = find_or_create_place(conn, name, 35.0, 139.0).unwrap();
        let mut cand = candidate(&[]);
        let t = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap();
        cand.started_at = t;
        cand.ended_at = t;
        insert_visit(conn, id, &cand, "nominatim").unwrap();
        id
    }

    #[test]
    fn list_places_dto_maps_fields() {
        let c = open_in_memory().unwrap();
        seed_place(&c, "カフェ丸の内", "2026-09-01 12:00");
        let places = list_places_dto(&c, "count", None).unwrap();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].name, "カフェ丸の内");
        assert_eq!(places[0].visit_count, 1);
        assert_eq!(places[0].last_visit, "2026-09-01T12:00:00");
    }

    #[test]
    fn list_places_dto_rejects_unknown_sort() {
        let c = open_in_memory().unwrap();
        assert!(list_places_dto(&c, "bogus", None).is_err());
    }

    #[test]
    fn visits_of_dto_is_newest_first() {
        let c = open_in_memory().unwrap();
        let id = seed_place(&c, "A", "2026-01-01 12:00");
        let mut cand = candidate(&[]);
        let t = chrono::NaiveDateTime::parse_from_str("2026-09-01 12:00", "%Y-%m-%d %H:%M").unwrap();
        cand.started_at = t;
        cand.ended_at = t;
        insert_visit(&c, id, &cand, "nominatim").unwrap();
        let visits = visits_of_dto(&c, id).unwrap();
        assert_eq!(visits[0].started_at, "2026-09-01T12:00:00");
    }

    #[test]
    fn rename_place_dto_rejects_blank_name() {
        let mut c = open_in_memory().unwrap();
        let id = seed_place(&c, "A", "2026-09-01 12:00");
        assert!(rename_place_dto(&mut c, id, "   ").is_err());
    }

    #[test]
    fn place_dto_serializes_as_camel_case() {
        let dto = PlaceDto {
            id: 1,
            name: "A".to_string(),
            visit_count: 3,
            last_visit: "2026-09-01T12:00:00".to_string(),
        };
        let json = serde_json::to_value(&dto).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("visitCount"), "missing visitCount: {json}");
        assert!(obj.contains_key("lastVisit"), "missing lastVisit: {json}");
        assert!(!obj.contains_key("visit_count"), "snake_case leaked: {json}");
        assert!(!obj.contains_key("last_visit"), "snake_case leaked: {json}");
    }

    #[test]
    fn visit_dto_serializes_as_camel_case() {
        let dto = VisitDto {
            started_at: "2026-09-01T12:00:00".to_string(),
            ended_at: "2026-09-01T12:30:00".to_string(),
        };
        let json = serde_json::to_value(&dto).unwrap();
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("startedAt"), "missing startedAt: {json}");
        assert!(obj.contains_key("endedAt"), "missing endedAt: {json}");
        assert!(!obj.contains_key("started_at"), "snake_case leaked: {json}");
        assert!(!obj.contains_key("ended_at"), "snake_case leaked: {json}");
    }
}
