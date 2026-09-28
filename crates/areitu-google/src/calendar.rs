pub trait CalendarApi {
    fn list_events_page(&self, access_token: &str, params: &EventsListParams) -> crate::Result<EventsPage>;
}

pub struct EventsListParams<'a> {
    pub calendar_id: &'a str,
    pub sync_token: Option<&'a str>,
    pub page_token: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventsPage {
    pub body: String,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
}

pub struct CalendarClient {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl CalendarClient {
    pub fn new() -> crate::Result<CalendarClient> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(CalendarClient { client, base_url: "https://www.googleapis.com".to_owned() })
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsListMeta {
    #[serde(default)]
    next_page_token: Option<String>,
    #[serde(default)]
    next_sync_token: Option<String>,
}

impl CalendarApi for CalendarClient {
    fn list_events_page(&self, access_token: &str, params: &EventsListParams) -> crate::Result<EventsPage> {
        let mut req = self
            .client
            .get(format!("{}/calendar/v3/calendars/{}/events", self.base_url, params.calendar_id))
            .bearer_auth(access_token)
            .query(&[("singleEvents", "true")]);
        if let Some(token) = params.sync_token {
            req = req.query(&[("syncToken", token)]);
        }
        if let Some(token) = params.page_token {
            req = req.query(&[("pageToken", token)]);
        }
        let resp = req.send().map_err(|e| crate::Error::Http(e.to_string()))?;
        if resp.status().as_u16() == 410 {
            return Err(crate::Error::SyncTokenExpired);
        }
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("calendar events.list status {}", resp.status())));
        }
        let body = resp.text().map_err(|e| crate::Error::Http(e.to_string()))?;
        let meta: EventsListMeta = serde_json::from_str(&body)?;
        Ok(EventsPage { body, next_page_token: meta.next_page_token, next_sync_token: meta.next_sync_token })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FetchedEvents {
    pub bodies: Vec<String>,
    pub next_sync_token: Option<String>,
}

/// `sync_token` は最初のリクエストにだけ載せる。2ページ目以降は `page_token` だけを送る
/// (Google Calendar API のページングの仕様に合わせる)。
pub fn fetch_all_pages(
    api: &dyn CalendarApi,
    access_token: &str,
    calendar_id: &str,
    sync_token: Option<&str>,
) -> crate::Result<FetchedEvents> {
    let mut bodies = Vec::new();
    let mut page_token: Option<String> = None;
    let mut next_sync_token = None;
    loop {
        let params = EventsListParams {
            calendar_id,
            sync_token: if page_token.is_none() { sync_token } else { None },
            page_token: page_token.as_deref(),
        };
        let page = api.list_events_page(access_token, &params)?;
        bodies.push(page.body);
        if page.next_sync_token.is_some() {
            next_sync_token = page.next_sync_token;
        }
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => break,
        }
    }
    Ok(FetchedEvents { bodies, next_sync_token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn single_page_returns_body_and_next_sync_token() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/calendar/v3/calendars/primary/events")
                .query_param("singleEvents", "true");
            then.status(200).json_body(serde_json::json!({"items": [], "nextSyncToken": "token-1"}));
        });
        let client = CalendarClient::new().unwrap().with_base_url(&server.base_url());
        let page = client
            .list_events_page("access", &EventsListParams { calendar_id: "primary", sync_token: None, page_token: None })
            .unwrap();
        assert_eq!(page.next_sync_token.as_deref(), Some("token-1"));
        assert_eq!(page.next_page_token, None);
        assert!(page.body.contains("nextSyncToken"));
    }

    #[test]
    fn gone_status_maps_to_sync_token_expired_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/calendar/v3/calendars/primary/events");
            then.status(410);
        });
        let client = CalendarClient::new().unwrap().with_base_url(&server.base_url());
        let err = client
            .list_events_page("access", &EventsListParams { calendar_id: "primary", sync_token: Some("stale"), page_token: None })
            .unwrap_err();
        assert!(matches!(err, crate::Error::SyncTokenExpired));
    }

    #[test]
    fn non_success_non_410_status_is_a_generic_http_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/calendar/v3/calendars/primary/events");
            then.status(401);
        });
        let client = CalendarClient::new().unwrap().with_base_url(&server.base_url());
        let err = client
            .list_events_page("access", &EventsListParams { calendar_id: "primary", sync_token: None, page_token: None })
            .unwrap_err();
        assert!(matches!(err, crate::Error::Http(_)));
    }

    #[test]
    fn fetch_all_pages_follows_pagination_and_sends_sync_token_once() {
        let server = MockServer::start();
        let page1 = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/calendar/v3/calendars/primary/events")
                .query_param("syncToken", "prev-token");
            then.status(200).json_body(serde_json::json!({"items": [], "nextPageToken": "p2"}));
        });
        let page2 = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/calendar/v3/calendars/primary/events")
                .query_param("pageToken", "p2");
            then.status(200).json_body(serde_json::json!({"items": [], "nextSyncToken": "final-token"}));
        });
        let client = CalendarClient::new().unwrap().with_base_url(&server.base_url());
        let fetched = fetch_all_pages(&client, "access", "primary", Some("prev-token")).unwrap();
        page1.assert();
        page2.assert();
        assert_eq!(fetched.bodies.len(), 2);
        assert_eq!(fetched.next_sync_token.as_deref(), Some("final-token"));
    }

    #[test]
    fn fetch_all_pages_with_no_sync_token_does_a_full_sync() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/calendar/v3/calendars/primary/events");
            then.status(200).json_body(serde_json::json!({"items": [], "nextSyncToken": "fresh-token"}));
        });
        let client = CalendarClient::new().unwrap().with_base_url(&server.base_url());
        let fetched = fetch_all_pages(&client, "access", "primary", None).unwrap();
        mock.assert();
        assert_eq!(fetched.bodies.len(), 1);
        assert_eq!(fetched.next_sync_token.as_deref(), Some("fresh-token"));
    }
}
