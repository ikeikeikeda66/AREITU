# Phase 2B: Google OAuth + Drive 同期 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** デスクトップ OAuth 2.0 Authorization Code + PKCE で Google にサインインし、`areitu.db`（SQLite 単一ファイル）を Google Drive の appDataFolder に安全に同期する Rust ライブラリ `areitu-google` を作る。UI は作らない。最終タスクでのみ、Tauri コマンド（`google_sign_in` / `google_sign_out` / `google_status` / `drive_sync_now`）としての結線と、既存のバックグラウンドポーリングへの組み込みを行う。

**Architecture:** 新規クレート `crates/areitu-google`（ライブラリ）を workspace に追加する。外部 I/O（OAuth トークンエンドポイント、Drive API、OS キーチェーン、システムブラウザ起動、ループバック HTTP サーバー）はすべて小さなトレイトか `base_url` 差し替え可能な構造体の背後に置き、テストは `httpmock` によるローカルモックサーバーとフェイク実装だけで完結させる（`areitu-core` の `ReverseGeocoder` / `LlmClient` トレイトと同じ設計パターンを踏襲する）。データの流れは「PKCE 生成 → ループバック待受 → ブラウザで認可画面 → コード交換 → リフレッシュトークンをキーチェーンへ → （同期時）DB を `VACUUM INTO` でスナップショット → appDataFolder の状態と比較 → アップロード / ダウンロード / 競合退避 → 同期状態を JSON に保存」。

**Tech Stack:** Rust edition 2024（既存ワークスペースと同じ）。`reqwest`（blocking, json）、`rusqlite`（bundled）、`serde` / `serde_json`、`thiserror`、`chrono`、`url`、`rand`、`sha2`、`base64`、`keyring`、`webbrowser`。テスト用に `tempfile`、`httpmock`。

**Spec:** GitHub issue #17（Desktop OAuth 2.0 Authorization Code + PKCE, ループバックリダイレクト）と issue #16（Drive appDataFolder への `areitu.db` 同期）。全体ロードマップ: `docs/superpowers/plans/2026-09-26-roadmap.md`。並行して進む Phase 2A（Tauri デスクトップアプリ, `docs/superpowers/plans/2026-09-27-phase2a-desktop-app.md`）が `apps/desktop` を作る前提で、本プランの最終タスクはその成果物に依存する。

## Global Constraints

- Rust edition 2024（`crates/areitu-core` と同じ）。clippy は `cargo clippy --workspace --all-targets -- -D warnings` で警告ゼロを保つ
- CI は ubuntu-latest / macos-latest / windows-latest の3 OS で `cargo test --workspace` を通す
- 独自サーバーを持たない。データは単一ファイル `areitu.db`（SQLite）のまま、Google Drive の appDataFolder（ユーザー自身のアカウント）にのみ同期する
- 今回リクエストするスコープは `https://www.googleapis.com/auth/drive.appdata` のみ（カレンダーは Phase 3 で incremental auth により追加）
- リフレッシュトークンは OS キーチェーンにのみ保存する（`keyring` クレート経由）。アクセストークン・リフレッシュトークンは一切ログに出さない（`Debug` 実装でリダクトする）
- Google の Client ID / Secret はビルド時の環境変数 `AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` から `option_env!` で読む。未設定時は分かりやすいエラーを返す
- ループバックサーバーは `127.0.0.1` のランダムポートで待ち受け、期待した1回のリダイレクトだけを受理し、タイムアウトする
- 外部 HTTP はすべてトレイトか `base_url` 差し替え可能な構造体の背後に置き、テストは `httpmock` のローカルサーバーで行う。ライブの Google 呼び出しを行う手動テストは各フローにつき1つだけ `#[ignore]` で残す
- クレート作成は `cargo new --vcs none` を使う
- コミットメッセージは本文の後に空行を1つ置き、`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` で終える
- 依存クレートのバージョンは実行時に `cargo add <crate>` で解決する（本プランではバージョンを固定しない）

## Review Focus

1. Google が既に許可済みのアカウントで再ログインし、リフレッシュトークンを返さない場合 → `sign_in` は黒歴史なアクセストークンだけで成功したように見せず、明確なエラーを返す（Task 7 でテスト）
2. ブラウザがリダイレクトを返さない、あるいはユーザーが認可画面を閉じた場合 → ループバックサーバーは無限に待たず、タイムアウトで抜ける（Task 4 でテスト）
3. リダイレクトの `state` が期待値と一致しない（古いタブの再利用や CSRF の疑い）場合 → コードを受理せず、明確な `OAuth` エラーとして拒否する（Task 4 でテスト）
4. appDataFolder に同名ファイルが0件、または前回の異常終了で残った同名ファイルが複数件ある場合 → クラッシュせず、0件は「まだ同期していない」として扱い、複数件は先頭の1件を対象にする（Task 8 でテスト）
5. 直前の同期が `VACUUM INTO` の一時ファイルを消し忘れて異常終了した場合 → 次回のスナップショット作成は「ファイルが既に存在する」で失敗しない（Task 11 でテスト）

## File Structure

```
Cargo.toml                                  workspace（members に areitu-google を追加）
crates/areitu-google/
  Cargo.toml
  src/lib.rs           モジュール宣言, Error / Result, SCOPE_DRIVE_APPDATA
  src/oauth.rs          PKCE 生成, state 生成, 認可URL構築, TokenClient（交換・リフレッシュ）
  src/loopback.rs        127.0.0.1 の1回限りのリダイレクト受信サーバー
  src/keychain.rs         TokenStore トレイト, KeyringStore（本物）, InMemoryStore（テスト用）
  src/auth.rs             BrowserOpener トレイト, GoogleAuth（sign_in / sign_out / status / access_token）
  src/drive.rs            DriveApi トレイト, DriveClient（files.list / multipart upload / download）
  src/snapshot.rs          VACUUM INTO によるスナップショットと SHA-256 ハッシュ
  src/state.rs             SyncState の JSON 永続化
  src/decision.rs          同期方針を決める純粋関数 decide()
  src/swap.rs              ダウンロードした DB へのアトミックな入れ替え
  src/sync.rs              上記すべてを束ねる sync_now()
```

---

### Task 1: クレート雛形と共通エラー型

**Files:**
- Create: `crates/areitu-google/Cargo.toml`
- Create: `crates/areitu-google/src/lib.rs`
- Modify: `Cargo.toml`（workspace members）

**Interfaces:**
- Consumes: なし（最初のタスク）
- Produces: `areitu_google::Error`, `areitu_google::Result<T>`, `areitu_google::SCOPE_DRIVE_APPDATA: &str`。以降のすべてのタスクがこの `Error` / `Result` を使う

- [ ] **Step 1: クレートを作成し workspace に登録する**

```bash
cd /Users/ikedashinichi/AREITU
cargo new --vcs none --lib crates/areitu-google
```

Expected: `Created library \`areitu-google\` package` のような出力。`crates/areitu-google/{Cargo.toml,src/lib.rs}` が生成される。

`Cargo.toml`（workspace ルート）を編集する。

```toml
[workspace]
resolver = "2"
members = ["crates/areitu-core", "crates/areitu-cli", "crates/areitu-google"]
```

- [ ] **Step 2: 依存クレートを追加する（本プラン全体で使うものを一度に入れる）**

```bash
cd /Users/ikedashinichi/AREITU/crates/areitu-google
cargo add reqwest --features blocking,json
cargo add rusqlite --features bundled
cargo add serde --features derive
cargo add serde_json
cargo add thiserror
cargo add chrono
cargo add url
cargo add rand
cargo add sha2
cargo add base64
cargo add keyring
cargo add webbrowser
cargo add --dev tempfile
cargo add --dev httpmock
```

Expected: 各コマンドが `Adding <crate> vX.Y.Z to dependencies` を出力し、`crates/areitu-google/Cargo.toml` の `[dependencies]` / `[dev-dependencies]` に追記される。

- [ ] **Step 3: 失敗するテストを書く（`Error` がまだ存在しないことを確認する）**

`crates/areitu-google/src/lib.rs` を次の内容で置き換える（テスト部分のみ先に書く）。

```rust
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
}
```

- [ ] **Step 2: 確認 — テストを実行し `Error` 未定義で失敗することを確認する**

Run: `cargo test -p areitu-google`
Expected: `error[E0433]: failed to resolve: use of undeclared type \`Error\`` を含むコンパイルエラー

- [ ] **Step 3: 最小実装を書く**

`crates/areitu-google/src/lib.rs` の先頭（テストモジュールの前）に追加する。

```rust
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
```

まだ存在しないモジュールファイルを空で作成する。

```bash
cd /Users/ikedashinichi/AREITU/crates/areitu-google/src
for f in auth decision drive keychain loopback oauth snapshot state swap sync; do
  printf '// implemented in later tasks of docs/superpowers/plans/2026-09-27-phase2b-google-sync.md\n' > "$f.rs"
done
```

