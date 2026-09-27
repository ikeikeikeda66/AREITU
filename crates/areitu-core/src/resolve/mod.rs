pub mod dictionary;
pub mod geocode;
pub mod llm;

use rusqlite::Connection;

use crate::cluster::VisitCandidate;
use crate::Result;
use geocode::ReverseGeocoder;
use llm::{build_prompt, parse_answer, LlmClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Dictionary,
    Nominatim,
    Llm,
    Fallback,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Dictionary => "dictionary",
            Method::Nominatim => "nominatim",
            Method::Llm => "llm",
            Method::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub name: String,
    pub method: Method,
}

pub struct Resolver<'a> {
    pub geocoder: &'a dyn ReverseGeocoder,
    pub llm: Option<&'a dyn LlmClient>,
    pub min_confidence: f64,
}

impl Resolver<'_> {
    pub fn resolve(&self, conn: &Connection, cand: &VisitCandidate) -> Result<Resolution> {
        if let Some(name) = dictionary::lookup(conn, cand.lat, cand.lon)? {
            return Ok(Resolution { name, method: Method::Dictionary });
        }
        let poi = self.geocoder.reverse(cand.lat, cand.lon)?;
        let poi_name = poi.as_ref().and_then(|p| p.name.clone());
        let agrees = |n: &str| cand.hints.iter().any(|h| h.contains(n) || n.contains(h.as_str()));
        let needs_llm = match &poi_name {
            None => true,
            Some(n) => !cand.hints.is_empty() && !agrees(n),
        };
        if needs_llm
            && let Some(llm) = self.llm
            && let Ok(raw) = llm.complete_json(&build_prompt(cand, poi.as_ref()))
            && let Some(a) = parse_answer(&raw)
            && a.confidence >= self.min_confidence
        {
            return Ok(Resolution { name: a.name, method: Method::Llm });
        }
        if let Some(name) = poi_name {
            return Ok(Resolution { name, method: Method::Nominatim });
        }
        let name = poi
            .map(|p| p.display_name.split(',').next().unwrap_or("").trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("不明な場所 ({:.5}, {:.5})", cand.lat, cand.lon));
        Ok(Resolution { name, method: Method::Fallback })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::testutil::{candidate, FailingGeocoder, FakeGeocoder, FakeLlm, PanicGeocoder};
    use geocode::PoiGuess;

    fn poi(name: Option<&str>) -> Option<PoiGuess> {
        Some(PoiGuess {
            name: name.map(str::to_owned),
            display_name: "丸の内ビルディング, 丸の内, 千代田区".into(),
            category: None,
        })
    }

    fn run(geo: &dyn geocode::ReverseGeocoder, llm: Option<&FakeLlm>, hints: &[&str]) -> Resolution {
        let c = open_in_memory().unwrap();
        let llm = llm.map(|l| l as &dyn llm::LlmClient);
        Resolver { geocoder: geo, llm, min_confidence: 0.6 }
            .resolve(&c, &candidate(hints))
            .unwrap()
    }

    #[test]
    fn dictionary_wins_without_calling_geocoder() {
        let c = open_in_memory().unwrap();
        dictionary::record_correction(&c, 35.6812, 139.7671, "x", "カフェ丸の内").unwrap();
        let r = Resolver { geocoder: &PanicGeocoder, llm: None, min_confidence: 0.6 }
            .resolve(&c, &candidate(&[]))
            .unwrap();
        assert_eq!(r, Resolution { name: "カフェ丸の内".into(), method: Method::Dictionary });
    }

    #[test]
    fn named_poi_without_hints_uses_nominatim() {
        let llm = FakeLlm(Ok(r#"{"name":"別の店","confidence":0.9}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("東京駅"))), Some(&llm), &[]);
        assert_eq!(r, Resolution { name: "東京駅".into(), method: Method::Nominatim });
    }

    #[test]
    fn hint_agreeing_with_poi_skips_llm() {
        let llm = FakeLlm(Ok(r#"{"name":"別の店","confidence":0.9}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["丸の内ビルディング 5F"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn conflicting_hint_uses_confident_llm() {
        let llm = FakeLlm(Ok(r#"{"name":"カフェ丸の内","confidence":0.8}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["カフェ丸の内でランチ"]);
        assert_eq!(r, Resolution { name: "カフェ丸の内".into(), method: Method::Llm });
    }

    #[test]
    fn low_confidence_llm_falls_back_to_nominatim() {
        let llm = FakeLlm(Ok(r#"{"name":"たぶんここ","confidence":0.3}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["ランチ"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn llm_failure_falls_back_to_nominatim() {
        let llm = FakeLlm(Err("connection refused".into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["ランチ"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn nameless_poi_without_llm_uses_first_address_part() {
        let r = run(&FakeGeocoder(poi(None)), None, &[]);
        assert_eq!(r, Resolution { name: "丸の内ビルディング".into(), method: Method::Fallback });
    }

    #[test]
    fn nothing_found_gives_unknown_place() {
        let r = run(&FakeGeocoder(None), None, &[]);
        assert_eq!(r.name, "不明な場所 (35.68120, 139.76710)");
        assert_eq!(r.method, Method::Fallback);
    }

    #[test]
    fn geocoder_error_is_returned() {
        let c = open_in_memory().unwrap();
        let r = Resolver { geocoder: &FailingGeocoder, llm: None, min_confidence: 0.6 }
            .resolve(&c, &candidate(&[]));
        assert!(r.is_err());
    }
}
