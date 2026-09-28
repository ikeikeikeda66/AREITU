pub mod auth;
pub mod decision;
pub mod drive;
pub mod keychain;
pub mod loopback;
pub mod oauth;
pub mod snapshot;
pub mod state;
pub mod swap;
pub mod sync;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("oauth: {0}")]
    OAuth(String),
    #[error("keychain: {0}")]
    Keychain(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// このフェーズでリクエストするスコープは appDataFolder のみ。
/// カレンダーへの incremental auth は Phase 3 で別スコープを追加する。
pub const SCOPE_DRIVE_APPDATA: &str = "https://www.googleapis.com/auth/drive.appdata";

/// Phase 3 で追加するスコープ。ユーザーが設定でカレンダー連携を有効にしたときだけ、
/// `SCOPE_DRIVE_APPDATA` と合わせて incremental authorization でリクエストする。
pub const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn scope_is_drive_appdata_only() {
        assert_eq!(SCOPE_DRIVE_APPDATA, "https://www.googleapis.com/auth/drive.appdata");
    }

    #[test]
    fn calendar_readonly_scope_constant_is_correct() {
        assert_eq!(SCOPE_CALENDAR_READONLY, "https://www.googleapis.com/auth/calendar.readonly");
    }
}