- [ ] **Step 4: テストを実行し成功を確認する**

Run: `cargo test -p areitu-google`
Expected: `test tests::io_error_converts ... ok` と `test tests::scope_is_drive_appdata_only ... ok`、`test result: ok. 2 passed`

Run: `cargo clippy -p areitu-google --all-targets -- -D warnings`
Expected: 警告・エラーなしで終了（`Finished` のみ）

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add Cargo.toml crates/areitu-google
git commit -m "$(cat <<'EOF'
feat(areitu-google): scaffold crate with shared Error/Result

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: PKCE と state の生成

**Files:**
- Modify: `crates/areitu-google/src/oauth.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub struct Pkce { pub verifier: String, pub challenge: String }`, `pub fn generate_pkce() -> Pkce`, `pub fn generate_state() -> String`。Task 3（認可URL構築）と Task 7（sign_in）がこれを使う

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-google/src/oauth.rs` に追記する。

```rust
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
```

- [ ] **Step 2: 確認 — 実装を一時的に消してテストが失敗することを確認する**

Run: `cargo test -p areitu-google oauth::`
Expected: 上記コードをそのまま追加した段階では既に実装込みなので、まず `random_url_safe_token` 等の実装部分をコメントアウトしてから実行し、`cannot find function` 系のエラーになることを確認してから元に戻す。

- [ ] **Step 4: テストを実行し成功を確認する**

Run: `cargo test -p areitu-google oauth::`
Expected: `test oauth::tests::state_is_nonempty_and_varies ... ok`、`test oauth::tests::pkce_challenge_is_sha256_of_verifier ... ok`、`test oauth::tests::pkce_verifier_has_no_padding_or_plus_slash ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/oauth.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): generate PKCE verifier/challenge and state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 認可URLの構築

**Files:**
- Modify: `crates/areitu-google/src/oauth.rs`

**Interfaces:**
- Consumes: なし（純粋関数）
- Produces: `pub struct AuthorizeUrlParams<'a> { pub client_id: &'a str, pub redirect_uri: &'a str, pub scope: &'a str, pub state: &'a str, pub code_challenge: &'a str }`, `pub fn build_authorize_url(base_url: &str, p: &AuthorizeUrlParams) -> Result<String>`。Task 7 の `sign_in` が使う

**想定するエンドポイント（仮定として明記する）:** `https://accounts.google.com/o/oauth2/v2/auth`

- [ ] **Step 1: 失敗するテストを書く**

```rust
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
```

- [ ] **Step 2: 確認 — テストを実行し失敗することを確認する**

上記のうち `build_authorize_url` 本体を先にコメントアウトした状態で実行する。
Run: `cargo test -p areitu-google authorize_url_tests`
Expected: `cannot find function \`build_authorize_url\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google authorize_url_tests`
Expected: `test oauth::authorize_url_tests::includes_all_required_query_params ... ok`、`test oauth::authorize_url_tests::rejects_malformed_base_url ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/oauth.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): build Google OAuth authorize URL with PKCE params

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: ループバックリダイレクトサーバー

**Files:**
- Modify: `crates/areitu-google/src/loopback.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub struct CallbackResult { pub code: String, pub state: String }`, `pub fn bind_loopback() -> Result<(std::net::TcpListener, u16)>`, `pub fn await_callback(listener: std::net::TcpListener, expected_state: &str, timeout: std::time::Duration) -> Result<CallbackResult>`。Task 7 の `sign_in` が使う

- [ ] **Step 1: 失敗するテストを書く**

```rust
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::{Error, Result};

pub struct CallbackResult {
    pub code: String,
    pub state: String,
}

pub fn bind_loopback() -> Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

pub fn await_callback(listener: TcpListener, expected_state: &str, timeout: Duration) -> Result<CallbackResult> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return handle_connection(stream, expected_state),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(Error::OAuth("timed out waiting for the OAuth redirect".into()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
}

fn handle_connection(mut stream: TcpStream, expected_state: &str) -> Result<CallbackResult> {
    stream.set_nonblocking(false)?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
    let first_line = request.lines().next().unwrap_or("").to_owned();
    let path_and_query = first_line.split_whitespace().nth(1).unwrap_or("").to_owned();
    let query = path_and_query.split_once('?').map(|(_, q)| q.to_owned()).unwrap_or_default();
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let outcome = match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) if state == expected_state => {
            Ok(CallbackResult { code: code.clone(), state: state.clone() })
        }
        (Some(_), Some(_)) => Err(Error::OAuth("state mismatch on OAuth redirect".into())),
        _ => Err(Error::OAuth("missing code or state on OAuth redirect".into())),
    };

    let body = match &outcome {
        Ok(_) => "<html><body>AREITU: sign-in complete. You can close this tab.</body></html>",
        Err(_) => "<html><body>AREITU: sign-in failed. You can close this tab.</body></html>",
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_get(port: u16, path_and_query: &str) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!("GET {path_and_query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut discard = [0u8; 512];
        let _ = stream.read(&mut discard);
    }

    #[test]
    fn accepts_matching_code_and_state() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=expected-state");
        let result = handle.join().unwrap().unwrap();
        assert_eq!(result.code, "auth-code-1");
        assert_eq!(result.state, "expected-state");
    }

    #[test]
    fn rejects_state_mismatch() {
        let (listener, port) = bind_loopback().unwrap();
        let handle = std::thread::spawn(move || await_callback(listener, "expected-state", Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        send_get(port, "/callback?code=auth-code-1&state=wrong-state");
        let err = handle.join().unwrap().unwrap_err();
        assert!(matches!(err, Error::OAuth(_)));
    }

    #[test]
    fn times_out_when_nothing_connects() {
        let (listener, _port) = bind_loopback().unwrap();
        let started = Instant::now();
        let err = await_callback(listener, "expected-state", Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, Error::OAuth(_)));
        assert!(started.elapsed() >= Duration::from_millis(200));
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google loopback::`
Expected: `cannot find function \`bind_loopback\`` を含むコンパイルエラー（実装部分をコメントアウトした状態）

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google loopback::`
Expected: `test loopback::tests::accepts_matching_code_and_state ... ok`、`test loopback::tests::rejects_state_mismatch ... ok`、`test loopback::tests::times_out_when_nothing_connects ... ok`（3件とも ok、合計テスト時間は概ね1秒未満）

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/loopback.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): single-shot loopback server for OAuth redirect

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: トークン交換とリフレッシュ

**Files:**
- Modify: `crates/areitu-google/src/oauth.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub struct TokenResponse { pub access_token: String, pub expires_in: i64, pub refresh_token: Option<String>, pub scope: String, pub token_type: String }`（`Debug` はトークンをリダクトする）、`pub struct TokenClient { .. }` に `pub fn new() -> Result<TokenClient>`, `pub fn with_base_url(self, url: &str) -> Self`, `pub fn exchange_code(&self, client_id: &str, client_secret: &str, code: &str, redirect_uri: &str, code_verifier: &str) -> Result<TokenResponse>`, `pub fn refresh(&self, client_id: &str, client_secret: &str, refresh_token: &str) -> Result<TokenResponse>`。Task 7 の `GoogleAuth` が使う

**想定するエンドポイント（仮定として明記する）:** `https://oauth2.googleapis.com/token`（`grant_type=authorization_code` と `grant_type=refresh_token` の両方をここに POST する）

- [ ] **Step 1: 失敗するテストを書く**

```rust
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
                .body_contains("grant_type=authorization_code")
                .body_contains("code_verifier=verifier-1");
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
            when.method(httpmock::Method::POST).path("/token").body_contains("grant_type=refresh_token");
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
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google token_client_tests`
Expected: `cannot find struct \`TokenClient\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google token_client_tests`
Expected: `test oauth::token_client_tests::exchange_code_parses_token_response ... ok`、`... refresh_uses_refresh_token_grant ... ok`、`... revoked_refresh_token_is_an_error ... ok`、`... debug_output_never_contains_raw_tokens ... ok`（`live_refresh_with_real_credentials` は `#[ignore]` により実行されない）

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/oauth.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): exchange and refresh Google OAuth tokens

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: リフレッシュトークンの保管（キーチェーン）

**Files:**
- Modify: `crates/areitu-google/src/keychain.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub trait TokenStore { fn save_refresh_token(&self, refresh_token: &str) -> Result<()>; fn load_refresh_token(&self) -> Result<Option<String>>; fn clear_refresh_token(&self) -> Result<()>; }`, `pub struct KeyringStore;`（実装のみ）, `#[cfg(test)] pub struct InMemoryStore { .. }`（テスト用フェイク）。Task 7 の `GoogleAuth<S, B>` がジェネリックとして使う

**注意（Global Constraints 継承）:** OS のキーチェーン実体に触れるテストは CI 環境（特に ubuntu-latest のヘッドレス環境）で secret-service が無く失敗しうるため、`areitu-core::Nominatim::live_tokyo_station` と同じ方針で `#[ignore]` にする。デフォルトの `cargo test` はトレイト経由の `InMemoryStore` だけを検証する。

- [ ] **Step 1: 失敗するテストを書く**

```rust
pub trait TokenStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()>;
    fn load_refresh_token(&self) -> crate::Result<Option<String>>;
    fn clear_refresh_token(&self) -> crate::Result<()>;
}

const SERVICE: &str = "com.areitu.google";
const ACCOUNT: &str = "refresh_token";

pub struct KeyringStore;

impl TokenStore for KeyringStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        entry.set_password(refresh_token).map_err(|e| crate::Error::Keychain(e.to_string()))
    }

    fn load_refresh_token(&self) -> crate::Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(crate::Error::Keychain(e.to_string())),
        }
    }

    fn clear_refresh_token(&self) -> crate::Result<()> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(crate::Error::Keychain(e.to_string())),
        }
    }
}

#[cfg(test)]
pub struct InMemoryStore(std::sync::Mutex<Option<String>>);

#[cfg(test)]
impl InMemoryStore {
    pub fn new() -> Self {
        InMemoryStore(std::sync::Mutex::new(None))
    }
}

#[cfg(test)]
impl TokenStore for InMemoryStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()> {
        *self.0.lock().unwrap() = Some(refresh_token.to_owned());
        Ok(())
    }

    fn load_refresh_token(&self) -> crate::Result<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn clear_refresh_token(&self) -> crate::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryStore::new();
        assert_eq!(store.load_refresh_token().unwrap(), None);
        store.save_refresh_token("token-1").unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), Some("token-1".to_owned()));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn clearing_an_already_empty_store_is_not_an_error() {
        let store = InMemoryStore::new();
        assert!(store.clear_refresh_token().is_ok());
    }

    #[test]
    #[ignore = "touches the real OS keychain / secret service"]
    fn keyring_store_round_trips_on_this_machine() {
        let store = KeyringStore;
        store.save_refresh_token("areitu-test-token").unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), Some("areitu-test-token".to_owned()));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google keychain::`
Expected: `cannot find struct \`InMemoryStore\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google keychain::`
Expected: `test keychain::tests::in_memory_store_round_trips ... ok`、`... clearing_an_already_empty_store_is_not_an_error ... ok`（`keyring_store_round_trips_on_this_machine` は `#[ignore]` により実行されない）

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/keychain.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): store refresh token behind a TokenStore trait

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: サインイン / サインアウト / 状態確認の統合

