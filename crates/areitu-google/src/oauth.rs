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
