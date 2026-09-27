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
}