**Files:**
- Modify: `crates/areitu-google/src/auth.rs`

**Interfaces:**
- Consumes: `crate::oauth::{generate_pkce, generate_state, build_authorize_url, AuthorizeUrlParams, TokenClient}`, `crate::loopback::{bind_loopback, await_callback}`, `crate::keychain::TokenStore`
- Produces: `pub trait BrowserOpener { fn open(&self, url: &str) -> Result<()>; }`, `pub struct SystemBrowser;`, `pub struct GoogleClientCredentials { pub client_id: String, pub client_secret: String }`, `pub fn client_credentials_from_env() -> Result<GoogleClientCredentials>`, `#[derive(Debug, Clone, PartialEq)] pub enum AuthStatus { SignedOut, SignedIn }`, `pub struct GoogleAuth<S: TokenStore, B: BrowserOpener> { .. }` に `pub fn new(store: S, browser: B, token_client: TokenClient, creds: GoogleClientCredentials) -> Self`, `pub fn status(&self) -> Result<AuthStatus>`, `pub fn sign_in(&self) -> Result<()>`, `pub fn sign_out(&self) -> Result<()>`, `pub fn access_token(&self) -> Result<String>`。Task 16（Tauri 結線）がこれをそのまま呼ぶ

- [ ] **Step 1: 失敗するテストを書く**

```rust
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
        Ok(match self.store.load_refresh_token()? {
            Some(_) => AuthStatus::SignedIn,
            None => AuthStatus::SignedOut,
        })
    }

    pub fn sign_out(&self) -> crate::Result<()> {
        self.store.clear_refresh_token()
    }

    pub fn sign_in(&self) -> crate::Result<()> {
        let (listener, port) = crate::loopback::bind_loopback()?;
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let pkce = crate::oauth::generate_pkce();
        let state = crate::oauth::generate_state();
        let url = crate::oauth::build_authorize_url(
            &self.authorize_base_url,
            &crate::oauth::AuthorizeUrlParams {
                client_id: &self.creds.client_id,
                redirect_uri: &redirect_uri,
                scope: crate::SCOPE_DRIVE_APPDATA,
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
    use crate::keychain::InMemoryStore;
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
    fn access_token_without_sign_in_is_an_error() {
        let auth = GoogleAuth::new(InMemoryStore::new(), RecordingBrowser(Arc::new(Mutex::new(None))), crate::oauth::TokenClient::new().unwrap(), test_creds());
        let err = auth.access_token().unwrap_err();
        assert!(matches!(err, crate::Error::OAuth(_)));
    }

    #[test]
    fn full_sign_in_flow_extracts_port_opens_browser_and_stores_refresh_token() {
        let token_server = MockServer::start();
        token_server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/token").body_contains("grant_type=authorization_code");
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
        let handle = std::thread::spawn(move || auth.sign_in().map(|()| auth));
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
        let handle = std::thread::spawn(move || auth.sign_in());
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
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google auth::`
Expected: `cannot find struct \`GoogleAuth\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google auth::`
Expected: `test auth::tests::status_reflects_store_contents ... ok`、`... access_token_without_sign_in_is_an_error ... ok`、`... full_sign_in_flow_extracts_port_opens_browser_and_stores_refresh_token ... ok`、`... missing_refresh_token_in_response_is_a_clear_error ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/auth.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): wire PKCE + loopback + token exchange into sign_in/sign_out/status

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Drive appDataFolder のファイル検索

**Files:**
- Modify: `crates/areitu-google/src/drive.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `#[derive(Debug, Clone, PartialEq, serde::Deserialize)] pub struct DriveFile { pub id: String, pub name: String, pub modified_time: Option<String>, pub md5_checksum: Option<String> }`, `pub trait DriveApi { fn find_db_file(&self, access_token: &str, name: &str) -> Result<Option<DriveFile>>; .. }`（他メソッドは Task 9・10 で追加）, `pub struct DriveClient { .. }` に `pub fn new() -> Result<DriveClient>`, `pub fn with_base_url`, `pub fn with_upload_base_url`。Task 15 の `sync_now` が `DriveApi` トレイトを経由して使う

**想定するエンドポイント（仮定として明記する）:** `GET https://www.googleapis.com/drive/v3/files?spaces=appDataFolder&fields=files(id,name,modifiedTime,md5Checksum)&q=name='<name>'`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "modifiedTime")]
    pub modified_time: Option<String>,
    #[serde(default, rename = "md5Checksum")]
    pub md5_checksum: Option<String>,
}

#[derive(serde::Deserialize)]
struct FilesListResponse {
    #[serde(default)]
    files: Vec<DriveFile>,
}

pub trait DriveApi {
    fn find_db_file(&self, access_token: &str, name: &str) -> crate::Result<Option<DriveFile>>;
}

pub struct DriveClient {
    client: reqwest::blocking::Client,
    base_url: String,
    upload_base_url: String,
}

impl DriveClient {
    pub fn new() -> crate::Result<DriveClient> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(DriveClient {
            client,
            base_url: "https://www.googleapis.com".to_owned(),
            upload_base_url: "https://www.googleapis.com/upload".to_owned(),
        })
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    pub fn with_upload_base_url(mut self, url: &str) -> Self {
        self.upload_base_url = url.trim_end_matches('/').to_owned();
        self
    }
}

impl DriveApi for DriveClient {
    fn find_db_file(&self, access_token: &str, name: &str) -> crate::Result<Option<DriveFile>> {
        let resp = self
            .client
            .get(format!("{}/drive/v3/files", self.base_url))
            .bearer_auth(access_token)
            .query(&[
                ("spaces", "appDataFolder"),
                ("fields", "files(id,name,modifiedTime,md5Checksum)"),
                ("q", &format!("name = '{name}'")),
            ])
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive files.list status {}", resp.status())));
        }
        let parsed: FilesListResponse = resp.json().map_err(|e| crate::Error::Http(e.to_string()))?;
        Ok(parsed.files.into_iter().next())
    }
}

