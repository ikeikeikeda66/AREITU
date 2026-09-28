// implemented in later tasks of docs/superpowers/plans/2026-09-27-phase2b-google-sync.md

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

fn random_url_safe_token(byte_len: usize) -> String {
    let bytes: Vec<u8> = (0..byte_len).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn generate_state() -> String {
    random_url_safe_token(32)
}

pub fn generate_pkce() -> Pkce {
    let verifier = random_url_safe_token(64);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest.as_slice());
    Pkce { verifier, challenge }
}

pub struct AuthorizeUrlParams<'a> {
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub scope: &'a str,
    pub state: &'a str,
    pub code_challenge: &'a str,
}

pub fn build_authorize_url(base_url: &str, p: &AuthorizeUrlParams) -> crate::Result<String> {
    let mut url = url::Url::parse(base_url)
        .map_err(|e| crate::Error::Invalid(format!("invalid authorize base url: {e}")))?;
    url.set_path("/o/oauth2/v2/auth");
    url.query_pairs_mut()
        .append_pair("client_id", p.client_id)
        .append_pair("redirect_uri", p.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", p.scope)
        .append_pair("code_challenge", p.code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", p.state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    Ok(url.to_string())
}

#[cfg(test)]
mod authorize_url_tests {
    use super::*;

    #[test]
    fn includes_all_required_query_params() {
        let params = AuthorizeUrlParams {
            client_id: "client-123",
            redirect_uri: "http://127.0.0.1:54321/callback",
            scope: crate::SCOPE_DRIVE_APPDATA,
            state: "state-abc",
            code_challenge: "challenge-xyz",
        };
        let url = build_authorize_url("https://accounts.google.com", &params).unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("client_id").unwrap(), "client-123");
        assert_eq!(pairs.get("redirect_uri").unwrap(), "http://127.0.0.1:54321/callback");
        assert_eq!(pairs.get("response_type").unwrap(), "code");
        assert_eq!(pairs.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(pairs.get("scope").unwrap(), crate::SCOPE_DRIVE_APPDATA);
        assert_eq!(pairs.get("state").unwrap(), "state-abc");
        assert_eq!(parsed.path(), "/o/oauth2/v2/auth");
    }

    #[test]
    fn rejects_malformed_base_url() {
        let params = AuthorizeUrlParams {
            client_id: "c",
            redirect_uri: "http://127.0.0.1/callback",
            scope: "s",
            state: "st",
            code_challenge: "cc",
        };
        assert!(build_authorize_url("not a url", &params).is_err());
    }
}

use std::time::Duration;

pub struct TokenClient {
    client: reqwest::blocking::Client,
    base_url: String,
}

#[derive(Clone, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub scope: String,
    pub token_type: String,
}

impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "<redacted>"))
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field("token_type", &self.token_type)
            .finish()
    }
}

impl TokenClient {
    pub fn new() -> crate::Result<TokenClient> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(TokenClient { client, base_url: "https://oauth2.googleapis.com".to_owned() })
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    pub fn exchange_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> crate::Result<TokenResponse> {
        self.post_token(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
    }

    pub fn refresh(&self, client_id: &str, client_secret: &str, refresh_token: &str) -> crate::Result<TokenResponse> {
        self.post_token(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        ])
    }

    fn post_token(&self, form: &[(&str, &str)]) -> crate::Result<TokenResponse> {
        let resp = self
            .client
            .post(format!("{}/token", self.base_url))
            .form(form)
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("token endpoint status {}", resp.status())));
        }
        resp.json().map_err(|e| crate::Error::Http(e.to_string()))
    }
}

#[cfg(test)]
mod token_client_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn exchange_code_parses_token_response() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/token")
                .body_includes("grant_type=authorization_code")
                .body_includes("code_verifier=verifier-1");
            then.status(200).json_body(serde_json::json!({
                "access_token": "access-1",
                "expires_in": 3600,
                "refresh_token": "refresh-1",
                "scope": crate::SCOPE_DRIVE_APPDATA,
                "token_type": "Bearer"
            }));
        });
        let client = TokenClient::new().unwrap().with_base_url(&server.base_url());
        let token = client
            .exchange_code("client-id", "client-secret", "auth-code", "http://127.0.0.1:1/callback", "verifier-1")
            .unwrap();
        mock.assert();
        assert_eq!(token.access_token, "access-1");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));
    }

    #[test]
    fn refresh_uses_refresh_token_grant() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token").body_includes("grant_type=refresh_token");
            then.status(200).json_body(serde_json::json!({
                "access_token": "access-2",
                "expires_in": 3600,
                "scope": crate::SCOPE_DRIVE_APPDATA,
                "token_type": "Bearer"
            }));
        });
        let client = TokenClient::new().unwrap().with_base_url(&server.base_url());
        let token = client.refresh("client-id", "client-secret", "refresh-1").unwrap();
        assert_eq!(token.access_token, "access-2");
        assert_eq!(token.refresh_token, None);
    }

    #[test]
    fn revoked_refresh_token_is_an_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token");
            then.status(400).json_body(serde_json::json!({"error": "invalid_grant"}));
        });
        let client = TokenClient::new().unwrap().with_base_url(&server.base_url());
        let err = client.refresh("client-id", "client-secret", "revoked").unwrap_err();
        assert!(matches!(err, crate::Error::Http(_)));
    }

    #[test]
    fn debug_output_never_contains_raw_tokens() {
        let token = TokenResponse {
            access_token: "super-secret-access".to_owned(),
            expires_in: 10,
            refresh_token: Some("super-secret-refresh".to_owned()),
            scope: "s".to_owned(),
            token_type: "Bearer".to_owned(),
        };
        let printed = format!("{token:?}");
        assert!(!printed.contains("super-secret-access"));
        assert!(!printed.contains("super-secret-refresh"));
    }

    #[test]
    #[ignore = "hits the real Google OAuth token endpoint"]
    fn live_refresh_with_real_credentials() {
        let creds = crate::auth::client_credentials_from_env().unwrap();
        let refresh_token = std::env::var("AREITU_TEST_GOOGLE_REFRESH_TOKEN").unwrap();
        let client = TokenClient::new().unwrap();
        let token = client.refresh(&creds.client_id, &creds.client_secret, &refresh_token).unwrap();
        assert!(!token.access_token.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_nonempty_and_varies() {
        let a = generate_state();
        let b = generate_state();
        assert!(!a.is_empty());
        assert_ne!(a, b);
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let p = generate_pkce();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier.as_bytes()).as_slice());
        assert_eq!(p.challenge, expected);
        assert!(!p.verifier.is_empty());
    }

    #[test]
    fn pkce_verifier_has_no_padding_or_plus_slash() {
        let p = generate_pkce();
        assert!(!p.verifier.contains('='));
        assert!(!p.verifier.contains('+'));
        assert!(!p.verifier.contains('/'));
    }
}
