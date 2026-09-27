// implemented in later tasks of docs/superpowers/plans/2026-09-27-phase2b-google-sync.md

/// OAuth クライアントの client_id / client_secret。Task 7 の `GoogleAuth` が本格的に使う。
/// ここでは Task 5 の `#[ignore]` 付きライブテストがコンパイルできるよう、
/// 環境変数からの最小限の読み込みのみを提供する。
pub struct ClientCredentials {
    pub client_id: String,
    pub client_secret: String,
}

pub fn client_credentials_from_env() -> crate::Result<ClientCredentials> {
    let client_id = std::env::var("AREITU_GOOGLE_CLIENT_ID")
        .map_err(|_| crate::Error::Invalid("AREITU_GOOGLE_CLIENT_ID not set".to_owned()))?;
    let client_secret = std::env::var("AREITU_GOOGLE_CLIENT_SECRET")
        .map_err(|_| crate::Error::Invalid("AREITU_GOOGLE_CLIENT_SECRET not set".to_owned()))?;
    Ok(ClientCredentials { client_id, client_secret })
}