#[cfg(test)]
mod find_db_file_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn returns_none_when_no_file_matches() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(200).json_body(serde_json::json!({"files": []}));
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert_eq!(drive.find_db_file("token", "areitu.db").unwrap(), None);
    }

    #[test]
    fn returns_first_match_when_duplicates_exist() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(200).json_body(serde_json::json!({"files": [
                {"id": "file-1", "name": "areitu.db", "modifiedTime": "2026-09-27T00:00:00Z", "md5Checksum": "abc"},
                {"id": "file-2", "name": "areitu.db", "modifiedTime": "2026-09-26T00:00:00Z", "md5Checksum": "def"}
            ]}));
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        let found = drive.find_db_file("token", "areitu.db").unwrap().unwrap();
        assert_eq!(found.id, "file-1");
    }

    #[test]
    fn non_success_status_is_an_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files");
            then.status(401);
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert!(drive.find_db_file("token", "areitu.db").is_err());
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google find_db_file_tests`
Expected: `cannot find struct \`DriveClient\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google find_db_file_tests`
Expected: `test drive::find_db_file_tests::returns_none_when_no_file_matches ... ok`、`... returns_first_match_when_duplicates_exist ... ok`、`... non_success_status_is_an_error ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/drive.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): list appDataFolder files behind a DriveApi trait

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Drive へのマルチパートアップロード（作成・更新）

**Files:**
- Modify: `crates/areitu-google/src/drive.rs`

**Interfaces:**
- Consumes: `DriveClient`（Task 8）
- Produces: `DriveApi` に `fn upload_create(&self, access_token: &str, name: &str, content: &[u8]) -> Result<DriveFile>`, `fn upload_update(&self, access_token: &str, file_id: &str, content: &[u8]) -> Result<DriveFile>` を追加し、`DriveClient` がそれを実装する。`pub fn build_multipart_related_body(boundary: &str, metadata: &serde_json::Value, content: &[u8]) -> Vec<u8>` を公開する。Task 15 が使う

**想定するエンドポイント（仮定として明記する）:** `POST https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart`（新規作成、`parents: ["appDataFolder"]` をメタデータに含める）、`PATCH https://www.googleapis.com/upload/drive/v3/files/{fileId}?uploadType=multipart`（既存ファイルの内容更新）。どちらも `multipart/related` ボディで、reqwest の `multipart::Form`（`multipart/form-data` 用）は使わず手組みする

- [ ] **Step 1: 失敗するテストを書く**

```rust
pub fn build_multipart_related_body(boundary: &str, metadata: &serde_json::Value, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n").as_bytes());
    body.extend_from_slice(metadata.to_string().as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl DriveClient {
    fn multipart_request(
        &self,
        method: reqwest::Method,
        url: &str,
        access_token: &str,
        metadata: &serde_json::Value,
        content: &[u8],
    ) -> crate::Result<DriveFile> {
        const BOUNDARY: &str = "areitu-sync-boundary";
        let body = build_multipart_related_body(BOUNDARY, metadata, content);
        let resp = self
            .client
            .request(method, url)
            .bearer_auth(access_token)
            .header("Content-Type", format!("multipart/related; boundary={BOUNDARY}"))
            .body(body)
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive upload status {}", resp.status())));
        }
        resp.json().map_err(|e| crate::Error::Http(e.to_string()))
    }
}

impl DriveApi for DriveClient {
    fn upload_create(&self, access_token: &str, name: &str, content: &[u8]) -> crate::Result<DriveFile> {
        let metadata = serde_json::json!({"name": name, "parents": ["appDataFolder"]});
        let url = format!("{}/drive/v3/files?uploadType=multipart", self.upload_base_url);
        self.multipart_request(reqwest::Method::POST, &url, access_token, &metadata, content)
    }

    fn upload_update(&self, access_token: &str, file_id: &str, content: &[u8]) -> crate::Result<DriveFile> {
        let metadata = serde_json::json!({});
        let url = format!("{}/drive/v3/files/{file_id}?uploadType=multipart", self.upload_base_url);
        self.multipart_request(reqwest::Method::PATCH, &url, access_token, &metadata, content)
    }
}

#[cfg(test)]
mod upload_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn multipart_body_contains_boundaries_metadata_and_content() {
        let metadata = serde_json::json!({"name": "areitu.db", "parents": ["appDataFolder"]});
        let body = build_multipart_related_body("BOUNDARY", &metadata, b"binary-db-bytes");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUNDARY\r\n"));
        assert!(text.contains("Content-Type: application/json"));
        assert!(text.contains("\"name\":\"areitu.db\""));
        assert!(text.contains("Content-Type: application/octet-stream"));
        assert!(text.contains("binary-db-bytes"));
        assert!(text.ends_with("--BOUNDARY--\r\n"));
    }

    #[test]
    fn upload_create_posts_to_upload_endpoint_with_parents() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/upload/drive/v3/files")
                .query_param("uploadType", "multipart")
                .body_contains("appDataFolder");
            then.status(200).json_body(serde_json::json!({"id": "new-file-1", "name": "areitu.db"}));
        });
        let drive = DriveClient::new().unwrap().with_upload_base_url(&format!("{}/upload", server.base_url()));
        let created = drive.upload_create("token", "areitu.db", b"db-bytes").unwrap();
        mock.assert();
        assert_eq!(created.id, "new-file-1");
    }

    #[test]
    fn upload_update_patches_existing_file_id() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path("/upload/drive/v3/files/existing-file-1")
                .query_param("uploadType", "multipart");
            then.status(200).json_body(serde_json::json!({"id": "existing-file-1", "name": "areitu.db"}));
        });
        let drive = DriveClient::new().unwrap().with_upload_base_url(&format!("{}/upload", server.base_url()));
        let updated = drive.upload_update("token", "existing-file-1", b"new-db-bytes").unwrap();
        mock.assert();
        assert_eq!(updated.id, "existing-file-1");
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google upload_tests`
Expected: `cannot find function \`build_multipart_related_body\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google upload_tests`
Expected: `test drive::upload_tests::multipart_body_contains_boundaries_metadata_and_content ... ok`、`... upload_create_posts_to_upload_endpoint_with_parents ... ok`、`... upload_update_patches_existing_file_id ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/drive.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): upload/update areitu.db via multipart/related to Drive

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: Drive からのダウンロード

**Files:**
- Modify: `crates/areitu-google/src/drive.rs`

**Interfaces:**
- Consumes: `DriveClient`（Task 8, 9）
- Produces: `DriveApi` に `fn download(&self, access_token: &str, file_id: &str) -> Result<Vec<u8>>` を追加。Task 15 が使う

**想定するエンドポイント（仮定として明記する）:** `GET https://www.googleapis.com/drive/v3/files/{fileId}?alt=media`

- [ ] **Step 1: 失敗するテストを書く**

```rust
impl DriveApi for DriveClient {
    fn download(&self, access_token: &str, file_id: &str) -> crate::Result<Vec<u8>> {
        let resp = self
            .client
            .get(format!("{}/drive/v3/files/{file_id}", self.base_url))
            .bearer_auth(access_token)
            .query(&[("alt", "media")])
            .send()
            .map_err(|e| crate::Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(crate::Error::Http(format!("drive download status {}", resp.status())));
        }
        Ok(resp.bytes().map_err(|e| crate::Error::Http(e.to_string()))?.to_vec())
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;
    use httpmock::MockServer;

    #[test]
    fn downloads_raw_bytes_with_alt_media() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/drive/v3/files/remote-file-1")
                .query_param("alt", "media");
            then.status(200).body(b"sqlite-db-content".to_vec());
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        let bytes = drive.download("token", "remote-file-1").unwrap();
        mock.assert();
        assert_eq!(bytes, b"sqlite-db-content".to_vec());
    }

    #[test]
    fn missing_file_is_an_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/drive/v3/files/missing");
            then.status(404);
        });
        let drive = DriveClient::new().unwrap().with_base_url(&server.base_url());
        assert!(drive.download("token", "missing").is_err());
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google download_tests`
Expected: `no method named \`download\` found` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google download_tests`
Expected: `test drive::download_tests::downloads_raw_bytes_with_alt_media ... ok`、`... missing_file_is_an_error ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/drive.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): download areitu.db content from Drive

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: DB スナップショット（VACUUM INTO）とハッシュ

**Files:**
- Modify: `crates/areitu-google/src/snapshot.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub fn vacuum_into(source_db: &std::path::Path, dest_path: &std::path::Path) -> Result<()>`, `pub fn sha256_hex(path: &std::path::Path) -> Result<String>`。Task 15 の `sync_now` が使う

- [ ] **Step 1: 失敗するテストを書く**

