pub mod calendar;
pub mod cluster;
pub mod db;
pub mod exif;
pub mod geo;
pub mod model;
pub mod pipeline;
pub mod query;
pub mod resolve;
pub mod scan;
pub mod store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
pub(crate) mod testutil {
    use ::exif::{experimental::Writer, Field, In, Rational, Tag, Value};

    pub fn ascii(tag: Tag, s: &str) -> Field {
        Field { tag, ifd_num: In::PRIMARY, value: Value::Ascii(vec![s.as_bytes().to_vec()]) }
    }

    pub fn dms(tag: Tag, d: u32, m: u32, s_num: u32, s_den: u32) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(vec![
                Rational { num: d, denom: 1 },
                Rational { num: m, denom: 1 },
                Rational { num: s_num, denom: s_den },
            ]),
        }
    }

    pub fn tiff(fields: &[Field]) -> Vec<u8> {
        let mut w = Writer::new();
        for f in fields {
            w.push_field(f);
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        w.write(&mut buf, false).unwrap();
        buf.into_inner()
    }

    pub fn jpeg(tiff: &[u8]) -> Vec<u8> {
        let len = (2 + 6 + tiff.len()) as u16;
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(b"Exif\0\0");
        v.extend_from_slice(tiff);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    use crate::cluster::VisitCandidate;
    use crate::resolve::geocode::{PoiGuess, ReverseGeocoder};
    use crate::resolve::llm::LlmClient;
    use crate::{Error, Result};

    pub fn candidate(hints: &[&str]) -> VisitCandidate {
        let t = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        VisitCandidate {
            started_at: t("2026-09-01 12:00"),
            ended_at: t("2026-09-01 12:45"),
            lat: 35.6812,
            lon: 139.7671,
            log_ids: vec![],
            hints: hints.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub struct FakeGeocoder(pub Option<PoiGuess>);
    impl ReverseGeocoder for FakeGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            Ok(self.0.clone())
        }
    }

    pub struct FailingGeocoder;
    impl ReverseGeocoder for FailingGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            Err(Error::Http("offline".into()))
        }
    }

    pub struct PanicGeocoder;
    impl ReverseGeocoder for PanicGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            panic!("geocoder must not be called")
        }
    }

    pub struct FakeLlm(pub std::result::Result<String, String>);
    impl LlmClient for FakeLlm {
        fn complete_json(&self, _: &str) -> Result<String> {
            self.0.clone().map_err(Error::Http)
        }
    }
}
