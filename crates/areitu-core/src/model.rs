use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Photo,
    Calendar,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Photo => "photo",
            Source::Calendar => "calendar",
        }
    }

    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "photo" => Some(Source::Photo),
            "calendar" => Some(Source::Calendar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawLog {
    pub source: Source,
    pub source_id: String,
    pub occurred_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub text: Option<String>,
}