```rust
use std::path::Path;

pub fn vacuum_into(source_db: &Path, dest_path: &Path) -> crate::Result<()> {
    if dest_path.exists() {
        std::fs::remove_file(dest_path)?;
    }
    let conn = rusqlite::Connection::open(source_db)?;
    conn.execute("VACUUM INTO ?1", rusqlite::params![dest_path.to_string_lossy()])?;
    Ok(())
}

pub fn sha256_hex(path: &Path) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(digest.as_slice().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t (v) VALUES ('hello');").unwrap();
    }

    #[test]
    fn snapshot_contains_same_rows_as_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("areitu.db");
        let dest = dir.path().join("areitu.snapshot");
        make_db(&source);
        vacuum_into(&source, &dest).unwrap();
        let conn = rusqlite::Connection::open(&dest).unwrap();
        let v: String = conn.query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hello");
    }

    #[test]
    fn snapshot_overwrites_a_leftover_file_from_a_crashed_previous_run() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("areitu.db");
        let dest = dir.path().join("areitu.snapshot");
        make_db(&source);
        std::fs::write(&dest, b"stale leftover bytes from a crashed sync").unwrap();
        vacuum_into(&source, &dest).unwrap();
        let conn = rusqlite::Connection::open(&dest).unwrap();
        let v: String = conn.query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hello");
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        let hash = sha256_hex(&path).unwrap();
        assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha256_hex_differs_for_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"content-a").unwrap();
        std::fs::write(&b, b"content-b").unwrap();
        assert_ne!(sha256_hex(&a).unwrap(), sha256_hex(&b).unwrap());
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google snapshot::`
Expected: `cannot find function \`vacuum_into\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google snapshot::`
Expected: `test snapshot::tests::snapshot_contains_same_rows_as_source ... ok`、`... snapshot_overwrites_a_leftover_file_from_a_crashed_previous_run ... ok`、`... sha256_hex_matches_known_vector ... ok`、`... sha256_hex_differs_for_different_content ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/snapshot.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): snapshot the live DB with VACUUM INTO and hash it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: 同期状態の永続化

**Files:**
- Modify: `crates/areitu-google/src/state.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)] pub struct SyncState { pub remote_file_id: Option<String>, pub remote_modified_time: Option<String>, pub local_content_hash: Option<String> }`, `pub fn load_state(path: &std::path::Path) -> Result<SyncState>`, `pub fn save_state(path: &std::path::Path, state: &SyncState) -> Result<()>`。Task 15 が使う

- [ ] **Step 1: 失敗するテストを書く**

```rust
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyncState {
    pub remote_file_id: Option<String>,
    pub remote_modified_time: Option<String>,
    pub local_content_hash: Option<String>,
}

pub fn load_state(path: &Path) -> crate::Result<SyncState> {
    if !path.exists() {
        return Ok(SyncState::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_state(path: &Path, state: &SyncState) -> crate::Result<()> {
    let raw = serde_json::to_string_pretty(state)?;
    std::fs::write(path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_state_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        assert_eq!(load_state(&path).unwrap(), SyncState::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        let state = SyncState {
            remote_file_id: Some("file-1".to_owned()),
            remote_modified_time: Some("2026-09-27T00:00:00Z".to_owned()),
            local_content_hash: Some("abc123".to_owned()),
        };
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path).unwrap(), state);
    }

    #[test]
    fn corrupt_state_file_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_state(&path).is_err());
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google state::`
Expected: `cannot find function \`load_state\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google state::`
Expected: `test state::tests::missing_state_file_loads_as_default ... ok`、`... save_then_load_round_trips ... ok`、`... corrupt_state_file_is_an_error_not_a_silent_reset ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/state.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): persist sync state as JSON

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 13: 同期方針を決める純粋関数

**Files:**
- Modify: `crates/areitu-google/src/decision.rs`

**Interfaces:**
- Consumes: なし（純粋関数）
- Produces: `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Decision { NoOp, Upload, Download, UploadWithConflictBackup }`, `pub fn decide(remote_changed: bool, local_changed: bool) -> Decision`。Task 15 が使う

**同期方針（すでに決定済みのルール、そのまま実装する）:**
- リモート不変・ローカル変化 → アップロード
- リモート変化・ローカル不変 → ダウンロードしてローカルを安全に置き換える
- 両方変化 → ローカルをアップロードし、直前のリモートの内容を `areitu-conflict-<timestamp>.db` として appDataFolder に退避し、Conflict を返す
- 両方不変 → 何もしない

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    NoOp,
    Upload,
    Download,
    UploadWithConflictBackup,
}

pub fn decide(remote_changed: bool, local_changed: bool) -> Decision {
    match (remote_changed, local_changed) {
        (false, false) => Decision::NoOp,
        (false, true) => Decision::Upload,
        (true, false) => Decision::Download,
        (true, true) => Decision::UploadWithConflictBackup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neither_changed_is_noop() {
        assert_eq!(decide(false, false), Decision::NoOp);
    }

    #[test]
    fn only_local_changed_uploads() {
        assert_eq!(decide(false, true), Decision::Upload);
    }

    #[test]
    fn only_remote_changed_downloads() {
        assert_eq!(decide(true, false), Decision::Download);
    }

    #[test]
    fn both_changed_uploads_with_conflict_backup() {
        assert_eq!(decide(true, true), Decision::UploadWithConflictBackup);
    }

    #[test]
    fn decision_covers_all_four_boolean_combinations_exhaustively() {
        let all: std::collections::HashSet<Decision> = [
            decide(false, false),
            decide(false, true),
            decide(true, false),
            decide(true, true),
        ]
        .into_iter()
        .collect();
        assert_eq!(all.len(), 4, "each of the 4 input combinations must map to a distinct decision");
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google decision::`
Expected: `cannot find function \`decide\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google decision::`
Expected: `test decision::tests::neither_changed_is_noop ... ok`、`... only_local_changed_uploads ... ok`、`... only_remote_changed_downloads ... ok`、`... both_changed_uploads_with_conflict_backup ... ok`、`... decision_covers_all_four_boolean_combinations_exhaustively ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/decision.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): pure decide() for the 4-way sync decision table

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 14: ダウンロードした DB のアトミックな入れ替え

**Files:**
- Modify: `crates/areitu-google/src/swap.rs`

**Interfaces:**
- Consumes: `crate::Error`, `crate::Result`
- Produces: `pub fn swap_db_files(current_db_path: &std::path::Path, downloaded_db_path: &std::path::Path) -> Result<()>`。呼び出し側（Tauri 統合、Task 16）が SQLite 接続の close/reopen を担当する前提

- [ ] **Step 1: 失敗するテストを書く**

```rust
use std::path::Path;

pub fn swap_db_files(current_db_path: &Path, downloaded_db_path: &Path) -> crate::Result<()> {
    let backup_path = current_db_path.with_extension("db.bak");
    if current_db_path.exists() {
        std::fs::rename(current_db_path, &backup_path)?;
    }
    match std::fs::rename(downloaded_db_path, current_db_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup_path);
            Ok(())
        }
        Err(e) => {
            if backup_path.exists() {
                let _ = std::fs::rename(&backup_path, current_db_path);
            }
            Err(crate::Error::Io(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_current_db_with_downloaded_one() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let downloaded = dir.path().join("areitu.db.downloaded");
        std::fs::write(&current, b"old content").unwrap();
        std::fs::write(&downloaded, b"new content").unwrap();
        swap_db_files(&current, &downloaded).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"new content");
        assert!(!downloaded.exists());
        assert!(!current.with_extension("db.bak").exists());
    }

    #[test]
    fn works_even_when_current_db_did_not_exist_yet() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let downloaded = dir.path().join("areitu.db.downloaded");
        std::fs::write(&downloaded, b"first sync content").unwrap();
        swap_db_files(&current, &downloaded).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), b"first sync content");
    }

    #[test]
    fn restores_backup_if_the_final_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("areitu.db");
        let missing_downloaded = dir.path().join("does-not-exist.db");
        std::fs::write(&current, b"original content").unwrap();
        let err = swap_db_files(&current, &missing_downloaded).unwrap_err();
        assert!(matches!(err, crate::Error::Io(_)));
        assert_eq!(std::fs::read(&current).unwrap(), b"original content");
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google swap::`
Expected: `cannot find function \`swap_db_files\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google swap::`
Expected: `test swap::tests::replaces_current_db_with_downloaded_one ... ok`、`... works_even_when_current_db_did_not_exist_yet ... ok`、`... restores_backup_if_the_final_rename_fails ... ok`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/swap.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): atomically swap in a downloaded DB with rollback on failure

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 15: 同期オーケストレーション `sync_now`

**Files:**
- Modify: `crates/areitu-google/src/sync.rs`

**Interfaces:**
- Consumes: `crate::drive::{DriveApi, DriveFile}`, `crate::snapshot::{vacuum_into, sha256_hex}`, `crate::state::{SyncState, load_state, save_state}`, `crate::decision::{decide, Decision}`, `crate::swap::swap_db_files`
- Produces: `pub const DB_FILE_NAME: &str = "areitu.db";`, `#[derive(Debug, Clone, PartialEq)] pub enum SyncOutcome { NoOp, Uploaded, Downloaded, Conflict { conflict_backup_name: String } }`, `pub struct SyncContext<'a> { pub drive: &'a dyn DriveApi, pub access_token: &'a str, pub db_path: &'a std::path::Path, pub state_path: &'a std::path::Path }`, `pub fn sync_now(ctx: &SyncContext) -> Result<SyncOutcome>`。Task 16 の Tauri コマンド `drive_sync_now` が直接これを呼ぶ

