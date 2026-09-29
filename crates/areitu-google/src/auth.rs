use std::time::Duration;

pub trait BrowserOpener {
    fn open(&self, url: &str) -> crate::Result<()>;
}

pub struct SystemBrowser;

impl BrowserOpener for SystemBrowser {
    fn open(&self, url: &str) -> crate::Result<()> {
        webbrowser::open(url).map_err(|e| crate::Error::OAuth(format!("failed to open system browser: {e}")))
    }
}

pub struct GoogleClientCredentials {
    pub client_id: String,
    pub client_secret: String,
}

pub fn client_credentials_from_env() -> crate::Result<GoogleClientCredentials> {
    let client_id = option_env!("AREITU_GOOGLE_CLIENT_ID")
        .ok_or_else(|| crate::Error::Invalid("AREITU_GOOGLE_CLIENT_ID is not set at build time".into()))?;
    let client_secret = option_env!("AREITU_GOOGLE_CLIENT_SECRET")
        .ok_or_else(|| crate::Error::Invalid("AREITU_GOOGLE_CLIENT_SECRET is not set at build time".into()))?;
    Ok(GoogleClientCredentials { client_id: client_id.to_owned(), client_secret: client_secret.to_owned() })
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthStatus {
    SignedOut,
    SignedIn,
}

/// Reads the sign-in state from the token store alone; no client credentials are needed.
pub fn auth_status(store: &impl crate::keychain::TokenStore) -> crate::Result<AuthStatus> {
    Ok(match store.load_refresh_token()? {
        Some(_) => AuthStatus::SignedIn,
        None => AuthStatus::SignedOut,
    })
}

pub struct GoogleAuth<S: crate::keychain::TokenStore, B: BrowserOpener> {
    store: S,
    browser: B,
    token_client: crate::oauth::TokenClient,
    creds: GoogleClientCredentials,
    authorize_base_url: String,
}

impl<S: crate::keychain::TokenStore, B: BrowserOpener> GoogleAuth<S, B> {
    pub fn new(store: S, browser: B, token_client: crate::oauth::TokenClient, creds: GoogleClientCredentials) -> Self {
        GoogleAuth { store, browser, token_client, creds, authorize_base_url: "https://accounts.google.com".to_owned() }
    }

    pub fn with_authorize_base_url(mut self, url: &str) -> Self {
        self.authorize_base_url = url.to_owned();
        self
    }

    pub fn status(&self) -> crate::Result<AuthStatus> {
        auth_status(&self.store)
    }

    pub fn sign_out(&self) -> crate::Result<()> {
        self.store.clear_refresh_token()
    }

    pub fn sign_in(&self, scope: &str) -> crate::Result<()> {
        let (listener, port) = crate::loopback::bind_loopback()?;
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let pkce = crate::oauth::generate_pkce();
        let state = crate::oauth::generate_state();
        let url = crate::oauth::build_authorize_url(
            &self.authorize_base_url,
            &crate::oauth::AuthorizeUrlParams {
                client_id: &self.creds.client_id,
                redirect_uri: &redirect_uri,
                scope,
                state: &state,
                code_challenge: &pkce.challenge,
            },
        )?;
        self.browser.open(&url)?;
        let callback = crate::loopback::await_callback(listener, &state, Duration::from_secs(120))?;
        let token = self.token_client.exchange_code(
            &self.creds.client_id,
            &self.creds.client_secret,
            &callback.code,
            &redirect_uri,
            &pkce.verifier,
        )?;
        let refresh_token = token
            .refresh_token
            .ok_or_else(|| crate::Error::OAuth("Google did not return a refresh token; revoke app access at https://myaccount.google.com/permissions and sign in again".into()))?;
        self.store.save_refresh_token(&refresh_token)
    }

    pub fn access_token(&self) -> crate::Result<String> {
        let refresh_token = self
            .store
            .load_refresh_token()?
            .ok_or_else(|| crate::Error::OAuth("not signed in".into()))?;
        let token = self.token_client.refresh(&self.creds.client_id, &self.creds.client_secret, &refresh_token)?;
        if let Some(rotated) = &token.refresh_token {
            self.store.save_refresh_token(rotated)?;
        }
        Ok(token.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::{InMemoryStore, TokenStore};
    use httpmock::MockServer;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    struct RecordingBrowser(Arc<Mutex<Option<String>>>);

    impl BrowserOpener for RecordingBrowser {
        fn open(&self, url: &str) -> crate::Result<()> {
            *self.0.lock().unwrap() = Some(url.to_owned());
            Ok(())
        }
    }

    fn test_creds() -> GoogleClientCredentials {
        GoogleClientCredentials { client_id: "client-id".to_owned(), client_secret: "client-secret".to_owned() }
    }

    #[test]
    fn status_reflects_store_contents() {
        let auth = GoogleAuth::new(InMemoryStore::new(), RecordingBrowser(Arc::new(Mutex::new(None))), crate::oauth::TokenClient::new().unwrap(), test_creds());
        assert_eq!(auth.status().unwrap(), AuthStatus::SignedOut);
        auth.store.save_refresh_token("r").unwrap();
        assert_eq!(auth.status().unwrap(), AuthStatus::SignedIn);
        auth.sign_out().unwrap();
        assert_eq!(auth.status().unwrap(), AuthStatus::SignedOut);
    }

    #[test]
    fn auth_status_needs_only_the_token_store() {
        let store = InMemoryStore::new();
        assert_eq!(auth_status(&store).unwrap(), AuthStatus::SignedOut);
        store.save_refresh_token("r").unwrap();
        assert_eq!(auth_status(&store).unwrap(), AuthStatus::SignedIn);
    }

    #[test]
    fn access_token_without_sign_in_is_an_error() {
        let auth = GoogleAuth::new(InMemoryStore::new(), RecordingBrowser(Arc::new(Mutex::new(None))), crate::oauth::TokenClient::new().unwrap(), test_creds());
        let err = auth.access_token().unwrap_err();
        assert!(matches!(err, crate::Error::OAuth(_)));
    }

    #[test]
    fn full_sign_in_flow_extracts_port_opens_browser_and_stores_refresh_token() {
        let token_server = MockServer::start();
        token_server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token").body_includes("grant_type=authorization_code");
            then.status(200).json_body(serde_json::json!({
                "access_token": "access-1",
                "expires_in": 3600,
                "refresh_token": "refresh-1",
                "scope": crate::SCOPE_DRIVE_APPDATA,
                "token_type": "Bearer"
            }));
        });

        let browser_url = Arc::new(Mutex::new(None));
        let auth = GoogleAuth::new(
            InMemoryStore::new(),
            RecordingBrowser(browser_url.clone()),
            crate::oauth::TokenClient::new().unwrap().with_base_url(&token_server.base_url()),
            test_creds(),
        );

        // sign_in はブラウザ起動後にループバックの応答を待ち続けるので、
        // 別スレッドでテストが「ユーザーの認可完了」を模したリダイレクトを送る。
        let handle = std::thread::spawn(move || auth.sign_in(crate::SCOPE_DRIVE_APPDATA).map(|()| auth));
        let mut fired = false;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            if let Some(url) = browser_url.lock().unwrap().clone() {
                let parsed = url::Url::parse(&url).unwrap();
                let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
                let port: u16 = url::Url::parse(pairs.get("redirect_uri").unwrap())
                    .unwrap()
                    .port()
                    .unwrap();
                let state = pairs.get("state").unwrap().clone();
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let req = format!("GET /callback?code=auth-code-1&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
                stream.write_all(req.as_bytes()).unwrap();
                let mut discard = [0u8; 512];
                let _ = stream.read(&mut discard);
                fired = true;
                break;
            }
        }
        assert!(fired, "sign_in never opened the browser with a redirect_uri");

        let auth = handle.join().unwrap().unwrap();
        assert_eq!(auth.status().unwrap(), AuthStatus::SignedIn);
    }

    #[test]
    fn missing_refresh_token_in_response_is_a_clear_error() {
        let token_server = MockServer::start();
        token_server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token");
            then.status(200).json_body(serde_json::json!({
                "access_token": "access-1",
                "expires_in": 3600,
                "scope": crate::SCOPE_DRIVE_APPDATA,
                "token_type": "Bearer"
            }));
        });
        let browser_url = Arc::new(Mutex::new(None));
        let auth = GoogleAuth::new(
            InMemoryStore::new(),
            RecordingBrowser(browser_url.clone()),
            crate::oauth::TokenClient::new().unwrap().with_base_url(&token_server.base_url()),
            test_creds(),
        );
        let handle = std::thread::spawn(move || auth.sign_in(crate::SCOPE_DRIVE_APPDATA));
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            if let Some(url) = browser_url.lock().unwrap().clone() {
                let parsed = url::Url::parse(&url).unwrap();
                let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
                let port: u16 = url::Url::parse(pairs.get("redirect_uri").unwrap()).unwrap().port().unwrap();
                let state = pairs.get("state").unwrap().clone();
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let req = format!("GET /callback?code=auth-code-1&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
                stream.write_all(req.as_bytes()).unwrap();
                let mut discard = [0u8; 512];
                let _ = stream.read(&mut discard);
                break;
            }
        }
        let err = handle.join().unwrap().unwrap_err();
        assert!(matches!(err, crate::Error::OAuth(_)));
    }

    #[test]
    fn sign_in_requests_the_scope_it_is_given() {
        let token_server = MockServer::start();
        token_server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token");
            then.status(200).json_body(serde_json::json!({
                "access_token": "access-1",
                "expires_in": 3600,
                "refresh_token": "refresh-1",
                "scope": format!("{} {}", crate::SCOPE_DRIVE_APPDATA, crate::SCOPE_CALENDAR_READONLY),
                "token_type": "Bearer"
            }));
        });
        let browser_url = Arc::new(Mutex::new(None));
        let auth = GoogleAuth::new(
            InMemoryStore::new(),
            RecordingBrowser(browser_url.clone()),
            crate::oauth::TokenClient::new().unwrap().with_base_url(&token_server.base_url()),
            test_creds(),
        );
        let combined_scope = format!("{} {}", crate::SCOPE_DRIVE_APPDATA, crate::SCOPE_CALENDAR_READONLY);
        let handle = std::thread::spawn(move || auth.sign_in(&combined_scope));
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            if let Some(url) = browser_url.lock().unwrap().clone() {
                let parsed = url::Url::parse(&url).unwrap();
                let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
                assert_eq!(pairs.get("scope").unwrap(), &format!("{} {}", crate::SCOPE_DRIVE_APPDATA, crate::SCOPE_CALENDAR_READONLY));
                let port: u16 = url::Url::parse(pairs.get("redirect_uri").unwrap()).unwrap().port().unwrap();
                let state = pairs.get("state").unwrap().clone();
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let req = format!("GET /callback?code=auth-code-1&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
                stream.write_all(req.as_bytes()).unwrap();
                let mut discard = [0u8; 512];
                let _ = stream.read(&mut discard);
                break;
            }
        }
        handle.join().unwrap().unwrap();
    }
}
