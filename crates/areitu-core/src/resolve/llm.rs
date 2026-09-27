use serde::Deserialize;
use std::time::Duration;

use crate::cluster::VisitCandidate;
use crate::resolve::geocode::PoiGuess;
use crate::{Error, Result};

pub trait LlmClient {
    fn complete_json(&self, prompt: &str) -> Result<String>;
}

pub fn build_prompt(cand: &VisitCandidate, poi: Option<&PoiGuess>) -> String {
    let mut s = String::from(
        "あなたは訪問履歴から、訪れた店舗・施設の名前を特定するアシスタントです。\n",
    );
    s += &format!(
        "日時: {} 〜 {}\n",
        cand.started_at.format("%Y-%m-%d %H:%M"),
        cand.ended_at.format("%Y-%m-%d %H:%M")
    );
    s += &format!("座標: {:.6}, {:.6}\n", cand.lat, cand.lon);
    if let Some(p) = poi {
        s += &format!("逆ジオコーディング結果: {}\n", p.display_name);
        if let Some(n) = &p.name {
            s += &format!("候補施設名: {n}\n");
        }
    }
    if !cand.hints.is_empty() {
        s += "同じ時間帯のカレンダー予定:\n";
        for h in &cand.hints {
            s += &format!("- {h}\n");
        }
    }
    s += "最も可能性の高い店舗・施設名を1つ選び、次の形式のJSONだけで答えてください: \
          {\"name\": \"店舗名\", \"confidence\": 0.0から1.0の数値}\n";
    s
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct LlmAnswer {
    pub name: String,
    #[serde(default)]
    pub confidence: f64,
}

pub fn parse_answer(raw: &str) -> Option<LlmAnswer> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    let mut a: LlmAnswer = serde_json::from_str(&raw[start..=end]).ok()?;
    a.name = a.name.trim().to_owned();
    if a.name.is_empty() {
        return None;
    }
    a.confidence = a.confidence.clamp(0.0, 1.0);
    Some(a)
}

pub struct Ollama {
    client: reqwest::blocking::Client,
    base_url: String,
    model: String,
}

impl Ollama {
    pub fn new(base_url: &str, model: &str) -> Result<Ollama> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Ollama {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
        })
    }
}

impl LlmClient for Ollama {
    fn complete_json(&self, prompt: &str) -> Result<String> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": "json",
        });
        let resp = self
            .client
            .post(format!("{}/api/generate", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("ollama status {}", resp.status())));
        }
        let v: serde_json::Value = resp.json().map_err(|e| Error::Http(e.to_string()))?;
        v.get("response")
            .and_then(|r| r.as_str())
            .map(str::to_owned)
            .ok_or_else(|| Error::Invalid("ollama: missing response field".into()))
    }
}

pub fn openai_request_body(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "response_format": {"type": "json_object"},
    })
}

pub fn parse_openai_response(json: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Response {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(Deserialize)]
    struct Message {
        content: String,
    }
    let resp: Response = serde_json::from_str(json)?;
    resp.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| Error::Invalid("openai: no choices in response".into()))
}

pub struct OpenAi {
    client: reqwest::blocking::Client,
    api_key: String,
    model: String,
}

impl OpenAi {
    pub fn new(api_key: &str, model: &str) -> Result<OpenAi> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(OpenAi { client, api_key: api_key.to_owned(), model: model.to_owned() })
    }
}

impl LlmClient for OpenAi {
    fn complete_json(&self, prompt: &str) -> Result<String> {
        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&openai_request_body(&self.model, prompt))
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("openai status {}", resp.status())));
        }
        parse_openai_response(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::candidate;

    #[test]
    fn openai_request_body_has_json_response_format() {
        let body = openai_request_body("gpt-4o-mini", "こんにちは");
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "こんにちは");
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn parses_openai_chat_completion_response() {
        let json = r#"{"choices":[{"message":{"content":"{\"name\":\"カフェ丸の内\",\"confidence\":0.9}"}}]}"#;
        let content = parse_openai_response(json).unwrap();
        let answer = parse_answer(&content).unwrap();
        assert_eq!(answer.name, "カフェ丸の内");
        assert_eq!(answer.confidence, 0.9);
    }

    #[test]
    fn openai_response_without_choices_is_error() {
        assert!(parse_openai_response(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn openai_response_broken_json_is_error() {
        assert!(parse_openai_response("not json").is_err());
    }

    #[test]
    fn prompt_contains_time_coords_poi_and_hints() {
        let c = candidate(&["ランチ", "丸の内ビルディング 5F"]);
        let poi = PoiGuess {
            name: Some("丸の内ビルディング".into()),
            display_name: "丸の内ビルディング, 千代田区".into(),
            category: None,
        };
        let p = build_prompt(&c, Some(&poi));
        assert!(p.contains("2026-09-01 12:00"));
        assert!(p.contains("35.681200"));
        assert!(p.contains("丸の内ビルディング, 千代田区"));
        assert!(p.contains("- 丸の内ビルディング 5F"));
        assert!(p.contains("\"confidence\""));
    }

    #[test]
    fn parses_plain_json_answer() {
        let a = parse_answer(r#"{"name": " カフェ丸の内 ", "confidence": 0.8}"#).unwrap();
        assert_eq!(a.name, "カフェ丸の内");
        assert_eq!(a.confidence, 0.8);
    }

    #[test]
    fn parses_json_wrapped_in_text() {
        let a = parse_answer("答え: {\"name\":\"A\",\"confidence\":1.5} 以上").unwrap();
        assert_eq!(a.confidence, 1.0);
    }

    #[test]
    fn rejects_empty_name_and_garbage() {
        assert_eq!(parse_answer(r#"{"name":"  ","confidence":0.9}"#), None);
        assert_eq!(parse_answer("わかりません"), None);
    }
}