- [ ] **Step 1: 失敗するテストを書く**

```rust
use crate::decision::{decide, Decision};
use crate::drive::{DriveApi, DriveFile};
use crate::snapshot::{sha256_hex, vacuum_into};
use crate::state::{load_state, save_state, SyncState};
use crate::swap::swap_db_files;
use std::path::Path;

pub const DB_FILE_NAME: &str = "areitu.db";

#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    NoOp,
    Uploaded,
    Downloaded,
    Conflict { conflict_backup_name: String },
}

pub struct SyncContext<'a> {
    pub drive: &'a dyn DriveApi,
    pub access_token: &'a str,
    pub db_path: &'a Path,
    pub state_path: &'a Path,
}

pub fn sync_now(ctx: &SyncContext) -> crate::Result<SyncOutcome> {
    let mut state = load_state(ctx.state_path)?;
    let dir = ctx
        .db_path
        .parent()
        .ok_or_else(|| crate::Error::Invalid("db_path has no parent directory".into()))?;
    let snapshot_path = dir.join(format!("{DB_FILE_NAME}.snapshot"));
    vacuum_into(ctx.db_path, &snapshot_path)?;
    let local_hash = sha256_hex(&snapshot_path)?;
    let remote = ctx.drive.find_db_file(ctx.access_token, DB_FILE_NAME)?;

    let outcome = if state.remote_file_id.is_none() {
        bootstrap(ctx, &mut state, remote, &snapshot_path, &local_hash)?
    } else {
        let remote_file = remote.ok_or_else(|| {
            crate::Error::Invalid("remote areitu.db disappeared from appDataFolder since the last sync".into())
        })?;
        steady_state(ctx, &mut state, remote_file, &snapshot_path, &local_hash)?
    };

    save_state(ctx.state_path, &state)?;
    let _ = std::fs::remove_file(&snapshot_path);
    Ok(outcome)
}

fn bootstrap(
    ctx: &SyncContext,
    state: &mut SyncState,
    remote: Option<DriveFile>,
    snapshot_path: &Path,
    local_hash: &str,
) -> crate::Result<SyncOutcome> {
    match remote {
        None => {
            let content = std::fs::read(snapshot_path)?;
            let uploaded = ctx.drive.upload_create(ctx.access_token, DB_FILE_NAME, &content)?;
            state.remote_file_id = Some(uploaded.id);
            state.remote_modified_time = uploaded.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Uploaded)
        }
        Some(remote_file) => {
            let bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let downloaded_path = snapshot_path.with_extension("downloaded");
            std::fs::write(&downloaded_path, &bytes)?;
            swap_db_files(ctx.db_path, &downloaded_path)?;
            state.remote_file_id = Some(remote_file.id);
            state.remote_modified_time = remote_file.modified_time;
            state.local_content_hash = Some(sha256_hex(ctx.db_path)?);
            Ok(SyncOutcome::Downloaded)
        }
    }
}

fn steady_state(
    ctx: &SyncContext,
    state: &mut SyncState,
    remote_file: DriveFile,
    snapshot_path: &Path,
    local_hash: &str,
) -> crate::Result<SyncOutcome> {
    let remote_changed = remote_file.modified_time != state.remote_modified_time;
    let local_changed = Some(local_hash.to_owned()) != state.local_content_hash;

    match decide(remote_changed, local_changed) {
        Decision::NoOp => Ok(SyncOutcome::NoOp),
        Decision::Upload => {
            let content = std::fs::read(snapshot_path)?;
            let updated = ctx.drive.upload_update(ctx.access_token, &remote_file.id, &content)?;
            state.remote_file_id = Some(updated.id);
            state.remote_modified_time = updated.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Uploaded)
        }
        Decision::Download => {
            let bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let downloaded_path = snapshot_path.with_extension("downloaded");
            std::fs::write(&downloaded_path, &bytes)?;
            swap_db_files(ctx.db_path, &downloaded_path)?;
            state.remote_file_id = Some(remote_file.id);
            state.remote_modified_time = remote_file.modified_time;
            state.local_content_hash = Some(sha256_hex(ctx.db_path)?);
            Ok(SyncOutcome::Downloaded)
        }
        Decision::UploadWithConflictBackup => {
            let remote_bytes = ctx.drive.download(ctx.access_token, &remote_file.id)?;
            let conflict_name = format!("areitu-conflict-{}.db", chrono::Local::now().format("%Y%m%dT%H%M%S"));
            ctx.drive.upload_create(ctx.access_token, &conflict_name, &remote_bytes)?;
            let content = std::fs::read(snapshot_path)?;
            let updated = ctx.drive.upload_update(ctx.access_token, &remote_file.id, &content)?;
            state.remote_file_id = Some(updated.id);
            state.remote_modified_time = updated.modified_time;
            state.local_content_hash = Some(local_hash.to_owned());
            Ok(SyncOutcome::Conflict { conflict_backup_name: conflict_name })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeDrive {
        files: Mutex<Vec<DriveFile>>,
        contents: Mutex<std::collections::HashMap<String, Vec<u8>>>,
        next_id: Mutex<u32>,
    }

    impl FakeDrive {
        fn empty() -> Self {
            FakeDrive { files: Mutex::new(vec![]), contents: Mutex::new(std::collections::HashMap::new()), next_id: Mutex::new(1) }
        }

        fn seeded(name: &str, modified_time: &str, content: &[u8]) -> Self {
            let d = Self::empty();
            let id = "seed-1".to_owned();
            d.files.lock().unwrap().push(DriveFile {
                id: id.clone(),
                name: name.to_owned(),
                modified_time: Some(modified_time.to_owned()),
                md5_checksum: None,
            });
            d.contents.lock().unwrap().insert(id, content.to_vec());
            d
        }

        fn next_file_id(&self) -> String {
            let mut n = self.next_id.lock().unwrap();
            *n += 1;
            format!("file-{n}")
        }
    }

    impl DriveApi for FakeDrive {
        fn find_db_file(&self, _access_token: &str, name: &str) -> crate::Result<Option<DriveFile>> {
            Ok(self.files.lock().unwrap().iter().find(|f| f.name == name).cloned())
        }

        fn upload_create(&self, _access_token: &str, name: &str, content: &[u8]) -> crate::Result<DriveFile> {
            let id = self.next_file_id();
            let file = DriveFile { id: id.clone(), name: name.to_owned(), modified_time: Some(format!("mtime-{id}")), md5_checksum: None };
            self.files.lock().unwrap().push(file.clone());
            self.contents.lock().unwrap().insert(id, content.to_vec());
            Ok(file)
        }

        fn upload_update(&self, _access_token: &str, file_id: &str, content: &[u8]) -> crate::Result<DriveFile> {
            let mut files = self.files.lock().unwrap();
            let file = files.iter_mut().find(|f| f.id == file_id).expect("file must exist");
            file.modified_time = Some(format!("mtime-updated-{file_id}"));
            let updated = file.clone();
            self.contents.lock().unwrap().insert(file_id.to_owned(), content.to_vec());
            Ok(updated)
        }

        fn download(&self, _access_token: &str, file_id: &str) -> crate::Result<Vec<u8>> {
            Ok(self.contents.lock().unwrap().get(file_id).cloned().expect("file content must exist"))
        }
    }

    fn make_local_db(dir: &Path, marker: &str) -> std::path::PathBuf {
        let path = dir.join("areitu.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS marker (v TEXT); DELETE FROM marker; INSERT INTO marker (v) VALUES ('{marker}');"
        ))
        .unwrap();
        path
    }

    #[test]
    fn bootstrap_with_no_remote_file_uploads_and_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "first-run");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Uploaded);
        assert_eq!(drive.files.lock().unwrap().len(), 1);
        let state = load_state(&state_path).unwrap();
        assert!(state.remote_file_id.is_some());
    }

    #[test]
    fn bootstrap_with_existing_remote_file_downloads_and_adopts_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "stale-local");
        let state_path = dir.path().join("sync-state.json");

        let remote_dir = tempfile::tempdir().unwrap();
        let remote_db_path = make_local_db(remote_dir.path(), "remote-content");
        let remote_bytes = std::fs::read(&remote_db_path).unwrap();
        let drive = FakeDrive::seeded(DB_FILE_NAME, "2026-09-27T00:00:00Z", &remote_bytes);

        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Downloaded);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "remote-content");
    }

    #[test]
    fn steady_state_neither_changed_is_noop_and_uploads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "unchanged");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        sync_now(&ctx).unwrap(); // 1回目: bootstrap upload
        let outcome = sync_now(&ctx).unwrap(); // 2回目: 何も変わっていない
        assert_eq!(outcome, SyncOutcome::NoOp);
        assert_eq!(drive.files.lock().unwrap().len(), 1, "no extra upload should have happened");
    }

    #[test]
    fn steady_state_local_change_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "v1");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };

        sync_now(&ctx).unwrap();
        make_local_db(dir.path(), "v2-changed-locally");
        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Uploaded);
    }

    #[test]
    fn steady_state_remote_change_downloads_and_replaces_local() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "local-unchanged");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        sync_now(&ctx).unwrap();

        // 別デバイスがリモートを更新したことを模す。
        let file_id = drive.files.lock().unwrap()[0].id.clone();
        let other_dir = tempfile::tempdir().unwrap();
        let other_db = make_local_db(other_dir.path(), "updated-elsewhere");
        let other_bytes = std::fs::read(&other_db).unwrap();
        drive.upload_update("token", &file_id, &other_bytes).unwrap();

        let outcome = sync_now(&ctx).unwrap();
        assert_eq!(outcome, SyncOutcome::Downloaded);
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "updated-elsewhere");
    }

    #[test]
    fn steady_state_both_changed_backs_up_remote_and_uploads_local() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = make_local_db(dir.path(), "v1");
        let state_path = dir.path().join("sync-state.json");
        let drive = FakeDrive::empty();
        let ctx = SyncContext { drive: &drive, access_token: "token", db_path: &db_path, state_path: &state_path };
        sync_now(&ctx).unwrap();

        // 両方が変わる: リモートは別デバイスから、ローカルはこのマシンから。
        let file_id = drive.files.lock().unwrap()[0].id.clone();
        let other_dir = tempfile::tempdir().unwrap();
        let other_db = make_local_db(other_dir.path(), "updated-elsewhere");
        let other_bytes = std::fs::read(&other_db).unwrap();
        drive.upload_update("token", &file_id, &other_bytes).unwrap();
        make_local_db(dir.path(), "updated-here-too");

        let outcome = sync_now(&ctx).unwrap();
        match outcome {
            SyncOutcome::Conflict { conflict_backup_name } => {
                assert!(conflict_backup_name.starts_with("areitu-conflict-"));
                assert!(conflict_backup_name.ends_with(".db"));
                let backed_up = drive.files.lock().unwrap().iter().any(|f| f.name == conflict_backup_name);
                assert!(backed_up, "the previous remote content must be preserved under the conflict name");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        // ローカルの内容がアップロードされて残っていること。
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let v: String = conn.query_row("SELECT v FROM marker", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "updated-here-too");
    }
}
```

