use serde::Deserialize;
use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::{Error, Result};

const MIN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq)]
pub struct PoiGuess {
    pub name: Option<String>,
    pub display_name: String,
    pub category: Option<String>,
}

pub trait ReverseGeocoder {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>>;
}

#[derive(Deserialize)]
struct Reverse {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub fn parse_reverse(json: &str) -> Result<Option<PoiGuess>> {
    let r: Reverse = serde_json::from_str(json)?;
    if r.error.is_some() {
        return Ok(None);
    }
    let Some(display_name) = r.display_name else {
        return Ok(None);
    };
    Ok(Some(PoiGuess {
        name: r.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()),
        display_name,
        category: r.category,
    }))
}

pub struct Nominatim {
    client: reqwest::blocking::Client,
    base_url: String,
    last_call: Cell<Option<Instant>>,
}

impl Nominatim {
    pub fn new(user_agent: &str) -> Result<Nominatim> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Nominatim {
            client,
            base_url: "https://nominatim.openstreetmap.org".to_owned(),
            last_call: Cell::new(None),
        })
    }

    pub fn with_base_url(mut self, url: &str) -> Nominatim {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    fn throttle(&self) {
        if let Some(last) = self.last_call.get() {
            let elapsed = last.elapsed();
            if elapsed < MIN_INTERVAL {
                std::thread::sleep(MIN_INTERVAL - elapsed);
            }
        }
        self.last_call.set(Some(Instant::now()));
    }
}

impl ReverseGeocoder for Nominatim {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>> {
        self.throttle();
        let (lat, lon) = (lat.to_string(), lon.to_string());
        let resp = self
            .client
            .get(format!("{}/reverse", self.base_url))
            .query(&[
                ("format", "jsonv2"),
                ("lat", lat.as_str()),
                ("lon", lon.as_str()),
                ("zoom", "18"),
                ("accept-language", "ja"),
            ])
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("nominatim status {}", resp.status())));
        }
        parse_reverse(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}

const GOOGLE_PLACES_RADIUS_M: f64 = 50.0;

pub fn google_places_request_body(lat: f64, lon: f64) -> serde_json::Value {
    serde_json::json!({
        "locationRestriction": {
            "circle": {
                "center": {"latitude": lat, "longitude": lon},
                "radius": GOOGLE_PLACES_RADIUS_M,
            }
        },
        "maxResultCount": 1,
    })
}

pub fn parse_google_places_response(json: &str) -> Result<Option<PoiGuess>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        places: Vec<Place>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Place {
        #[serde(default)]
        display_name: Option<DisplayName>,
        #[serde(default)]
        formatted_address: Option<String>,
        #[serde(default)]
        types: Vec<String>,
    }
    #[derive(Deserialize)]
    struct DisplayName {
        text: String,
    }
    let resp: Response = serde_json::from_str(json)?;
    let Some(place) = resp.places.into_iter().next() else {
        return Ok(None);
    };
    let Some(display_name) = place.formatted_address else {
        return Ok(None);
    };
    Ok(Some(PoiGuess {
        name: place.display_name.map(|d| d.text),
        display_name,
        category: place.types.into_iter().next(),
    }))
}

pub struct GooglePlaces {
    client: reqwest::blocking::Client,
    api_key: String,
}

impl GooglePlaces {
    pub fn new(api_key: &str) -> Result<GooglePlaces> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(GooglePlaces { client, api_key: api_key.to_owned() })
    }
}

impl ReverseGeocoder for GooglePlaces {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>> {
        let resp = self
            .client
            .post("https://places.googleapis.com/v1/places:searchNearby")
            .header("X-Goog-Api-Key", &self.api_key)
            .header(
                "X-Goog-FieldMask",
                "places.displayName,places.formattedAddress,places.types",
            )
            .json(&google_places_request_body(lat, lon))
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("google places status {}", resp.status())));
        }
        parse_google_places_response(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_poi() {
        let g = parse_reverse(
            r#"{"place_id":1,"category":"amenity","type":"cafe","name":"ブルーボトルコーヒー",
                "display_name":"ブルーボトルコーヒー, 丸の内, 千代田区, 東京都, 日本"}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(g.name.as_deref(), Some("ブルーボトルコーヒー"));
        assert_eq!(g.category.as_deref(), Some("amenity"));
    }

    #[test]
    fn empty_name_becomes_none() {
        let g = parse_reverse(r#"{"name":"","display_name":"1-1, 丸の内, 千代田区"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(g.name, None);
        assert_eq!(g.display_name, "1-1, 丸の内, 千代田区");
    }

    #[test]
    fn error_response_is_none() {
        assert_eq!(parse_reverse(r#"{"error":"Unable to geocode"}"#).unwrap(), None);
    }

    #[test]
    fn broken_json_is_error() {
        assert!(parse_reverse("<html>").is_err());
    }

    #[test]
    #[ignore = "hits the real Nominatim API"]
    fn live_tokyo_station() {
        let n = Nominatim::new("AREITU-test/0.1 (+https://github.com/ikeikeikeda66/AREITU)").unwrap();
        let g = n.reverse(35.681236, 139.767125).unwrap().unwrap();
        assert!(g.display_name.contains("千代田区"), "{}", g.display_name);
    }

    #[test]
    fn google_places_request_body_has_circle_restriction() {
        let body = google_places_request_body(35.6812, 139.7671);
        assert_eq!(body["locationRestriction"]["circle"]["center"]["latitude"], 35.6812);
        assert_eq!(body["maxResultCount"], 1);
    }

    #[test]
    fn parses_named_place() {
        let json = r#"{"places":[{"displayName":{"text":"ブルーボトルコーヒー","languageCode":"ja"},
            "formattedAddress":"日本、東京都千代田区丸の内","types":["cafe","food"]}]}"#;
        let g = parse_google_places_response(json).unwrap().unwrap();
        assert_eq!(g.name.as_deref(), Some("ブルーボトルコーヒー"));
        assert_eq!(g.category.as_deref(), Some("cafe"));
    }

    #[test]
    fn empty_places_is_none() {
        assert_eq!(parse_google_places_response(r#"{"places":[]}"#).unwrap(), None);
    }

    #[test]
    fn google_places_broken_json_is_error() {
        assert!(parse_google_places_response("<html>").is_err());
    }
}
