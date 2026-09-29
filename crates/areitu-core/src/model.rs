use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Photo,
    Calendar,
    Timeline,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Photo => "photo",
            Source::Calendar => "calendar",
            Source::Timeline => "timeline",
        }
    }

    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "photo" => Some(Source::Photo),
            "calendar" => Some(Source::Calendar),
            "timeline" => Some(Source::Timeline),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_source_round_trips_through_as_str_and_parse() {
        assert_eq!(Source::Timeline.as_str(), "timeline");
        assert_eq!(Source::parse("timeline"), Some(Source::Timeline));
    }

    #[test]
    fn unknown_source_string_is_none() {
        assert_eq!(Source::parse("bogus"), None);
    }
}