- [ ] **Step 2: 確認 — 実装本体を一時的にコメントアウトして失敗を確認する**

Run: `cargo test -p areitu-google sync::`
Expected: `cannot find function \`sync_now\`` を含むコンパイルエラー

- [ ] **Step 4: コメントアウトを解除してテストを実行し成功を確認する**

Run: `cargo test -p areitu-google sync::`
Expected: `test sync::tests::bootstrap_with_no_remote_file_uploads_and_creates_it ... ok`、`... bootstrap_with_existing_remote_file_downloads_and_adopts_it ... ok`、`... steady_state_neither_changed_is_noop_and_uploads_nothing ... ok`、`... steady_state_local_change_uploads ... ok`、`... steady_state_remote_change_downloads_and_replaces_local ... ok`、`... steady_state_both_changed_backs_up_remote_and_uploads_local ... ok`

さらにワークスペース全体を確認する。

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: すべてのテストが `ok`、clippy は警告ゼロで `Finished` のみ

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add crates/areitu-google/src/sync.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): orchestrate snapshot/decision/upload/download into sync_now

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 16: Tauri コマンドへの結線とポーリングへの組み込み（Phase 2A 完了後に実行する）

**前提:** このタスクは Phase 2A（`docs/superpowers/plans/2026-09-27-phase2a-desktop-app.md`）の完了後に実行する。2A が作るもの: `main.rs` の `pub struct AppState { pub conn: Mutex<rusqlite::Connection>, pub config_path: PathBuf }` と `generate_handler!`、`sync.rs` の `pub fn spawn_poll_thread(app: AppHandle)`（2A Task 11 で `run_sync_with_config` を呼ぶ形に置き換え済み）。DB は `app.path().app_data_dir()?.join("areitu.db")`。着手前にこれらが実在することを確認し、名前が違えば実物に合わせる。

**DB 接続の扱い（必須）:** アプリは `AppState.conn` で `areitu.db` を開いたままにしている。ダウンロードで DB ファイルを入れ替える前に接続を閉じ、入れ替え後に開き直さないと、古い接続が残って同期結果が見えない、または DB が壊れる。そのため同期は必ず `conn` のロックを取った状態で行い、ロック中に接続をインメモリ接続に差し替えて（元の接続を drop して閉じる）から `sync_now` を呼び、終わったら結果にかかわらず `areitu_core::db::open(db_path)` で開き直す。

**Files:**
- Create: `apps/desktop/src-tauri/src/google.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`（`mod google;` とコマンド登録）
- Modify: `apps/desktop/src-tauri/src/sync.rs`（`spawn_poll_thread` の各サイクル末尾で Drive 同期）
- Modify: `apps/desktop/src-tauri/Cargo.toml`（`areitu-google` を依存に追加）

**Interfaces:**
- Consumes: `areitu_google::auth::{GoogleAuth, SystemBrowser, client_credentials_from_env, AuthStatus}`, `areitu_google::keychain::KeyringStore`, `areitu_google::oauth::TokenClient`, `areitu_google::drive::DriveClient`, `areitu_google::sync::{sync_now, SyncContext, SyncOutcome}`, 2A の `crate::AppState`
- Produces: 4つの Tauri コマンド — `google_sign_in() -> Result<(), String>`, `google_sign_out() -> Result<(), String>`, `google_status() -> Result<String, String>`（戻り値は `"signed_in"` / `"signed_out"`）, `drive_sync_now(app: AppHandle) -> Result<String, String>`（戻り値は `"no_op"` / `"uploaded"` / `"downloaded"` / `"conflict:<conflict_backup_name>"`）と、`pub fn drive_sync_locked(conn: &Mutex<rusqlite::Connection>, db_path: &Path, state_path: &Path) -> Result<String, String>`。`areitu_google::Error` は `to_string()` で文字列化する

- [ ] **Step 1: `areitu-google` を desktop アプリの依存に追加する**

```bash
cd /Users/ikedashinichi/AREITU/apps/desktop/src-tauri
cargo add areitu-google --path ../../../crates/areitu-google
```

Expected: `Adding areitu-google (local) to dependencies`。`apps/desktop/src-tauri/Cargo.toml` に `areitu-google = { path = "../../../crates/areitu-google" }` が追記される（実際のパス階層が異なる場合は 2A の構成に合わせて相対パスを直す）。

- [ ] **Step 2: `apps/desktop/src-tauri/src/google.rs` を作成する**

```rust
use areitu_google::auth::{client_credentials_from_env, AuthStatus, GoogleAuth, SystemBrowser};
use areitu_google::drive::DriveClient;
use areitu_google::keychain::KeyringStore;
use areitu_google::oauth::TokenClient;
use areitu_google::sync::{sync_now, SyncContext, SyncOutcome};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Closes the shared connection while `f` runs so `f` may replace the DB file, then reopens it.
pub fn with_db_closed<T>(
    conn: &Mutex<Connection>,
    db_path: &Path,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = conn.lock().map_err(|e| e.to_string())?;
    *guard = Connection::open_in_memory().map_err(|e| e.to_string())?;
    let result = f();
    *guard = areitu_core::db::open(db_path).map_err(|e| e.to_string())?;
    result
}

fn build_auth() -> Result<GoogleAuth<KeyringStore, SystemBrowser>, String> {
    let creds = client_credentials_from_env().map_err(|e| e.to_string())?;
    let token_client = TokenClient::new().map_err(|e| e.to_string())?;
    Ok(GoogleAuth::new(KeyringStore, SystemBrowser, token_client, creds))
}

#[tauri::command]
pub fn google_sign_in() -> Result<(), String> {
    build_auth()?.sign_in().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn google_sign_out() -> Result<(), String> {
    build_auth()?.sign_out().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn google_status() -> Result<String, String> {
    let status = build_auth()?.status().map_err(|e| e.to_string())?;
    Ok(match status {
        AuthStatus::SignedIn => "signed_in".to_owned(),
        AuthStatus::SignedOut => "signed_out".to_owned(),
    })
}

pub fn drive_sync_locked(conn: &Mutex<Connection>, db_path: &Path, state_path: &Path) -> Result<String, String> {
    let auth = build_auth()?;
    let access_token = auth.access_token().map_err(|e| e.to_string())?;
    let drive = DriveClient::new().map_err(|e| e.to_string())?;
    let (db_path, state_path) = (PathBuf::from(db_path), PathBuf::from(state_path));
    with_db_closed(conn, &db_path, || {
        let outcome = sync_now(&SyncContext {
            drive: &drive,
            access_token: &access_token,
            db_path: &db_path,
            state_path: &state_path,
        })
        .map_err(|e| e.to_string())?;
        Ok(match outcome {
            SyncOutcome::NoOp => "no_op".to_owned(),
            SyncOutcome::Uploaded => "uploaded".to_owned(),
            SyncOutcome::Downloaded => "downloaded".to_owned(),
            SyncOutcome::Conflict { conflict_backup_name } => format!("conflict:{conflict_backup_name}"),
        })
    })
}

pub fn sync_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok((dir.join("areitu.db"), dir.join("google-sync-state.json")))
}

#[tauri::command]
pub fn drive_sync_now(app: AppHandle) -> Result<String, String> {
    let (db_path, state_path) = sync_paths(&app)?;
    let state = app.state::<crate::AppState>();
    drive_sync_locked(&state.conn, &db_path, &state_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(conn: &Mutex<Connection>) -> i64 {
        conn.lock().unwrap().query_row("SELECT COUNT(*) FROM places", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn reopens_db_after_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("areitu.db");
        let remote = dir.path().join("remote.db");
        let conn = Mutex::new(areitu_core::db::open(&live).unwrap());
        let other = areitu_core::db::open(&remote).unwrap();
        other.execute("INSERT INTO places (name, lat, lon) VALUES ('X', 35.0, 139.0)", []).unwrap();
        drop(other);

        let out = with_db_closed(&conn, &live, || {
            std::fs::copy(&remote, &live).map_err(|e| e.to_string())?;
            Ok("downloaded")
        })
        .unwrap();

        assert_eq!(out, "downloaded");
        assert_eq!(count(&conn), 1);
    }

    #[test]
    fn reopens_db_even_when_sync_fails() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("areitu.db");
        let conn = Mutex::new(areitu_core::db::open(&live).unwrap());
        let r: Result<(), String> = with_db_closed(&conn, &live, || Err("offline".into()));
        assert_eq!(r.unwrap_err(), "offline");
        assert_eq!(count(&conn), 0);
    }
}
```

