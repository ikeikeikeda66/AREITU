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