Run: `cd /Users/ikedashinichi/AREITU && cargo test -p <2A の src-tauri パッケージ名> google::`
Expected: 実装前（`with_db_closed` を書く前にテストだけ置いた状態）は `cannot find function with_db_closed` で FAIL、実装後は 2 passed。`tempfile` が src-tauri の dev-dependency にない場合は `cargo add --dev tempfile` を先に実行する。

- [ ] **Step 3: `main.rs` に4コマンドを登録し、ポーリングから `drive_sync_locked` を呼ぶ**

`apps/desktop/src-tauri/src/main.rs` に `mod google;` を宣言し、既存の `generate_handler!` の末尾に4コマンドを追加する。

```rust
mod google;
```

```rust
.invoke_handler(tauri::generate_handler![
    // ...2A が既に登録している既存コマンド...
    google::google_sign_in,
    google::google_sign_out,
    google::google_status,
    google::drive_sync_now,
])
```

`sync.rs` の `spawn_poll_thread` で、`run_sync_with_config` を呼んでロックを解放した直後（同じサイクル内）に次を追加する。`conn` のロックを保持したまま呼ぶとデッドロックするので、必ずロックのスコープの外で呼ぶ。未サインインや同期失敗でポーリング自体は止めない。

```rust
if let Ok((db_path, state_path)) = crate::google::sync_paths(&app) {
    if let Err(e) = crate::google::drive_sync_locked(&state.conn, &db_path, &state_path) {
        eprintln!("drive sync skipped this cycle: {e}");
    }
}
```

- [ ] **Step 4: ビルドを確認する**

Run: `cd /Users/ikedashinichi/AREITU/apps/desktop/src-tauri && cargo check`
Expected: エラーなく `Finished` （`AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` は `option_env!` 経由なのでビルド自体は環境変数なしでも通り、実行時に `google_sign_in` を呼んだときだけエラーになる）

Run: `cd /Users/ikedashinichi/AREITU && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 警告・エラーなしで `Finished`

- [ ] **Step 5: コミット**

```bash
cd /Users/ikedashinichi/AREITU
git add Cargo.lock apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/google.rs apps/desktop/src-tauri/src/main.rs apps/desktop/src-tauri/src/sync.rs
git commit -m "$(cat <<'EOF'
feat(desktop): wire areitu-google into Tauri commands and the polling cycle

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**
- issue #17（PKCE + ループバック + system browser + state check + code exchange + refresh + scope固定 + keyring + env変数のclient_id/secret）→ Task 2〜7 で実装。
- issue #16（VACUUM INTO スナップショット、appDataFolder への files.list / multipart upload・update / download、状態ファイル、決定ルールの純粋関数と網羅テスト、アトミック入れ替え）→ Task 8〜15 で実装。
- 「HTTP はトレイトか base_url 差し替え可能な構造体越し、テストは httpmock、ライブテストは各フローに1つ `#[ignore]`」→ `TokenClient` / `DriveClient` / `KeyringStore` の各テストで踏襲。
- 「トークンを絶対にログに出さない」→ `TokenResponse` の手書き `Debug` でリダクトし、テストで検証（Task 5）。
- 「Tauri コマンドとポーリングへの結線を最終タスクとして精密に記述」→ Task 16 で4コマンドの完全なシグネチャと戻り値の文字列表現、ポーリングループへの挿入位置を記述。

**2. Placeholder scan:** 各タスクのコードブロックはすべて実際に動くコードで書いた。「TODO」「あとで」「同様に実装する」に類する記述はない。Task 16 の一部コメントは 2A の実際のファイル配置・API名に合わせて読み替える旨を明記しているが、これは 2A が未着手のため生じる本質的な依存であり、コード自体は完全な実装として書いてある。

**3. Type consistency:** `Decision` / `SyncOutcome` / `AuthStatus` / `DriveFile` / `SyncState` の各フィールド名・列挙子は、定義したタスク（8, 12, 13, 7）から使用するタスク（15, 16）まで同じ名前で通している。`DriveApi` トレイトのメソッド名（`find_db_file` / `upload_create` / `upload_update` / `download`）は Task 8〜10 の定義と Task 15 の `FakeDrive` 実装・`sync_now` 呼び出しで一致させた。

**4. Review Focus 網羅:**
1. リフレッシュトークン欠落 → Task 7 の `missing_refresh_token_in_response_is_a_clear_error`
2. ループバックのタイムアウト → Task 4 の `times_out_when_nothing_connects`
3. state 不一致 → Task 4 の `rejects_state_mismatch`
4. appDataFolder の0件・複数件 → Task 8 の `returns_none_when_no_file_matches` / `returns_first_match_when_duplicates_exist`
5. VACUUM INTO の残骸ファイル → Task 11 の `snapshot_overwrites_a_leftover_file_from_a_crashed_previous_run`
以上5点すべてにテストが対応している。

## 実行時に確認・調整が必要な仮定（Assumptions）

- **エンドポイント:** 認可 `https://accounts.google.com/o/oauth2/v2/auth`、トークン `https://oauth2.googleapis.com/token`、Drive files.list `https://www.googleapis.com/drive/v3/files`、Drive アップロード `https://www.googleapis.com/upload/drive/v3/files`、Drive ダウンロード `https://www.googleapis.com/drive/v3/files/{fileId}?alt=media`。いずれも Google 公式ドキュメントに基づく現行の仕様だが、実行時に変更がないか確認すること。
- **`access_type=offline` + `prompt=consent`:** 毎回のサインインで確実にリフレッシュトークンを取得するために付与した。ユーザー体験として毎回同意画面が出る点は Phase 2A の UI 側と要相談。
- **keyring クレートのメソッド名:** `Entry::delete_credential()` は keyring 3.x の名称という前提で書いた。`cargo add` で解決されたバージョンによっては `delete_password()` である可能性があるため、Task 6 の実装時にドキュメントを確認する。
- **rand クレートの `rand::random::<u8>()`:** バージョン間でトップレベル関数のシグネチャが変わる可能性がある。`cargo add` 解決後にコンパイルが通らなければ `rand::rngs::OsRng` 等への置き換えを検討する。
- **Task 16 のファイルパス:** `apps/desktop/src-tauri/src/{main.rs,google.rs}` および `tauri::AppHandle::path().app_data_dir()` は Tauri 2 の一般的な構成を仮定したものであり、Phase 2A が実際に作る構成に合わせて調整する前提で書いている。

## ユーザーが決めるべきこと（このプランの実行前に）

- **Google Cloud の OAuth クライアント作成:** Google Cloud Console で「デスクトップアプリ」種別の OAuth 2.0 クライアントを作成し、Client ID / Secret を取得して `AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` としてビルド環境に設定する必要がある。これはユーザー自身の Google アカウント操作であり、本プランの範囲外。
- **OAuth consent screen のテストユーザー登録:** アプリが Google の審査を受けていない間は、consent screen に自分の Google アカウントをテストユーザーとして追加しておく必要がある。
