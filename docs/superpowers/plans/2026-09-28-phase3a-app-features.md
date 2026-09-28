# Phase 3A: Calendar 自動取得・初回セットアップ・Google タイムライン取り込み Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Google Calendar の差分取得（syncToken による incremental sync）と incremental authorization、初回セットアップ画面、Google アカウント設定 UI、Google タイムライン（オンデバイスエクスポート／レガシー Takeout）の取り込みを実装し、一般ユーザーが GitHub Releases からインストールして「Google でログイン」とフォルダ選択だけでセットアップを終えられる状態にする。

**Architecture:** `areitu-google` に `calendar.rs`（Google Calendar API クライアント、events.list の差分ページング、HTTP 410 の検出）を追加し、`GoogleAuth::sign_in` はリクエストするスコープを呼び出し側から受け取るようにして incremental authorization（`include_granted_scopes=true`）に対応する。同期状態（syncToken）は Drive 同期状態と同じ「同一ディレクトリの一時ファイル→rename」パターンで JSON 永続化する。`areitu-core` は既存の `calendar::parse_events` はそのままに、キャンセル済みイベントの ID 抽出（`parse_cancelled_source_ids`）と、visit に未割り当ての raw_log だけを消す `store::delete_unassigned_raw_log` を追加する。Google タイムライン取り込みは `areitu-core::timeline` に新規実装し、`RawLog` の新しい `Source::Timeline` を経由して既存のクラスタリング／解決パイプラインにそのまま乗せる。デスクトップアプリ側は、カレンダー取り込みを既存の「写真スキャン→visit 構築」サイクル（`sync_on_own_connection`、`AppState.conn` を握らない専用コネクション）に合流させ、Drive 同期と同じ `sync_lock` の下で動かす。初回セットアップ画面と Google アカウント設定 UI はフロントエンドのみの追加で、既存の Tauri コマンドと新規コマンド（`setup_completed`、`import_timeline_file`）を呼ぶ。

**Tech Stack:** Rust edition 2024（`apps/desktop/src-tauri` もこの機会に統一する）。`areitu-google` は既存の `reqwest`（blocking, json, query）・`serde`/`serde_json`・`thiserror` を使い回す。フォルダ選択ダイアログに `tauri-plugin-dialog`（Rust）と `@tauri-apps/plugin-dialog`（npm）を新規導入する。バージョンは実行時に `cargo add` / `npm install` で解決する。

**Spec:** GitHub issue #18（Google Calendar 自動取得）・#19（初回セットアップ画面 + Google アカウント UI）・#20（Google タイムライン取り込み）。全体ロードマップ: `docs/superpowers/plans/2026-09-26-roadmap.md`（Phase 3 節）。先行フェーズ: `docs/superpowers/plans/2026-09-27-phase2a-desktop-app.md`、`docs/superpowers/plans/2026-09-27-phase2b-google-sync.md`（Phase 1〜2 はマージ済み、コードが正）。

## Global Constraints

- Rust edition 2024 を `apps/desktop/src-tauri` にも適用し、ワークスペース全体を揃える（Task 1）。clippy は `cargo clippy --workspace --all-targets -- -D warnings` で警告ゼロを保つ
- CI は ubuntu-latest / macos-latest / windows-latest の3 OS で `cargo test --workspace` を通す
- Tauri の境界を越える Rust の struct/enum は必ず `#[serde(rename_all = "camelCase")]` を付け、enum の文字列値は TypeScript の union と完全一致させる。そのような型ごとに、JSON キーの厳密一致を確認する Rust テストと、`invoke` の呼び出しコマンド名・引数キーの厳密一致を確認するフロントエンドテストを両方書く
- バックグラウンド同期（ポーリングスレッド・トレイの「今すぐ同期」・`sync_now`／新設のカレンダー取り込み）は `AppState.conn` を保持したままネットワーク呼び出しを行わない。ロック順序は必ず `sync_lock` → `conn`（既存の Phase 2B の規約を維持）
- テストは実際の Google エンドポイントに接続しない・ブラウザを開かない・実 OS キーチェーンに触れない（`areitu-google` の `httpmock`・`InMemoryStore`・フェイク実装パターンを踏襲する）
- 今回追加でリクエストするスコープは `https://www.googleapis.com/auth/calendar.readonly` のみ。ユーザーがカレンダー連携を有効にしたときだけ `include_granted_scopes=true` の incremental authorization で追加リクエストする
- raw_logs.source は SQLite 上 TEXT カラムで CHECK 制約を持たないため、新しい `Source::Timeline`（`"timeline"`）の追加にスキーマ変更・マイグレーションは不要（既存データはそのまま有効）
- コミットメッセージは本文の後に空行を1つ置き、`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` で終える
- クレートを新規作成する場合は `cargo new --vcs none` を使う（本プランでは新規クレートは作らない）
- 依存クレート／npm パッケージのバージョンは実行時に `cargo add` / `npm install`（バージョン指定なし）で解決する

## Review Focus

1. カレンダー連携をまだ許可していない（`drive.appdata` スコープだけでサインイン済みの）状態でユーザーが設定画面のトグルだけを ON にした場合 → カレンダー取得はアクセストークンを持たないまま実行を試みてクラッシュしたり無限リトライしたりせず、「Google に許可が必要」と分かる形で穏当に失敗し、写真同期は止めない（Task 7 でテスト）
2. ポーリング中に Google 側の syncToken が失効（HTTP 410）した場合 → 例外的にエラーを積み上げ続けるのではなく、syncToken を破棄して1回だけフルシンクをやり直す（Task 4・Task 6 でテスト）
3. 既に取り込み済みのカレンダー予定が Google 側でキャンセル・削除された場合 → 対応する raw_log は、まだ visit に割り当てられていなければ削除し、既に visit に組み込まれていれば削除しない（過去の訪問履歴を後から消してしまわないため）（Task 5・Task 6 でテスト）
4. Google タイムラインのエクスポート形式は「オンデバイス export（度数記号付き文字列・`geo:` URI）」と「レガシー Takeout（E7 整数座標）」の2種類があり、片方の形式しか読めないパーサーはもう一方のファイルを無言で空扱いにしてしまう（Task 13 でテスト）
5. 初回セットアップで「スキップ」を押した場合、あるいは Google サインインがブラウザを閉じられて失敗した場合 → アプリはその後も使える状態（config.json が存在し、次回起動時はオンボーディングを出さない）でなければならず、真っ白な画面やクラッシュループにしてはならない（Task 17・Task 18 でテスト）

## File Structure

```
crates/areitu-google/
  src/lib.rs             SCOPE_CALENDAR_READONLY 追加、Error::SyncTokenExpired 追加
  src/oauth.rs            build_authorize_url に include_granted_scopes=true を追加
  src/auth.rs             GoogleAuth::sign_in がスコープ文字列を引数に取るよう変更
  src/state.rs            JSON 永続化を汎用化し、CalendarSyncState を追加
  src/calendar.rs         新規: CalendarApi トレイト・CalendarClient・EventsListParams/EventsPage・fetch_all_pages
crates/areitu-core/
  src/calendar.rs         parse_cancelled_source_ids を追加
  src/store.rs            delete_unassigned_raw_log を追加
  src/model.rs             Source::Timeline を追加
  src/cluster.rs           lat/lon を持つログの text をそのクラスタ自身のヒントにも使う
  src/timeline.rs         新規: Google タイムライン（オンデバイス export／レガシー Takeout）のパーサーと取り込み
  src/lib.rs              pub mod timeline; を追加
apps/desktop/src-tauri/
  Cargo.toml               edition 2024 化、tauri-plugin-dialog 追加
  src/lib.rs                greet 削除、AppState.calendar_state_path 追加、dialog プラグイン登録
  src/google.rs             ingest_calendar_page_bodies / ingest_calendar / calendar_scope_for、google_sign_in の async 化
  src/sync.rs                run_sync_with_config・sync_on_own_connection(_locked) にカレンダー取り込みを合流、SyncSummary 拡張
  src/config.rs              calendar_enabled フィールド、SecretStore::get の Result 化、KeyStatus、config_exists
  src/commands.rs             SettingsDto/SaveSettingsDto 拡張、import_timeline_file、setup_completed
  src/tray.rs                 sync_on_own_connection_locked の呼び出し更新
  capabilities/default.json    dialog:default 権限を追加
apps/desktop/src/
  api/types.ts                Settings 拡張、GoogleStatus、DriveSyncOutcome 等
  api/tauri.ts                 google*/driveSyncNow/importTimelineFile/setupCompleted を追加
  api/tauri.test.ts            新規: invoke 呼び出しの契約テスト
  screens/SettingsScreen.tsx    Google アカウント区画、キー状態表示、カレンダートグル、タイムライン取り込みボタン
  screens/OnboardingScreen.tsx   新規: 初回セットアップ画面
  screens/OnboardingScreen.test.tsx 新規
  App.tsx                      setup_completed でオンボーディング分岐
  App.css                      削除（未参照）
```

---

### Task 1: クリーンアップ（edition 2024 統一・不要コードの削除）

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Delete: `apps/desktop/src/App.css`

**Interfaces:**
- Consumes: なし
- Produces: `apps/desktop/src-tauri` のワークスペース全体との edition 統一。以降のタスクは `greet` コマンドが存在しない前提で `invoke_handler!` を編集する

- [ ] **Step 1: `apps/desktop/src-tauri/Cargo.toml` の edition を書き換える**

`edition = "2021"` を `edition = "2024"` に変更する。

- [ ] **Step 2: ビルドが通ることを確認する**

Run: `cargo check -p areitu-desktop`
Expected: エラーなく終了する（edition 2024 化で新たに borrow-checker の指摘が出た場合はそのコードを直す。今回のコードベースには 2024 で問題になる書き方はない）

- [ ] **Step 3: `greet` コマンドと `App.css` を削除する**

`apps/desktop/src-tauri/src/lib.rs` から次を削除する:

```rust
// 削除: greet 関数本体
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
```

`invoke_handler![...]` の一覧から `greet,` の行を削除する。

`apps/desktop/src/App.css` を削除する（`apps/desktop/src/main.tsx` は `./styles.css` のみを import しており、`App.css` を import しているファイルは存在しない。フロントエンドの `greet`/`#greet-input` への参照も存在しない）。

- [ ] **Step 4: ビルドとテストを確認する**

Run: `cargo check -p areitu-desktop && cd apps/desktop && npm run build`
Expected: 両方ともエラーなく終了する

- [ ] **Step 5: コミット**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/lib.rs apps/desktop/src/App.css
git commit -m "$(cat <<'EOF'
chore: unify src-tauri to edition 2024 and drop scaffold greet/App.css

apps/desktop/src-tauri was still on the Tauri template's edition 2021
while every other crate in the workspace is on 2024, and the greet
command and App.css were unused leftovers from `tauri create`.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `areitu-google` — incremental authorization 対応

**Files:**
- Modify: `crates/areitu-google/src/lib.rs`
- Modify: `crates/areitu-google/src/oauth.rs`
- Modify: `crates/areitu-google/src/auth.rs`

**Interfaces:**
- Consumes: なし
- Produces: `areitu_google::SCOPE_CALENDAR_READONLY: &str`、`GoogleAuth::sign_in(&self, scope: &str) -> crate::Result<()>`（従来の引数なし `sign_in()` から変更）。Task 6・Task 7 のデスクトップ側 `build_auth()?.sign_in(&scope)` 呼び出しがこれを使う

- [ ] **Step 1: 失敗するテストを書く（スコープ定数と authorize URL の include_granted_scopes）**

`crates/areitu-google/src/lib.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn calendar_readonly_scope_constant_is_correct() {
        assert_eq!(SCOPE_CALENDAR_READONLY, "https://www.googleapis.com/auth/calendar.readonly");
    }
```

`crates/areitu-google/src/oauth.rs` の `mod authorize_url_tests` に追加:

```rust
    #[test]
    fn includes_granted_scopes_is_always_true() {
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
        assert_eq!(pairs.get("include_granted_scopes").unwrap(), "true");
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-google calendar_readonly_scope_constant_is_correct includes_granted_scopes_is_always_true`
Expected: FAIL（`SCOPE_CALENDAR_READONLY` が存在しない／`include_granted_scopes` パラメータが送られない）

- [ ] **Step 3: `SCOPE_CALENDAR_READONLY` と `include_granted_scopes=true` を実装する**

`crates/areitu-google/src/lib.rs` の `SCOPE_DRIVE_APPDATA` の下に追加:

```rust
/// Phase 3 で追加するスコープ。ユーザーが設定でカレンダー連携を有効にしたときだけ、
/// `SCOPE_DRIVE_APPDATA` と合わせて incremental authorization でリクエストする。
pub const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
```

`crates/areitu-google/src/oauth.rs` の `build_authorize_url` 内、`.append_pair("prompt", "consent")` の直前に追加:

```rust
        .append_pair("include_granted_scopes", "true")
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-google calendar_readonly_scope_constant_is_correct includes_granted_scopes_is_always_true`
Expected: PASS

- [ ] **Step 5: `GoogleAuth::sign_in` がスコープを引数で受け取るよう変更する（失敗するテストから）**

`crates/areitu-google/src/auth.rs` の `full_sign_in_flow_extracts_port_opens_browser_and_stores_refresh_token` と `missing_refresh_token_in_response_is_a_clear_error` の中の

```rust
        let handle = std::thread::spawn(move || auth.sign_in().map(|()| auth));
```

および

```rust
        let handle = std::thread::spawn(move || auth.sign_in());
```

を、それぞれ次に変更する:

```rust
        let handle = std::thread::spawn(move || auth.sign_in(crate::SCOPE_DRIVE_APPDATA).map(|()| auth));
```

```rust
        let handle = std::thread::spawn(move || auth.sign_in(crate::SCOPE_DRIVE_APPDATA));
```

さらに、カレンダースコープを含む URL がリクエストされることを確認する新規テストを追加する:

```rust
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
```

- [ ] **Step 6: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-google --lib auth::`
Expected: FAIL（`sign_in` はまだ引数を取らずコンパイルエラーになる）

- [ ] **Step 7: `sign_in` の実装を変更する**

`crates/areitu-google/src/auth.rs` の `sign_in` を次に変更する:

```rust
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
```

- [ ] **Step 8: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-google --lib auth:: oauth::`
Expected: PASS（全テスト）

- [ ] **Step 9: コミット**

```bash
git add crates/areitu-google/src/lib.rs crates/areitu-google/src/oauth.rs crates/areitu-google/src/auth.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): support incremental authorization for new scopes

GoogleAuth::sign_in now takes the scope string it should request, and
the authorize URL always sets include_granted_scopes=true. This lets
the desktop app request calendar.readonly on top of an already-granted
drive.appdata grant without losing the earlier scope.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `areitu-google` — 同期状態の JSON 永続化を汎用化し `CalendarSyncState` を追加

**Files:**
- Modify: `crates/areitu-google/src/state.rs`

**Interfaces:**
- Consumes: なし
- Produces: `areitu_google::state::CalendarSyncState { pub sync_token: Option<String> }`（`Default`, `PartialEq`, `Serialize`, `Deserialize`）、`load_calendar_state(path: &Path) -> crate::Result<CalendarSyncState>`、`save_calendar_state(path: &Path, state: &CalendarSyncState) -> crate::Result<()>`。Task 7 が使う

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-google/src/state.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn calendar_state_missing_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        assert_eq!(load_calendar_state(&path).unwrap(), CalendarSyncState::default());
    }

    #[test]
    fn calendar_state_save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        let state = CalendarSyncState { sync_token: Some("token-1".to_owned()) };
        save_calendar_state(&path, &state).unwrap();
        assert_eq!(load_calendar_state(&path).unwrap(), state);
    }

    #[test]
    fn calendar_state_corrupt_file_is_an_error_not_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-calendar-state.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_calendar_state(&path).is_err());
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-google --lib state::tests::calendar_state`
Expected: FAIL（`CalendarSyncState`/`load_calendar_state`/`save_calendar_state` が存在しない）

- [ ] **Step 3: 既存の書き込み・読み込みロジックを汎用ヘルパーに切り出し、`CalendarSyncState` を追加する**

`crates/areitu-google/src/state.rs` の `load_state`/`save_state` を次のように書き換える（ファイル全体の該当部分を置き換える）:

```rust
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyncState {
    pub remote_file_id: Option<String>,
    pub remote_modified_time: Option<String>,
    pub local_content_hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalendarSyncState {
    pub sync_token: Option<String>,
}

fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> crate::Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// 書き込みは同一ディレクトリの一時ファイルに行い、`rename` で置き換える。
/// クラッシュで半端な内容のファイルが残らないようにするため。
fn write_json_atomically<T: serde::Serialize>(path: &Path, value: &T) -> crate::Result<()> {
    let raw = serde_json::to_string_pretty(value)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp_path = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("state"),
        std::process::id()
    ));
    // 念のため既存の同名一時ファイルを避ける（実運用では PID で十分だがテストの再実行を考慮）。
    let mut suffix = 0u32;
    while tmp_path.exists() {
        suffix += 1;
        tmp_path = dir.join(format!(
            ".{}.tmp-{}-{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("state"),
            std::process::id(),
            suffix
        ));
    }
    std::fs::write(&tmp_path, raw)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn load_state(path: &Path) -> crate::Result<SyncState> {
    read_json_or_default(path)
}

pub fn save_state(path: &Path, state: &SyncState) -> crate::Result<()> {
    write_json_atomically(path, state)
}

pub fn load_calendar_state(path: &Path) -> crate::Result<CalendarSyncState> {
    read_json_or_default(path)
}

pub fn save_calendar_state(path: &Path, state: &CalendarSyncState) -> crate::Result<()> {
    write_json_atomically(path, state)
}
```

（このファイルの既存の `#[cfg(test)] mod tests` はそのまま残し、Step 1 で追加した3テストだけを末尾に足す。)

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-google --lib state::`
Expected: PASS（既存の `SyncState` 系テストも含めて全て）

- [ ] **Step 5: コミット**

```bash
git add crates/areitu-google/src/state.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): add CalendarSyncState alongside Drive SyncState

Generalizes the existing atomic-write JSON persistence in state.rs so
the new calendar sync token can reuse it instead of duplicating the
temp-file-then-rename logic.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: `areitu-google` — Google Calendar API クライアント（`calendar.rs`）

**Files:**
- Create: `crates/areitu-google/src/calendar.rs`
- Modify: `crates/areitu-google/src/lib.rs`

**Interfaces:**
- Consumes: `crate::Error`(`SyncTokenExpired` variant を新設)
- Produces: `areitu_google::calendar::{CalendarApi, CalendarClient, EventsListParams, EventsPage, FetchedEvents, fetch_all_pages}`。Task 6 の `ingest_calendar_page_bodies` がこれを使う

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-google/src/calendar.rs` を新規作成する:

```rust
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
```

- [ ] **Step 2: `crate::Error::SyncTokenExpired` を追加し、モジュールを登録する**

`crates/areitu-google/src/lib.rs` の `Error` enum に追加:

```rust
    #[error("calendar sync token expired (HTTP 410)")]
    SyncTokenExpired,
```

`pub mod auth;` の下に追加:

```rust
pub mod calendar;
```

- [ ] **Step 3: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-google --lib calendar::`
Expected: PASS(5テスト)

- [ ] **Step 4: コミット**

```bash
git add crates/areitu-google/src/calendar.rs crates/areitu-google/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(areitu-google): add a Google Calendar events.list client

Adds CalendarApi/CalendarClient with syncToken + pageToken pagination
and a dedicated SyncTokenExpired error for HTTP 410, so callers can
tell "no changes" apart from "the sync token is stale, do a full sync".

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `areitu-core` — カレンダーのキャンセル検出と未割り当て raw_log の削除

**Files:**
- Modify: `crates/areitu-core/src/calendar.rs`
- Modify: `crates/areitu-core/src/store.rs`

**Interfaces:**
- Consumes: 既存の `calendar::EventsResponse`/`Event`(同ファイル内 private struct)、既存の `store::upsert_raw_log`
- Produces: `areitu_core::calendar::parse_cancelled_source_ids(json: &str) -> Result<Vec<String>>`、`areitu_core::store::delete_unassigned_raw_log(conn: &Connection, source: Source, source_id: &str) -> Result<bool>`(戻り値は実際に削除したら `true`)。Task 6 の `ingest_calendar_page_bodies` がこれを使う

- [ ] **Step 1: 失敗するテストを書く(`parse_cancelled_source_ids`)**

`crates/areitu-core/src/calendar.rs` の `#[cfg(test)] mod tests` に追加(既存の `JSON` 定数の `a3` がキャンセル済み予定):

```rust
    #[test]
    fn cancelled_events_are_extracted_by_id() {
        assert_eq!(parse_cancelled_source_ids(JSON).unwrap(), vec!["a3"]);
    }

    #[test]
    fn no_cancelled_events_is_an_empty_list() {
        assert!(parse_cancelled_source_ids(r#"{"items":[{"id":"a1","status":"confirmed","start":{"dateTime":"2026-09-01T12:00:00+09:00"},"end":{"dateTime":"2026-09-01T13:00:00+09:00"}}]}"#).unwrap().is_empty());
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-core --lib calendar::tests::cancelled`
Expected: FAIL(`parse_cancelled_source_ids` が存在しない)

- [ ] **Step 3: `parse_cancelled_source_ids` を実装する**

`crates/areitu-core/src/calendar.rs` の `to_raw_log` の直後に追加:

```rust
pub fn parse_cancelled_source_ids(json: &str) -> Result<Vec<String>> {
    let resp: EventsResponse = serde_json::from_str(json)?;
    Ok(resp
        .items
        .into_iter()
        .filter(|e| e.status.as_deref() == Some("cancelled"))
        .map(|e| e.id)
        .collect())
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-core --lib calendar::`
Expected: PASS(既存テストも含め全て)

- [ ] **Step 5: 失敗するテストを書く(`delete_unassigned_raw_log`)**

`crates/areitu-core/src/store.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn deletes_an_unassigned_raw_log() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        let deleted = delete_unassigned_raw_log(&c, Source::Calendar, "e1").unwrap();
        assert!(deleted);
        assert!(unassigned_raw_logs(&c).unwrap().is_empty());
    }

    #[test]
    fn leaves_an_already_assigned_raw_log_untouched() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        let log_id = unassigned_raw_logs(&c).unwrap()[0].0;
        let place_id = find_or_create_place(&c, "カフェ丸の内", 35.0, 139.0).unwrap();
        c.execute(
            "INSERT INTO visits (place_id, started_at, ended_at, method) VALUES (?1, '2026-09-01 12:00', '2026-09-01 12:30', 'nominatim')",
            params![place_id],
        )
        .unwrap();
        let visit_id: i64 = c
            .query_row("SELECT id FROM visits WHERE place_id = ?1", params![place_id], |r| r.get(0))
            .unwrap();
        c.execute("UPDATE raw_logs SET visit_id = ?1 WHERE id = ?2", params![visit_id, log_id]).unwrap();

        let deleted = delete_unassigned_raw_log(&c, Source::Calendar, "e1").unwrap();
        assert!(!deleted, "an already-assigned raw log must not be deleted");
        let count: i64 = c.query_row("SELECT COUNT(*) FROM raw_logs", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn deleting_a_nonexistent_raw_log_is_not_an_error() {
        let c = open_in_memory().unwrap();
        assert!(!delete_unassigned_raw_log(&c, Source::Calendar, "missing").unwrap());
    }
```

- [ ] **Step 6: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-core --lib store::tests::deletes_an_unassigned_raw_log store::tests::leaves_an_already_assigned_raw_log_untouched store::tests::deleting_a_nonexistent_raw_log_is_not_an_error`
Expected: FAIL(`delete_unassigned_raw_log` が存在しない)

- [ ] **Step 7: `delete_unassigned_raw_log` を実装する**

`crates/areitu-core/src/store.rs` の `upsert_raw_log` の直後に追加:

```rust
/// 対象の raw_log がまだ visit に割り当てられていない場合だけ削除する。
/// 既に visit に組み込まれている raw_log は、後からカレンダー側でキャンセルされても
/// 過去に確定した訪問履歴を壊さないよう、削除しない。
pub fn delete_unassigned_raw_log(conn: &Connection, source: Source, source_id: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM raw_logs WHERE source = ?1 AND source_id = ?2 AND visit_id IS NULL",
        params![source.as_str(), source_id],
    )?;
    Ok(changed > 0)
}
```

- [ ] **Step 8: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-core --lib store::`
Expected: PASS(既存テストも含め全て)

- [ ] **Step 9: コミット**

```bash
git add crates/areitu-core/src/calendar.rs crates/areitu-core/src/store.rs
git commit -m "$(cat <<'EOF'
feat(areitu-core): extract cancelled calendar events and let sync delete them

parse_cancelled_source_ids surfaces the ids Google Calendar's
incremental sync reports as status=cancelled. delete_unassigned_raw_log
lets a caller remove the matching raw_log, but only while it is still
unassigned, so an already-resolved visit is never retroactively erased.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: デスクトップ — カレンダー取り込みのコアロジック（`ingest_calendar_page_bodies`）

**Files:**
- Modify: `apps/desktop/src-tauri/src/google.rs`

**Interfaces:**
- Consumes: `areitu_google::calendar::{CalendarApi, EventsListParams, EventsPage, fetch_all_pages}`(Task 4)、`areitu_google::state::CalendarSyncState`(Task 3)、`areitu_core::calendar::{parse_events, parse_cancelled_source_ids, to_raw_log}`(既存＋Task 5)、`areitu_core::store::{upsert_raw_log, delete_unassigned_raw_log}`(既存＋Task 5)
- Produces: `pub struct CalendarIngestSummary { pub events_synced: usize, pub events_removed: usize, pub errors: Vec<String> }`、`pub fn ingest_calendar_page_bodies(conn: &Connection, api: &dyn CalendarApi, access_token: &str, calendar_id: &str, state: &mut CalendarSyncState) -> CalendarIngestSummary`。Task 7 の `ingest_calendar`（実ネットワーク・認証を束ねる層）がこれを呼ぶ

このタスクは純粋ロジックだけを実ネットワークなしでテストする。実際の `GoogleAuth`/`build_auth()` を絡めた結線は Task 7 で行う。

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src-tauri/src/google.rs` の末尾（既存の `#[cfg(test)] mod tests` の外、ファイル末尾）に新しいテストモジュールを追加する:

```rust
#[cfg(test)]
mod ingest_calendar_page_bodies_tests {
    use super::*;
    use areitu_google::calendar::{CalendarApi, EventsListParams, EventsPage};
    use areitu_google::state::CalendarSyncState;
    use std::sync::Mutex;

    struct FakeCalendarApi {
        pages: Mutex<Vec<areitu_google::Result<EventsPage>>>,
        seen_sync_tokens: Mutex<Vec<Option<String>>>,
    }

    impl FakeCalendarApi {
        fn new(pages: Vec<areitu_google::Result<EventsPage>>) -> Self {
            FakeCalendarApi { pages: Mutex::new(pages), seen_sync_tokens: Mutex::new(Vec::new()) }
        }
    }

    impl CalendarApi for FakeCalendarApi {
        fn list_events_page(&self, _access_token: &str, params: &EventsListParams) -> areitu_google::Result<EventsPage> {
            self.seen_sync_tokens.lock().unwrap().push(params.sync_token.map(str::to_owned));
            let mut pages = self.pages.lock().unwrap();
            assert!(!pages.is_empty(), "FakeCalendarApi called more times than pages were queued");
            pages.remove(0)
        }
    }

    fn events_json(id: &str, status: &str) -> String {
        format!(
            r#"{{"items":[{{"id":"{id}","status":"{status}","summary":"ランチ","start":{{"dateTime":"2026-09-01T12:00:00+09:00"}},"end":{{"dateTime":"2026-09-01T13:00:00+09:00"}}}}]}}"#
        )
    }

    fn calendar_row_count(c: &rusqlite::Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'calendar'", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn upserts_confirmed_events_and_advances_sync_token() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let api = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "confirmed"),
            next_page_token: None,
            next_sync_token: Some("token-1".to_owned()),
        })]);
        let mut state = CalendarSyncState::default();
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 1);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(state.sync_token.as_deref(), Some("token-1"));
        assert_eq!(calendar_row_count(&c), 1);
    }

    #[test]
    fn cancelled_event_removes_unassigned_raw_log() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let mut state = CalendarSyncState::default();
        let api1 = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "confirmed"),
            next_page_token: None,
            next_sync_token: Some("token-1".to_owned()),
        })]);
        ingest_calendar_page_bodies(&c, &api1, "access-token", "primary", &mut state);
        assert_eq!(calendar_row_count(&c), 1);

        let api2 = FakeCalendarApi::new(vec![Ok(EventsPage {
            body: events_json("e1", "cancelled"),
            next_page_token: None,
            next_sync_token: Some("token-2".to_owned()),
        })]);
        let summary = ingest_calendar_page_bodies(&c, &api2, "access-token", "primary", &mut state);
        assert_eq!(summary.events_removed, 1);
        assert_eq!(calendar_row_count(&c), 0);
    }

    #[test]
    fn sync_token_expired_clears_token_and_retries_full_sync_once() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let mut state = CalendarSyncState { sync_token: Some("stale-token".to_owned()) };
        let api = FakeCalendarApi::new(vec![
            Err(areitu_google::Error::SyncTokenExpired),
            Ok(EventsPage {
                body: events_json("e1", "confirmed"),
                next_page_token: None,
                next_sync_token: Some("fresh-token".to_owned()),
            }),
        ]);
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 1);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(state.sync_token.as_deref(), Some("fresh-token"));
        let seen = api.seen_sync_tokens.lock().unwrap();
        assert_eq!(seen.len(), 2, "expected one failed attempt with the stale token and one full-sync retry");
        assert_eq!(seen[0].as_deref(), Some("stale-token"));
        assert_eq!(seen[1], None, "the retry after a 410 must not resend the stale sync token");
    }

    #[test]
    fn pagination_across_two_pages_syncs_both_events() {
        let c = areitu_core::db::open_in_memory().unwrap();
        let api = FakeCalendarApi::new(vec![
            Ok(EventsPage { body: events_json("e1", "confirmed"), next_page_token: Some("p2".to_owned()), next_sync_token: None }),
            Ok(EventsPage { body: events_json("e2", "confirmed"), next_page_token: None, next_sync_token: Some("final-token".to_owned()) }),
        ]);
        let mut state = CalendarSyncState::default();
        let summary = ingest_calendar_page_bodies(&c, &api, "access-token", "primary", &mut state);
        assert_eq!(summary.events_synced, 2);
        assert_eq!(state.sync_token.as_deref(), Some("final-token"));
    }
}
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-desktop --lib ingest_calendar_page_bodies_tests::`
Expected: FAIL（`CalendarIngestSummary`/`ingest_calendar_page_bodies` が存在しない）

- [ ] **Step 3: `CalendarIngestSummary` と `ingest_calendar_page_bodies` を実装する**

`apps/desktop/src-tauri/src/google.rs` の先頭の `use` 群に追加:

```rust
use rusqlite::Connection;
```

（既に `use rusqlite::Connection;` がある場合は追加しない。既存の `use` 文を確認して重複を避ける。）

`pub fn drive_sync_now` の直後、テストモジュールの手前に追加:

```rust
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct CalendarIngestSummary {
    pub events_synced: usize,
    pub events_removed: usize,
    pub errors: Vec<String>,
}

/// カレンダーの1同期サイクル分のページ本文を raw_logs に反映する純粋なロジック。
/// Google 認証・アクセストークン取得・状態ファイルの読み書きは呼び出し元（`ingest_calendar`）
/// の責務とし、ここでは渡された `api`/`access_token`/`state` だけを使う。
pub fn ingest_calendar_page_bodies(
    conn: &Connection,
    api: &dyn areitu_google::calendar::CalendarApi,
    access_token: &str,
    calendar_id: &str,
    state: &mut areitu_google::state::CalendarSyncState,
) -> CalendarIngestSummary {
    let mut summary = CalendarIngestSummary::default();
    let fetched = match areitu_google::calendar::fetch_all_pages(api, access_token, calendar_id, state.sync_token.as_deref()) {
        Ok(f) => f,
        Err(areitu_google::Error::SyncTokenExpired) => {
            state.sync_token = None;
            match areitu_google::calendar::fetch_all_pages(api, access_token, calendar_id, None) {
                Ok(f) => f,
                Err(e) => {
                    summary.errors.push(e.to_string());
                    return summary;
                }
            }
        }
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };

    for body in &fetched.bodies {
        match areitu_core::calendar::parse_events(body) {
            Ok(events) => {
                for e in &events {
                    match areitu_core::store::upsert_raw_log(conn, &areitu_core::calendar::to_raw_log(e)) {
                        Ok(()) => summary.events_synced += 1,
                        Err(err) => summary.errors.push(err.to_string()),
                    }
                }
            }
            Err(e) => summary.errors.push(e.to_string()),
        }
        match areitu_core::calendar::parse_cancelled_source_ids(body) {
            Ok(ids) => {
                for id in ids {
                    match areitu_core::store::delete_unassigned_raw_log(conn, areitu_core::model::Source::Calendar, &id) {
                        Ok(true) => summary.events_removed += 1,
                        Ok(false) => {}
                        Err(err) => summary.errors.push(err.to_string()),
                    }
                }
            }
            Err(e) => summary.errors.push(e.to_string()),
        }
    }

    if fetched.next_sync_token.is_some() {
        state.sync_token = fetched.next_sync_token;
    }
    summary
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib ingest_calendar_page_bodies_tests::`
Expected: PASS（4テスト）

- [ ] **Step 5: コミット**

```bash
git add apps/desktop/src-tauri/src/google.rs
git commit -m "$(cat <<'EOF'
feat(desktop): add pure calendar-page-to-raw_logs ingestion logic

ingest_calendar_page_bodies applies fetched Calendar API pages to
raw_logs (upsert confirmed events, delete cancelled-but-unassigned
ones) and retries once with a full sync on a stale sync token. It
takes a CalendarApi + access token directly so it can be unit tested
with a fake, with no real auth or network involved.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: デスクトップ — `config.rs`（カレンダー有効フラグ・`SecretStore::get` の tri-state 化）

**Files:**
- Modify: `apps/desktop/src-tauri/src/config.rs`
- Modify: `apps/desktop/src-tauri/src/sync.rs`（`build_geocoder`/`build_llm` の `secrets.get` 呼び出し箇所のみ）

**Interfaces:**
- Consumes: なし
- Produces: `AppConfig.calendar_enabled: bool`（デフォルト `false`）、`SecretStore::get(&self, key: &str) -> Result<Option<String>, String>`（従来の `Option<String>` から変更）、`pub enum KeyStatus { Set, NotSet, Unavailable }`（`#[serde(rename_all = "snake_case")]`）、`pub fn config_exists(path: &Path) -> bool`。Task 9・Task 10 がこれを使う

`SecretStore::get` の変更理由: 従来は `KeyringSecretStore::get` がキーチェーンのどんなエラー（未設定・ロック中・アクセス不可）も `None` に潰しており、設定画面はロック中のキーチェーンを「未設定」と誤表示していた。`Ok(None)`（未設定）と `Err(_)`（読めない）を区別できるようにする。

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src-tauri/src/config.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn calendar_enabled_defaults_to_false() {
        assert!(!AppConfig::default().calendar_enabled);
    }

    #[test]
    fn calendar_enabled_round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut config = AppConfig::default();
        config.calendar_enabled = true;
        save_config(&path, &config).unwrap();
        assert!(load_config(&path).calendar_enabled);
    }

    #[test]
    fn fake_secret_store_get_distinguishes_not_set_from_unavailable() {
        let store = FakeSecretStore::new();
        assert_eq!(store.get(OPENAI_KEY).unwrap(), None);
        store.set(OPENAI_KEY, "sk-test").unwrap();
        assert_eq!(store.get(OPENAI_KEY).unwrap().as_deref(), Some("sk-test"));
        let locked = AlwaysUnavailableSecretStore;
        assert!(locked.get(OPENAI_KEY).is_err());
    }

    #[test]
    fn config_exists_reflects_whether_the_file_has_been_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert!(!config_exists(&path));
        save_config(&path, &AppConfig::default()).unwrap();
        assert!(config_exists(&path));
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-desktop --lib config::`
Expected: FAIL（`calendar_enabled` フィールドがない・`AlwaysUnavailableSecretStore` がない・`config_exists` がない・`get` の戻り値の型が合わない）

- [ ] **Step 3: `AppConfig`・`SecretStore`・`KeyStatus`・`config_exists` を実装する**

`apps/desktop/src-tauri/src/config.rs` の `AppConfig` struct とその `Default` 実装を次に置き換える:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub watched_dirs: Vec<String>,
    pub llm_provider: LlmProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
    pub gemini_model: String,
    pub google_places_enabled: bool,
    pub min_confidence: f64,
    pub poll_interval_minutes: u32,
    pub calendar_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            watched_dirs: Vec::new(),
            llm_provider: LlmProvider::None,
            ollama_url: "http://localhost:11434".to_owned(),
            ollama_model: String::new(),
            openai_model: "gpt-4o-mini".to_owned(),
            gemini_model: "gemini-1.5-flash".to_owned(),
            google_places_enabled: false,
            min_confidence: 0.6,
            poll_interval_minutes: 30,
            calendar_enabled: false,
        }
    }
}
```

`load_config`/`save_config` の直後に追加:

```rust
pub fn config_exists(path: &Path) -> bool {
    path.exists()
}
```

`SecretStore` トレイトと `KeyringSecretStore` を次に置き換える:

```rust
pub trait SecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    Set,
    NotSet,
    Unavailable,
}

pub fn key_status(secrets: &dyn SecretStore, key: &str) -> KeyStatus {
    match secrets.get(key) {
        Ok(Some(_)) => KeyStatus::Set,
        Ok(None) => KeyStatus::NotSet,
        Err(_) => KeyStatus::Unavailable,
    }
}

pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, key)
            .map_err(|e| e.to_string())?
            .set_password(value)
            .map_err(|e| e.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, key) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    }
}
```

`FakeSecretStore` の `impl SecretStore for FakeSecretStore` を次に置き換え、`AlwaysUnavailableSecretStore` を追加する:

```rust
#[cfg(test)]
impl SecretStore for FakeSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// キーチェーンがロック中・アクセス不可の状態を模す test-only 実装。
#[cfg(test)]
pub struct AlwaysUnavailableSecretStore;

#[cfg(test)]
impl SecretStore for AlwaysUnavailableSecretStore {
    fn get(&self, _key: &str) -> Result<Option<String>, String> {
        Err("keychain is locked".to_owned())
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Err("keychain is locked".to_owned())
    }

    fn delete(&self, _key: &str) -> Result<(), String> {
        Err("keychain is locked".to_owned())
    }
}
```

- [ ] **Step 4: `sync.rs` の呼び出し箇所を新しい `get` の戻り値に合わせる**

`apps/desktop/src-tauri/src/sync.rs` の `build_geocoder` 内:

```rust
        if let Some(key) = secrets.get(GOOGLE_PLACES_KEY) {
```

を次に変更する:

```rust
        if let Ok(Some(key)) = secrets.get(GOOGLE_PLACES_KEY) {
```

`build_llm` 内の3箇所（`Ollama` 以外の `OpenAi`/`Gemini` 分岐）:

```rust
        LlmProvider::OpenAi => secrets
            .get(OPENAI_KEY)
            .and_then(|key| OpenAi::new(&key, &config.openai_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
        LlmProvider::Gemini => secrets
            .get(GEMINI_KEY)
            .and_then(|key| Gemini::new(&key, &config.gemini_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
```

を次に変更する:

```rust
        LlmProvider::OpenAi => secrets
            .get(OPENAI_KEY)
            .ok()
            .flatten()
            .and_then(|key| OpenAi::new(&key, &config.openai_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
        LlmProvider::Gemini => secrets
            .get(GEMINI_KEY)
            .ok()
            .flatten()
            .and_then(|key| Gemini::new(&key, &config.gemini_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
```

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib config:: sync::`
Expected: PASS（`config.rs`/`sync.rs` の既存テストと Step 1 の新規テストがすべて通る。`openai_provider_without_api_key_degrades_to_nominatim_only` 等、`get` の戻り値変更の影響を受ける既存テストも引き続き PASS することを確認する）

- [ ] **Step 6: コミット**

```bash
git add apps/desktop/src-tauri/src/config.rs apps/desktop/src-tauri/src/sync.rs
git commit -m "$(cat <<'EOF'
feat(desktop): distinguish a locked keychain from an unset key

SecretStore::get now returns Result<Option<String>, String> instead of
collapsing every keychain error into None, so the settings screen can
tell "not set" apart from "couldn't read the keychain right now"
instead of always claiming the key is missing. Also adds
AppConfig.calendar_enabled and config_exists for the upcoming calendar
sync and first-run onboarding work.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: デスクトップ — `google.rs`（`ingest_calendar` の実結線・incremental な `google_sign_in`）

**Files:**
- Modify: `apps/desktop/src-tauri/src/google.rs`

**Interfaces:**
- Consumes: `Task 6` の `ingest_calendar_page_bodies`/`CalendarIngestSummary`、`Task 7` の `AppConfig.calendar_enabled`、`areitu_google::{SCOPE_DRIVE_APPDATA, SCOPE_CALENDAR_READONLY}`、`areitu_google::calendar::CalendarClient`、`areitu_google::state::{load_calendar_state, save_calendar_state}`
- Produces: `pub fn calendar_scope_for(config: &AppConfig) -> String`、`pub fn ingest_calendar(conn: &Connection, config: &AppConfig, calendar_state_path: &Path) -> CalendarIngestSummary`、`google_sign_in` コマンドが `state: State<'_, AppState>` を取り `async fn` になる（呼び出し側のコマンド名・引数は変わらない）。Task 9 がこれを呼ぶ

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src-tauri/src/google.rs` の既存 `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn calendar_scope_is_drive_only_when_calendar_import_is_disabled() {
        let config = crate::config::AppConfig { calendar_enabled: false, ..crate::config::AppConfig::default() };
        assert_eq!(calendar_scope_for(&config), areitu_google::SCOPE_DRIVE_APPDATA);
    }

    #[test]
    fn calendar_scope_adds_calendar_readonly_when_enabled() {
        let config = crate::config::AppConfig { calendar_enabled: true, ..crate::config::AppConfig::default() };
        assert_eq!(
            calendar_scope_for(&config),
            format!("{} {}", areitu_google::SCOPE_DRIVE_APPDATA, areitu_google::SCOPE_CALENDAR_READONLY)
        );
    }

    #[test]
    fn ingest_calendar_is_a_silent_noop_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let c = areitu_core::db::open_in_memory().unwrap();
        let config = crate::config::AppConfig { calendar_enabled: false, ..crate::config::AppConfig::default() };
        let summary = ingest_calendar(&c, &config, &dir.path().join("google-calendar-state.json"));
        assert_eq!(summary, CalendarIngestSummary::default());
    }

    #[test]
    fn ingest_calendar_enabled_without_client_credentials_reports_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let c = areitu_core::db::open_in_memory().unwrap();
        let config = crate::config::AppConfig { calendar_enabled: true, ..crate::config::AppConfig::default() };
        // AREITU_GOOGLE_CLIENT_ID / AREITU_GOOGLE_CLIENT_SECRET はビルド時の
        // option_env! で埋め込まれるため、このテスト環境で未設定なら build_auth() が
        // 即座に失敗する（drive_sync_blocks_until_sync_lock_is_released と同じ前提）。
        let summary = ingest_calendar(&c, &config, &dir.path().join("google-calendar-state.json"));
        assert!(!summary.errors.is_empty());
        assert_eq!(summary.events_synced, 0);
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-desktop --lib google::tests::calendar_scope google::tests::ingest_calendar`
Expected: FAIL（`calendar_scope_for`/`ingest_calendar` が存在しない）

- [ ] **Step 3: `calendar_scope_for` と `ingest_calendar` を実装する**

`apps/desktop/src-tauri/src/google.rs` 先頭の `use` 群（既存の `use std::path::{Path, PathBuf};` の下）に1行追加する:

```rust
use crate::config::AppConfig;
```

`fn build_auth()` の直後に追加:

```rust
const CALENDAR_ID: &str = "primary";

pub fn calendar_scope_for(config: &AppConfig) -> String {
    if config.calendar_enabled {
        format!("{} {}", areitu_google::SCOPE_DRIVE_APPDATA, areitu_google::SCOPE_CALENDAR_READONLY)
    } else {
        areitu_google::SCOPE_DRIVE_APPDATA.to_owned()
    }
}

/// カレンダー取り込みの唯一の入口。`config.calendar_enabled` が false なら何もしない。
/// サインインしていない・クライアント資格情報が未設定などの理由でアクセストークンが
/// 取れない場合も、エラーを `summary.errors` に積んで返すだけで、呼び出し元の
/// 写真同期・visit 構築は止めない。
pub fn ingest_calendar(conn: &Connection, config: &AppConfig, calendar_state_path: &Path) -> CalendarIngestSummary {
    let mut summary = CalendarIngestSummary::default();
    if !config.calendar_enabled {
        return summary;
    }
    let auth = match build_auth() {
        Ok(a) => a,
        Err(e) => {
            summary.errors.push(e);
            return summary;
        }
    };
    let access_token = match auth.access_token() {
        Ok(t) => t,
        Err(e) => {
            summary.errors.push(format!("Google カレンダーに接続できません: {e}"));
            return summary;
        }
    };
    let client = match areitu_google::calendar::CalendarClient::new() {
        Ok(c) => c,
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };
    let mut state = match areitu_google::state::load_calendar_state(calendar_state_path) {
        Ok(s) => s,
        Err(e) => {
            summary.errors.push(e.to_string());
            return summary;
        }
    };

    summary = ingest_calendar_page_bodies(conn, &client, &access_token, CALENDAR_ID, &mut state);
    if let Err(e) = areitu_google::state::save_calendar_state(calendar_state_path, &state) {
        summary.errors.push(e.to_string());
    }
    summary
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib google::`
Expected: PASS（既存テスト・Step 1 の新規テスト・Task 6 の `ingest_calendar_page_bodies_tests` すべて）

- [ ] **Step 5: 失敗するテストを書く（`google_sign_in` の scope 反映を async 化しても壊さない）**

`google_sign_in` は実際にブラウザを開き Google と通信するため自動テストの対象にしない（既存の `sign_in` 自体は Task 2 で `areitu-google` 側にテスト済み）。このステップでは、代わりにコンパイルが通ること自体を確認の対象とする。既存の `#[cfg(test)] mod tests` に変更は不要。

- [ ] **Step 6: `google_sign_in` を `AppState` を読む非同期コマンドに変更する**

`apps/desktop/src-tauri/src/google.rs` の

```rust
#[tauri::command]
pub fn google_sign_in() -> Result<(), String> {
    build_auth()?.sign_in().map_err(|e| e.to_string())
}
```

を次に置き換える:

```rust
/// ブラウザでの認可完了までブロックするため、`spawn_blocking` で Tauri の
/// 非同期ランタイム上のワーカースレッドに逃がす。こうしないと呼び出し中
/// フロントエンドの他の `invoke` 呼び出しがすべて詰まってしまう。
#[tauri::command]
pub async fn google_sign_in(state: State<'_, crate::AppState>) -> Result<(), String> {
    let config = crate::config::load_config(&state.config_path);
    tauri::async_runtime::spawn_blocking(move || {
        let scope = calendar_scope_for(&config);
        build_auth()?.sign_in(&scope)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

`apps/desktop/src-tauri/src/google.rs` 先頭の `use tauri::{AppHandle, Manager};` を `use tauri::{AppHandle, Manager, State};` に変更する。

- [ ] **Step 7: ビルドを確認する**

Run: `cargo check -p areitu-desktop`
Expected: エラーなく終了する（この時点では `invoke_handler!` 側の登録は既存のままでも `async fn` は登録可能なので、Task 9 でのワイヤリング前でもコンパイルは通る）

- [ ] **Step 8: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib google::`
Expected: PASS

- [ ] **Step 9: コミット**

```bash
git add apps/desktop/src-tauri/src/google.rs
git commit -m "$(cat <<'EOF'
feat(desktop): wire real calendar ingestion and make sign-in incremental

ingest_calendar builds the real GoogleAuth/CalendarClient and delegates
to the already-tested ingest_calendar_page_bodies. google_sign_in now
computes its requested scope from the persisted calendar_enabled flag
and runs the blocking browser flow on spawn_blocking so IPC stays
responsive while the user completes consent.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: デスクトップ — `AppState.calendar_state_path` の配線と同期サイクルへの合流

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/sync.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`（`sync_now` コマンドの呼び出し引数のみ）
- Modify: `apps/desktop/src-tauri/src/tray.rs`（呼び出し引数のみ）

**Interfaces:**
- Consumes: Task 8 の `crate::google::ingest_calendar`
- Produces: `AppState.calendar_state_path: PathBuf`、`sync::run_sync_with_config(conn, calendar_state_path, config, secrets)`・`sync::sync_on_own_connection(db_path, calendar_state_path, config, secrets)`・`sync::sync_on_own_connection_locked(sync_lock, db_path, calendar_state_path, config, secrets)`（いずれも新しい `calendar_state_path: &Path` 引数を追加）、`SyncSummary` に `calendar_synced: usize`・`calendar_removed: usize`・`calendar_errors: Vec<String>` を追加

- [ ] **Step 1: 失敗するテストを書く（`SyncSummary` の camelCase・カレンダー結果の反映）**

`apps/desktop/src-tauri/src/sync.rs` の既存 `sync_summary_serializes_as_camel_case` テストを次に置き換える:

```rust
    #[test]
    fn sync_summary_serializes_as_camel_case() {
        let summary = SyncSummary {
            scanned: 1,
            scan_errors: vec!["boom".to_string()],
            visits_created: 2,
            resolve_failed: 3,
            calendar_synced: 4,
            calendar_removed: 5,
            calendar_errors: vec!["cal-boom".to_string()],
        };
        let json = serde_json::to_value(&summary).unwrap();
        let obj = json.as_object().unwrap();
        for key in ["scanErrors", "visitsCreated", "resolveFailed", "calendarSynced", "calendarRemoved", "calendarErrors"] {
            assert!(obj.contains_key(key), "missing {key}: {json}");
        }
        for key in ["scan_errors", "visits_created", "resolve_failed", "calendar_synced", "calendar_removed", "calendar_errors"] {
            assert!(!obj.contains_key(key), "snake_case leaked: {json}");
        }
    }
```

`apps/desktop/src-tauri/src/sync.rs` の `sync_on_own_connection_does_not_need_the_shared_lock` テストの

```rust
        let summary = sync_on_own_connection(&db_path, &config, &secrets);
```

を次に変更する:

```rust
        let summary = sync_on_own_connection(&db_path, &dir.path().join("google-calendar-state.json"), &config, &secrets);
```

同ファイルの `openai_provider_without_api_key_degrades_to_nominatim_only` テストの

```rust
        let summary = run_sync_with_config(&mut c, &config, &secrets);
```

を次に変更する:

```rust
        let dir = tempfile::tempdir().unwrap();
        let summary = run_sync_with_config(&mut c, &dir.path().join("google-calendar-state.json"), &config, &secrets);
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-desktop --lib sync::`
Expected: FAIL（`SyncSummary` にカレンダー系フィールドがない・関数の引数個数が合わない）

- [ ] **Step 3: `SyncSummary`・`run_sync_with_config`・`sync_on_own_connection`・`sync_on_own_connection_locked` を変更する**

`apps/desktop/src-tauri/src/sync.rs` の `SyncSummary` を次に置き換える:

```rust
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub scanned: usize,
    pub scan_errors: Vec<String>,
    pub visits_created: usize,
    pub resolve_failed: usize,
    pub calendar_synced: usize,
    pub calendar_removed: usize,
    pub calendar_errors: Vec<String>,
}
```

`run_sync_with_config` を次に置き換える:

```rust
pub fn run_sync_with_config(
    conn: &mut Connection,
    calendar_state_path: &Path,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let calendar_summary = crate::google::ingest_calendar(conn, config, calendar_state_path);
    let geocoder = build_geocoder(config, secrets);
    let llm = build_llm(config, secrets);
    let mut summary = run_sync(conn, &config.watched_dirs, geocoder.as_ref(), llm.as_deref(), config.min_confidence)?;
    summary.calendar_synced = calendar_summary.events_synced;
    summary.calendar_removed = calendar_summary.events_removed;
    summary.calendar_errors = calendar_summary.errors;
    Ok(summary)
}
```

`sync_on_own_connection`/`sync_on_own_connection_locked` を次に置き換える:

```rust
pub fn sync_on_own_connection(
    db_path: &Path,
    calendar_state_path: &Path,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let mut conn = areitu_core::db::open(db_path).map_err(|e| e.to_string())?;
    run_sync_with_config(&mut conn, calendar_state_path, config, secrets)
}

pub fn sync_on_own_connection_locked(
    sync_lock: &Mutex<()>,
    db_path: &Path,
    calendar_state_path: &Path,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let _guard = sync_lock.lock().map_err(|e| e.to_string())?;
    sync_on_own_connection(db_path, calendar_state_path, config, secrets)
}
```

`spawn_poll_thread` 内の呼び出しを次に変更する:

```rust
            let _ = sync_on_own_connection_locked(
                &state.sync_lock,
                &state.db_path,
                &state.calendar_state_path,
                &config,
                &crate::config::KeyringSecretStore,
            );
```

（この時点で `drive_sync_locked` の呼び出しはそのまま変更しない。）

- [ ] **Step 4: `AppState` に `calendar_state_path` を追加する**

`apps/desktop/src-tauri/src/lib.rs` の `AppState` struct に追加:

```rust
pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub config_path: std::path::PathBuf,
    pub db_path: std::path::PathBuf,
    pub calendar_state_path: std::path::PathBuf,
    pub sync_lock: Mutex<()>,
}
```

`setup(|app| { ... })` 内、`let config_path = ...;` の下に追加:

```rust
            let calendar_state_path = data_dir.join("google-calendar-state.json");
```

`app.manage(AppState { ... })` の呼び出しに `calendar_state_path,` を追加する:

```rust
            app.manage(AppState {
                conn: Mutex::new(conn),
                config_path,
                db_path,
                calendar_state_path,
                sync_lock: Mutex::new(()),
            });
```

- [ ] **Step 5: `commands.rs`・`tray.rs` の呼び出し引数を更新する**

`apps/desktop/src-tauri/src/commands.rs` の `sync_now` コマンド内:

```rust
    sync_on_own_connection_locked(&state.sync_lock, &state.db_path, &config, &KeyringSecretStore)
```

を次に変更する:

```rust
    sync_on_own_connection_locked(&state.sync_lock, &state.db_path, &state.calendar_state_path, &config, &KeyringSecretStore)
```

`apps/desktop/src-tauri/src/tray.rs` の `"sync_now"` ハンドラ内:

```rust
                    let _ = crate::sync::sync_on_own_connection_locked(
                        &state.sync_lock,
                        &state.db_path,
                        &config,
                        &crate::config::KeyringSecretStore,
                    );
```

を次に変更する:

```rust
                    let _ = crate::sync::sync_on_own_connection_locked(
                        &state.sync_lock,
                        &state.db_path,
                        &state.calendar_state_path,
                        &config,
                        &crate::config::KeyringSecretStore,
                    );
```

- [ ] **Step 6: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib`
Expected: PASS（`sync.rs`・`commands.rs`・`google.rs` の全テスト。既存の `empty_dirs_list_still_builds_visits_from_calendar_ingest` は `run_sync` 直呼び出しのため引数変更の影響を受けない）

- [ ] **Step 7: ビルド全体を確認する**

Run: `cargo build -p areitu-desktop`
Expected: エラーなく終了する

- [ ] **Step 8: コミット**

```bash
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/sync.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/tray.rs
git commit -m "$(cat <<'EOF'
feat(desktop): fold calendar ingestion into the existing sync cycle

AppState now carries calendar_state_path alongside db_path/config_path,
and every call site that drives a photo/visit sync (the poll thread,
the tray's "sync now", and the sync_now command) also runs calendar
ingestion under the same sync_lock and its own dedicated connection.
SyncSummary gains calendar_synced/calendar_removed/calendar_errors so
the UI can surface calendar sync results alongside photo scan results.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: デスクトップ — `commands.rs`（`SettingsDto`/`SaveSettingsDto` の拡張、`setup_completed`）

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: Task 7 の `AppConfig.calendar_enabled`・`config::{KeyStatus, key_status, config_exists}`
- Produces: `SettingsDto`/`SaveSettingsDto` の `calendarEnabled` フィールドと、`hasOpenaiKey`/`hasGeminiKey`/`hasGooglePlacesKey`（bool）を置き換える `openaiKeyStatus`/`geminiKeyStatus`/`googlePlacesKeyStatus`（`"set" | "not_set" | "unavailable"`）。`setup_completed(state) -> bool`。`import_timeline_file` コマンドは Task 14（`areitu-core::timeline` 実装後）で追加する

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src-tauri/src/commands.rs` の `settings_dto_serializes_as_camel_case` テストを次に置き換える:

```rust
    #[test]
    fn settings_dto_serializes_as_camel_case() {
        let dto = SettingsDto {
            watched_dirs: vec!["/photos".to_string()],
            llm_provider: LlmProvider::Ollama,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "llama3".to_string(),
            openai_model: "gpt-4o-mini".to_string(),
            gemini_model: "gemini-1.5-flash".to_string(),
            google_places_enabled: false,
            min_confidence: 0.6,
            poll_interval_minutes: 30,
            calendar_enabled: false,
            openai_key_status: KeyStatus::NotSet,
            gemini_key_status: KeyStatus::NotSet,
            google_places_key_status: KeyStatus::Unavailable,
        };
        let json = serde_json::to_value(&dto).unwrap();
        let obj = json.as_object().unwrap();
        for key in [
            "watchedDirs",
            "llmProvider",
            "ollamaUrl",
            "ollamaModel",
            "openaiModel",
            "geminiModel",
            "googlePlacesEnabled",
            "minConfidence",
            "pollIntervalMinutes",
            "calendarEnabled",
            "openaiKeyStatus",
            "geminiKeyStatus",
            "googlePlacesKeyStatus",
        ] {
            assert!(obj.contains_key(key), "missing {key}: {json}");
        }
        assert!(!obj.contains_key("watched_dirs"), "snake_case leaked: {json}");
        assert_eq!(obj.get("googlePlacesKeyStatus").unwrap(), "unavailable");
    }

    #[test]
    fn save_settings_dto_deserializes_camel_case_payload() {
        let payload = serde_json::json!({
            "watchedDirs": ["/photos"],
            "llmProvider": "openai",
            "ollamaUrl": "http://localhost:11434",
            "ollamaModel": "",
            "openaiModel": "gpt-4o-mini",
            "geminiModel": "gemini-1.5-flash",
            "googlePlacesEnabled": true,
            "minConfidence": 0.6,
            "pollIntervalMinutes": 30,
            "calendarEnabled": true,
            "openaiApiKey": "sk-test",
            "geminiApiKey": null,
            "googlePlacesApiKey": null,
        });
        let dto: SaveSettingsDto = serde_json::from_value(payload).unwrap();
        assert_eq!(dto.watched_dirs, vec!["/photos".to_string()]);
        assert_eq!(dto.llm_provider, LlmProvider::OpenAi);
        assert!(dto.google_places_enabled);
        assert!(dto.calendar_enabled);
        assert_eq!(dto.openai_api_key.as_deref(), Some("sk-test"));
        assert_eq!(dto.gemini_api_key, None);
    }

    #[test]
    fn setup_completed_reflects_whether_config_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!crate::config::config_exists(&dir.path().join("config.json")));
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-desktop --lib commands::tests::settings_dto_serializes_as_camel_case commands::tests::save_settings_dto_deserializes_camel_case_payload`
Expected: FAIL（新フィールドがまだ存在しない）

- [ ] **Step 3: `SettingsDto`/`SaveSettingsDto`/`get_settings`/`save_settings` を変更し、`setup_completed`/`import_timeline_file` を追加する**

`apps/desktop/src-tauri/src/commands.rs` 先頭の `use` を次に置き換える:

```rust
use tauri::State;

use crate::config::{
    config_exists, key_status, load_config, save_config, AppConfig, KeyStatus, KeyringSecretStore, LlmProvider,
    SecretStore, GEMINI_KEY, GOOGLE_PLACES_KEY, OPENAI_KEY,
};
use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
use crate::sync::{sync_on_own_connection_locked, SyncSummary};
use crate::AppState;
```

`sync_now` コマンドは Task 9 で既に `state.calendar_state_path` を渡すよう変更済み。`SettingsDto`/`SaveSettingsDto`/`get_settings`/`save_settings` を次に置き換える:

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub watched_dirs: Vec<String>,
    pub llm_provider: LlmProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
    pub gemini_model: String,
    pub google_places_enabled: bool,
    pub min_confidence: f64,
    pub poll_interval_minutes: u32,
    pub calendar_enabled: bool,
    pub openai_key_status: KeyStatus,
    pub gemini_key_status: KeyStatus,
    pub google_places_key_status: KeyStatus,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsDto {
    pub watched_dirs: Vec<String>,
    pub llm_provider: LlmProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
    pub gemini_model: String,
    pub google_places_enabled: bool,
    pub min_confidence: f64,
    pub poll_interval_minutes: u32,
    pub calendar_enabled: bool,
    /// None なら変更しない。Some("") ならキーチェーンから削除する。Some(value) (非空) なら保存する。
    pub openai_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub google_places_api_key: Option<String>,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> SettingsDto {
    let config = load_config(&state.config_path);
    let secrets = KeyringSecretStore;
    SettingsDto {
        watched_dirs: config.watched_dirs,
        llm_provider: config.llm_provider,
        ollama_url: config.ollama_url,
        ollama_model: config.ollama_model,
        openai_model: config.openai_model,
        gemini_model: config.gemini_model,
        google_places_enabled: config.google_places_enabled,
        min_confidence: config.min_confidence,
        poll_interval_minutes: config.poll_interval_minutes,
        calendar_enabled: config.calendar_enabled,
        openai_key_status: key_status(&secrets, OPENAI_KEY),
        gemini_key_status: key_status(&secrets, GEMINI_KEY),
        google_places_key_status: key_status(&secrets, GOOGLE_PLACES_KEY),
    }
}

fn apply_secret(secrets: &dyn SecretStore, key: &str, value: Option<String>) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(v) if v.is_empty() => secrets.delete(key),
        Some(v) => secrets.set(key, &v),
    }
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: SaveSettingsDto) -> Result<(), String> {
    let config = AppConfig {
        watched_dirs: settings.watched_dirs,
        llm_provider: settings.llm_provider,
        ollama_url: settings.ollama_url,
        ollama_model: settings.ollama_model,
        openai_model: settings.openai_model,
        gemini_model: settings.gemini_model,
        google_places_enabled: settings.google_places_enabled,
        min_confidence: settings.min_confidence,
        poll_interval_minutes: settings.poll_interval_minutes,
        calendar_enabled: settings.calendar_enabled,
    };
    let secrets = KeyringSecretStore;
    apply_secret(&secrets, OPENAI_KEY, settings.openai_api_key)?;
    apply_secret(&secrets, GEMINI_KEY, settings.gemini_api_key)?;
    apply_secret(&secrets, GOOGLE_PLACES_KEY, settings.google_places_api_key)?;
    save_config(&state.config_path, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn setup_completed(state: State<AppState>) -> bool {
    config_exists(&state.config_path)
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-desktop --lib commands::`
Expected: PASS

- [ ] **Step 5: `lib.rs` の `invoke_handler!` に新コマンドを登録する**

`apps/desktop/src-tauri/src/lib.rs` の `invoke_handler![...]` に `commands::setup_completed,` を追加する:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
            commands::sync_now,
            commands::get_settings,
            commands::save_settings,
            commands::setup_completed,
            google::google_sign_in,
            google::google_sign_out,
            google::google_status,
            google::drive_sync_now,
        ])
```

（`import_timeline_file` はまだ登録しない。Task 14 で `areitu_core::timeline` の実装と一緒に追加する。）

- [ ] **Step 6: ビルドを確認する**

Run: `cargo build -p areitu-desktop`
Expected: エラーなく終了する

- [ ] **Step 7: コミット**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(desktop): expose calendar toggle, key status and setup_completed

SettingsDto/SaveSettingsDto gain calendarEnabled, and the three
has*Key booleans are replaced with *KeyStatus ("set"/"not_set"/
"unavailable") so the settings screen can tell a locked keychain apart
from a genuinely unset key. Adds setup_completed, which the frontend
uses to decide whether to show first-run onboarding.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: フロントエンド — API レイヤー（Google アカウント・設定拡張・セットアップ判定）

**Files:**
- Modify: `apps/desktop/src/api/types.ts`
- Modify: `apps/desktop/src/api/tauri.ts`
- Create: `apps/desktop/src/api/tauri.test.ts`
- Modify: `apps/desktop/src/screens/SettingsScreen.tsx`（型追従のみ。UI 作り込みは Task 12）

**Interfaces:**
- Consumes: 既存の `google_sign_in`/`google_sign_out`/`google_status`/`drive_sync_now` コマンド、Task 10 の `get_settings`/`save_settings`/`setup_completed` コマンド
- Produces: `GoogleStatus`、`KeyStatus`、拡張された `Settings`/`SaveSettingsInput`、`googleSignIn`/`googleSignOut`/`googleStatus`/`driveSyncNow`/`setupCompleted` 関数。Task 12・Task 16 がこれらを使う

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src/api/tauri.test.ts` を新規作成する:

```ts
import { describe, expect, it, vi } from "vitest";
import * as core from "@tauri-apps/api/core";
import {
  driveSyncNow,
  getSettings,
  googleSignIn,
  googleSignOut,
  googleStatus,
  saveSettings,
  setupCompleted,
} from "./tauri";
import type { Settings } from "./types";

describe("tauri api layer", () => {
  it("googleSignIn invokes google_sign_in with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    await googleSignIn();
    expect(spy).toHaveBeenCalledWith("google_sign_in");
  });

  it("googleSignOut invokes google_sign_out with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    await googleSignOut();
    expect(spy).toHaveBeenCalledWith("google_sign_out");
  });

  it("googleStatus invokes google_status and returns its result", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue("signed_in");
    await expect(googleStatus()).resolves.toBe("signed_in");
    expect(spy).toHaveBeenCalledWith("google_status");
  });

  it("driveSyncNow invokes drive_sync_now with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue("uploaded");
    await expect(driveSyncNow()).resolves.toBe("uploaded");
    expect(spy).toHaveBeenCalledWith("drive_sync_now");
  });

  it("setupCompleted invokes setup_completed with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(true);
    await expect(setupCompleted()).resolves.toBe(true);
    expect(spy).toHaveBeenCalledWith("setup_completed");
  });

  it("getSettings invokes get_settings with no arguments", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue({
      watchedDirs: [],
      llmProvider: "none",
      ollamaUrl: "http://localhost:11434",
      ollamaModel: "",
      openaiModel: "gpt-4o-mini",
      geminiModel: "gemini-1.5-flash",
      googlePlacesEnabled: false,
      minConfidence: 0.6,
      pollIntervalMinutes: 30,
      calendarEnabled: false,
      openaiKeyStatus: "not_set",
      geminiKeyStatus: "not_set",
      googlePlacesKeyStatus: "not_set",
    });
    await getSettings();
    expect(spy).toHaveBeenCalledWith("get_settings");
  });

  it("saveSettings invokes save_settings with the settings object under the settings key", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(undefined);
    const input: Settings & { openaiApiKey: string | null; geminiApiKey: string | null; googlePlacesApiKey: string | null } = {
      watchedDirs: ["/photos"],
      llmProvider: "none",
      ollamaUrl: "http://localhost:11434",
      ollamaModel: "",
      openaiModel: "gpt-4o-mini",
      geminiModel: "gemini-1.5-flash",
      googlePlacesEnabled: false,
      minConfidence: 0.6,
      pollIntervalMinutes: 30,
      calendarEnabled: true,
      openaiKeyStatus: "not_set",
      geminiKeyStatus: "not_set",
      googlePlacesKeyStatus: "not_set",
      openaiApiKey: null,
      geminiApiKey: null,
      googlePlacesApiKey: null,
    };
    await saveSettings(input);
    expect(spy).toHaveBeenCalledWith("save_settings", { settings: input });
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- tauri.test.ts`
Expected: FAIL（`googleSignIn`/`googleSignOut`/`googleStatus`/`driveSyncNow`/`setupCompleted` が存在しない、`Settings` に `calendarEnabled`/`*KeyStatus` がない）

- [ ] **Step 3: 型を拡張する**

`apps/desktop/src/api/types.ts` の `Settings`/`SaveSettingsInput` を次に置き換え、`GoogleStatus`/`KeyStatus`/`DriveSyncOutcome` を追加する:

```ts
export type KeyStatus = "set" | "not_set" | "unavailable";

export interface Settings {
  watchedDirs: string[];
  llmProvider: LlmProvider;
  ollamaUrl: string;
  ollamaModel: string;
  openaiModel: string;
  geminiModel: string;
  googlePlacesEnabled: boolean;
  minConfidence: number;
  pollIntervalMinutes: number;
  calendarEnabled: boolean;
  openaiKeyStatus: KeyStatus;
  geminiKeyStatus: KeyStatus;
  googlePlacesKeyStatus: KeyStatus;
}

export interface SaveSettingsInput {
  watchedDirs: string[];
  llmProvider: LlmProvider;
  ollamaUrl: string;
  ollamaModel: string;
  openaiModel: string;
  geminiModel: string;
  googlePlacesEnabled: boolean;
  minConfidence: number;
  pollIntervalMinutes: number;
  calendarEnabled: boolean;
  openaiApiKey: string | null;
  geminiApiKey: string | null;
  googlePlacesApiKey: string | null;
}

export type GoogleStatus = "signed_in" | "signed_out";
export type DriveSyncOutcome = "no_op" | "uploaded" | "downloaded" | string;
```

（`DriveSyncOutcome` は `conflict:<name>` という可変の文字列も返るため、`string` を許容する union にしている。既存の `LlmProvider`/`Place`/`Visit`/`SortMode` の定義はそのまま残す。`SyncSummary` は Task 9 で追加されたカレンダー系フィールドに合わせて次に置き換える:）

```ts
export interface SyncSummary {
  scanned: number;
  scanErrors: string[];
  visitsCreated: number;
  resolveFailed: number;
  calendarSynced: number;
  calendarRemoved: number;
  calendarErrors: string[];
}
```

- [ ] **Step 4: `api/tauri.ts` に関数を追加する**

`apps/desktop/src/api/tauri.ts` に次を追加する（末尾に追加）:

```ts
export async function googleSignIn(): Promise<void> {
  return invoke<void>("google_sign_in");
}

export async function googleSignOut(): Promise<void> {
  return invoke<void>("google_sign_out");
}

export async function googleStatus(): Promise<GoogleStatus> {
  return invoke<GoogleStatus>("google_status");
}

export async function driveSyncNow(): Promise<DriveSyncOutcome> {
  return invoke<DriveSyncOutcome>("drive_sync_now");
}

export async function setupCompleted(): Promise<boolean> {
  return invoke<boolean>("setup_completed");
}
```

`apps/desktop/src/api/tauri.ts` 先頭の import を次に変更する:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { DriveSyncOutcome, GoogleStatus, Place, SaveSettingsInput, Settings, SortMode, SyncSummary, Visit } from "./types";
```

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- tauri.test.ts`
Expected: PASS（7テスト）

- [ ] **Step 6: 既存の `SettingsScreen.tsx` を型エラーが出ない最小限まで追従させる**

この時点では UI の作り込みは行わず、Step 3 で拡張した型に合わせてコンパイルを通すことだけを目的とする。本格的な UI（Google アカウント区画・カレンダートグル・キー状態表示）は Task 12 で書く。

`apps/desktop/src/screens/SettingsScreen.tsx` の `defaultSettings` を次に置き換える:

```ts
const defaultSettings: Settings = {
  watchedDirs: [],
  llmProvider: "none",
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "",
  openaiModel: "gpt-4o-mini",
  geminiModel: "gemini-1.5-flash",
  googlePlacesEnabled: false,
  minConfidence: 0.6,
  pollIntervalMinutes: 30,
  calendarEnabled: false,
  openaiKeyStatus: "not_set",
  geminiKeyStatus: "not_set",
  googlePlacesKeyStatus: "not_set",
};
```

`handleSave` 内の `saveSettings({...})` 呼び出しに `calendarEnabled: settings.calendarEnabled,` を追加する。`{settings.hasOpenaiKey ? "設定済み" : "未設定"}` は `{settings.openaiKeyStatus === "set" ? "設定済み" : "未設定"}` に、`hasGeminiKey`/`hasGooglePlacesKey` を使っている2箇所も同様に `geminiKeyStatus`/`googlePlacesKeyStatus` を使うよう書き換える。

- [ ] **Step 7: 既存のテストを含めて全て通ることを確認する**

Run: `cd apps/desktop && npm run test`
Expected: PASS（`tauri.test.ts` を含む全テストファイル）

- [ ] **Step 8: コミット**

```bash
git add apps/desktop/src/api/types.ts apps/desktop/src/api/tauri.ts apps/desktop/src/api/tauri.test.ts apps/desktop/src/screens/SettingsScreen.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add Google account and setup-completion calls to the api layer

googleSignIn/googleSignOut/googleStatus/driveSyncNow/setupCompleted
wrap the corresponding Tauri commands, and Settings/SaveSettingsInput
gain calendarEnabled and the tri-state *KeyStatus fields. tauri.test.ts
locks the exact invoke command names and argument keys down.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: フロントエンド — 設定画面に Google アカウント区画とカレンダートグルを実装する

**Files:**
- Modify: `apps/desktop/src/screens/SettingsScreen.tsx`
- Create: `apps/desktop/src/screens/SettingsScreen.test.tsx`

**Interfaces:**
- Consumes: Task 11 の `googleSignIn`/`googleSignOut`/`googleStatus`/`driveSyncNow`
- Produces: 設定画面に「Google アカウント」区画（サインイン／サインアウト／状態表示／「今すぐ Drive 同期」）と「カレンダー連携」トグルが表示される

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src/screens/SettingsScreen.test.tsx` を新規作成する:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./SettingsScreen";
import * as tauriApi from "../api/tauri";
import type { Settings } from "../api/types";

const baseSettings: Settings = {
  watchedDirs: [],
  llmProvider: "none",
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "",
  openaiModel: "gpt-4o-mini",
  geminiModel: "gemini-1.5-flash",
  googlePlacesEnabled: false,
  minConfidence: 0.6,
  pollIntervalMinutes: 30,
  calendarEnabled: false,
  openaiKeyStatus: "not_set",
  geminiKeyStatus: "not_set",
  googlePlacesKeyStatus: "not_set",
};

describe("SettingsScreen — Google アカウント", () => {
  it("shows signed-out status and calls google_sign_in when the sign-in button is clicked", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    const signIn = vi.spyOn(tauriApi, "googleSignIn").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText("未サインイン")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    await waitFor(() => expect(signIn).toHaveBeenCalledTimes(1));
  });

  it("shows signed-in status with sign-out and drive-sync buttons", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    const driveSync = vi.spyOn(tauriApi, "driveSyncNow").mockResolvedValue("uploaded");
    render(<SettingsScreen onBack={vi.fn()} />);

    expect(await screen.findByText("サインイン済み")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "今すぐ Drive 同期" }));
    await waitFor(() => expect(driveSync).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("uploaded")).toBeInTheDocument();
  });

  it("toggling the calendar checkbox and saving includes calendarEnabled", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_in");
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    render(<SettingsScreen onBack={vi.fn()} />);

    await screen.findByText("サインイン済み");
    fireEvent.click(screen.getByLabelText("Google カレンダーを自動で取り込む"));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ calendarEnabled: true });
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- SettingsScreen.test.tsx`
Expected: FAIL（「未サインイン」「Google でサインイン」等のテキスト・ボタンが存在しない）

- [ ] **Step 3: `SettingsScreen.tsx` を実装する**

`apps/desktop/src/screens/SettingsScreen.tsx` の import 行を次に置き換える:

```tsx
import { useEffect, useState } from "react";
import { driveSyncNow, getSettings, googleSignIn, googleSignOut, googleStatus, saveSettings } from "../api/tauri";
import type { GoogleStatus, LlmProvider, Settings } from "../api/types";
```

`export function SettingsScreen({ onBack }: Props) {` の中、既存の state 宣言群に追加する:

```tsx
  const [googleAccountStatus, setGoogleAccountStatus] = useState<GoogleStatus | null>(null);
  const [googleBusy, setGoogleBusy] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [driveSyncResult, setDriveSyncResult] = useState<string | null>(null);
```

既存の `useEffect(() => { getSettings().then(setSettings); }, []);` を次に置き換える:

```tsx
  useEffect(() => {
    getSettings().then(setSettings);
    googleStatus().then(setGoogleAccountStatus);
  }, []);
```

`handleSave` の直後に次のハンドラを追加する:

```tsx
  async function handleGoogleSignIn() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      await googleSignIn();
      setGoogleAccountStatus(await googleStatus());
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function handleGoogleSignOut() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      await googleSignOut();
      setGoogleAccountStatus(await googleStatus());
      setDriveSyncResult(null);
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function handleDriveSyncNow() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      setDriveSyncResult(await driveSyncNow());
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }
```

「同期」セクション（`<h2 ...>同期</h2>` を含む `<section>`）の直後、既存の

```tsx
      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google アカウント</h2>
        <p className="text-sm text-slate-500">Google Drive 連携は今後のバージョンで対応予定です。</p>
      </section>
```

を、丸ごと次に置き換える:

```tsx
      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">カレンダー連携</h2>
        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={settings.calendarEnabled}
            onChange={(e) => setSettings({ ...settings, calendarEnabled: e.target.checked })}
          />
          Google カレンダーを自動で取り込む
        </label>
        {settings.calendarEnabled && googleAccountStatus === "signed_in" && (
          <p className="text-sm text-slate-500">
            設定を保存した後、初めて有効にした場合はカレンダーへのアクセス許可のため Google への再サインインが必要です。
          </p>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google アカウント</h2>
        <p className="text-sm text-slate-700">
          {googleAccountStatus === "signed_in"
            ? "サインイン済み"
            : googleAccountStatus === "signed_out"
              ? "未サインイン"
              : "状態を確認しています…"}
        </p>
        <div className="flex items-center gap-3">
          {googleAccountStatus === "signed_in" ? (
            <>
              <button
                type="button"
                onClick={handleGoogleSignOut}
                disabled={googleBusy}
                className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
              >
                サインアウト
              </button>
              <button
                type="button"
                onClick={handleDriveSyncNow}
                disabled={googleBusy}
                className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
              >
                今すぐ Drive 同期
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={handleGoogleSignIn}
              disabled={googleBusy}
              className="rounded-md bg-slate-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50"
            >
              Google でサインイン
            </button>
          )}
          {googleBusy && <span className="text-sm text-slate-500">処理中…</span>}
        </div>
        {driveSyncResult !== null && <p className="text-sm text-slate-600">{driveSyncResult}</p>}
        {googleError !== null && <p className="text-sm text-red-600">{googleError}</p>}
      </section>
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- SettingsScreen.test.tsx`
Expected: PASS（3テスト）

- [ ] **Step 5: 全体のテストを実行して確認する**

Run: `cd apps/desktop && npm run test`
Expected: PASS（全テストファイル）

- [ ] **Step 6: コミット**

```bash
git add apps/desktop/src/screens/SettingsScreen.tsx apps/desktop/src/screens/SettingsScreen.test.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add the Google account section to the settings screen

Replaces the "coming in a future version" placeholder with sign-in/
sign-out, live status, and a "sync Drive now" button wired to the
existing google_sign_in/google_sign_out/google_status/drive_sync_now
commands, plus a calendar-import toggle that explains when
re-authentication is needed.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 13: `areitu-core` — `Source::Timeline` とクラスタの自前ヒント化

**Files:**
- Modify: `crates/areitu-core/src/model.rs`
- Modify: `crates/areitu-core/src/cluster.rs`

**Interfaces:**
- Consumes: なし
- Produces: `Source::Timeline`（`as_str() == "timeline"`）。`cluster()` が、緯度経度を持つどの `RawLog`（写真・タイムラインなど）でも自身の `text` をそのクラスタ自身の `hints` に含めるようになる（`Source::Calendar` の既存の hints 付与ロジックとは独立に動く）。Task 14 の `timeline::to_raw_log` が生成する `RawLog` がこの経路に乗る

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/model.rs` の末尾に追加:

```rust
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
```

`crates/areitu-core/src/cluster.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    fn timeline(id: i64, from: &str, to: &str, lat: f64, lon: f64, name: &str) -> (i64, RawLog) {
        (id, RawLog {
            source: Source::Timeline,
            source_id: format!("t{id}"),
            occurred_at: t(from),
            ended_at: Some(t(to)),
            lat: Some(lat),
            lon: Some(lon),
            text: Some(name.into()),
        })
    }

    #[test]
    fn a_log_with_text_seeds_its_own_cluster_hint() {
        let v = cluster(&[timeline(1, "2026-09-01 12:00", "2026-09-01 12:30", CAFE.0, CAFE.1, "カフェ丸の内")]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].hints, vec!["カフェ丸の内".to_owned()]);
    }

    #[test]
    fn joining_an_existing_cluster_appends_its_text_as_a_hint_too() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
            timeline(2, "2026-09-01 12:10", "2026-09-01 12:20", CAFE.0, CAFE.1, "カフェ丸の内"),
        ]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].hints, vec!["カフェ丸の内".to_owned()]);
    }
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cargo test -p areitu-core --lib model:: cluster::tests::a_log_with_text cluster::tests::joining_an_existing_cluster`
Expected: FAIL（`Source::Timeline` が存在せずコンパイルエラー。`cluster.rs` は `Source::Timeline` を使うためこの時点でビルドが通らない）

- [ ] **Step 3: `Source::Timeline` を追加する**

`crates/areitu-core/src/model.rs` の `Source` enum とその impl を次に置き換える:

```rust
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
```

- [ ] **Step 4: テストを実行して通ることを確認する（`model.rs` のみ）**

Run: `cargo test -p areitu-core --lib model::`
Expected: PASS（2テスト）

- [ ] **Step 5: `cluster()` が緯度経度を持つログの `text` を自クラスタのヒントにも使うよう変更する**

`crates/areitu-core/src/cluster.rs` の `cluster` 関数と `attach_calendar` 関数を次に置き換える:

```rust
pub fn cluster(logs: &[(i64, RawLog)]) -> Vec<VisitCandidate> {
    let mut points: Vec<(i64, &RawLog, (f64, f64))> = logs
        .iter()
        .filter_map(|(id, l)| Some((*id, l, (l.lat?, l.lon?))))
        .collect();
    points.sort_by_key(|(id, l, _)| (l.occurred_at, *id));

    let mut out: Vec<VisitCandidate> = Vec::new();
    for (id, l, p) in points {
        let end = l.ended_at.unwrap_or(l.occurred_at);
        let hints = text_hints(&l.text);
        if let Some(c) = out.last_mut() {
            let near = haversine_m((c.lat, c.lon), p) <= RADIUS_M;
            let soon = l.occurred_at - c.ended_at <= Duration::minutes(MAX_GAP_MINUTES);
            if near && soon {
                let n = c.log_ids.len() as f64;
                c.lat = (c.lat * n + p.0) / (n + 1.0);
                c.lon = (c.lon * n + p.1) / (n + 1.0);
                c.ended_at = c.ended_at.max(end);
                c.log_ids.push(id);
                c.hints.extend(hints);
                continue;
            }
        }
        out.push(VisitCandidate {
            started_at: l.occurred_at,
            ended_at: end,
            lat: p.0,
            lon: p.1,
            log_ids: vec![id],
            hints,
        });
    }
    attach_calendar(&mut out, logs);
    out
}

/// `text` を、既存の `attach_calendar` と同じ「改行区切り・トリム・空行除去」で
/// ヒントの列に変換する。写真は `text` を持たないため影響を受けない。
fn text_hints(text: &Option<String>) -> Vec<String> {
    text.as_deref()
        .map(|t| t.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn attach_calendar(cands: &mut [VisitCandidate], logs: &[(i64, RawLog)]) {
    let margin = Duration::minutes(HINT_MARGIN_MINUTES);
    for (id, l) in logs.iter().filter(|(_, l)| l.source == Source::Calendar) {
        let start = l.occurred_at;
        let end = l.ended_at.unwrap_or(start);
        let mut assigned = false;
        for c in cands.iter_mut() {
            if start <= c.ended_at + margin && end >= c.started_at - margin {
                c.hints.extend(text_hints(&l.text));
                if !assigned {
                    c.log_ids.push(*id);
                    assigned = true;
                }
            }
        }
    }
}
```

- [ ] **Step 6: テストを実行して通ることを確認する**

Run: `cargo test -p areitu-core --lib cluster::`
Expected: PASS（既存テスト・Step 1 の新規テストすべて。`overlapping_event_adds_hints_and_log_id` 等、カレンダーの既存挙動も変わらないことを確認する）

- [ ] **Step 7: ワークスペース全体のビルドを確認する**

Run: `cargo build --workspace`
Expected: エラーなく終了する（`Source` を網羅的にマッチしている箇所は `model.rs` の `as_str`/`parse` のみで、他の呼び出し元は `Source::Photo`/`Source::Calendar` の構築か `Source::parse` の呼び出しのみなので影響を受けない）

- [ ] **Step 8: コミット**

```bash
git add crates/areitu-core/src/model.rs crates/areitu-core/src/cluster.rs
git commit -m "$(cat <<'EOF'
feat(areitu-core): add Source::Timeline and let any log's text seed hints

Source::Timeline gives Google Timeline imports (Task 14) their own
raw_logs source. Since timeline visits carry a real place name, the
clustering step now seeds a cluster's own hints from any log's text
field (not just calendar events), so the resolver can use it the same
way it already uses calendar hints. Photos never carry text, so their
behavior is unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 14: `areitu-core` — Google タイムライン取り込み（`timeline.rs`）とデスクトップコマンド

**Files:**
- Create: `crates/areitu-core/src/timeline.rs`
- Modify: `crates/areitu-core/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 13 の `Source::Timeline`、既存の `store::upsert_raw_log`
- Produces: `areitu_core::timeline::{TimelineVisit, parse_timeline_export, to_raw_log, ingest_timeline_file}`、Tauri コマンド `import_timeline_file(state, path: String) -> Result<usize, String>`

**入力形式の前提（このタスクで明示的に仮定する仕様）:**
- (a) オンデバイス export（`Timeline.json`）: 最上位に `semanticSegments` 配列。各要素は `startTime`/`endTime`（オフセット付き ISO 8601）と、任意で `visit.topCandidate.placeLocation.latLng`（`"35.681236°, 139.767125°"` のような度数記号付き文字列、または `"geo:35.681236,139.767125"` の `geo:` URI）を持つ。場所名は `visit.topCandidate.name` にあれば使うが、無くても緯度経度があれば取り込む（実際のオンデバイス export の多くは場所名を含まないため、名前の有無で取り込みをスキップしない）
- (b) レガシー Takeout の月次 JSON: 最上位に `timelineObjects` 配列。各要素のうち `placeVisit` を持つものだけを対象にし、`location.latitudeE7`/`location.longitudeE7`（1e7 倍整数）・`location.name`（任意）・`duration.startTimestamp`/`duration.endTimestamp`（ISO 8601）を読む
- どちらの形式かはトップレベルのキー（`semanticSegments` か `timelineObjects` か）で自動判定する。どちらでもなければエラーにする
- 場所名（`name`）がある visit はその名前を raw_log の `text` に入れる。無い visit も緯度経度と時刻さえ取れれば raw_log として取り込む（写真と同様、後段の逆ジオコーディング／LLM 解決に委ねる）

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/timeline.rs` を新規作成する:

```rust
use chrono::{DateTime, NaiveDateTime};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineVisit {
    pub lat: f64,
    pub lon: f64,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub name: Option<String>,
}

fn wall_clock(s: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.naive_local())
}

/// `"35.681236°, 139.767125°"` と `"geo:35.681236,139.767125"` のどちらも読む。
fn parse_lat_lng(s: &str) -> Option<(f64, f64)> {
    let s = s.trim().strip_prefix("geo:").unwrap_or(s.trim());
    let mut parts = s.split(',');
    let lat: f64 = parts.next()?.trim().trim_end_matches('°').parse().ok()?;
    let lon: f64 = parts.next()?.trim().trim_end_matches('°').parse().ok()?;
    Some((lat, lon))
}

#[derive(Deserialize)]
struct OnDeviceExport {
    #[serde(default, rename = "semanticSegments")]
    semantic_segments: Vec<Segment>,
}

#[derive(Deserialize)]
struct Segment {
    #[serde(default, rename = "startTime")]
    start_time: Option<String>,
    #[serde(default, rename = "endTime")]
    end_time: Option<String>,
    #[serde(default)]
    visit: Option<Visit>,
}

#[derive(Deserialize)]
struct Visit {
    #[serde(rename = "topCandidate")]
    top_candidate: TopCandidate,
}

#[derive(Deserialize)]
struct TopCandidate {
    #[serde(rename = "placeLocation")]
    place_location: PlaceLocation,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct PlaceLocation {
    #[serde(rename = "latLng")]
    lat_lng: String,
}

fn parse_on_device(json: &str) -> Result<Vec<TimelineVisit>> {
    let export: OnDeviceExport = serde_json::from_str(json)?;
    Ok(export
        .semantic_segments
        .into_iter()
        .filter_map(|seg| {
            let visit = seg.visit?;
            let (lat, lon) = parse_lat_lng(&visit.top_candidate.place_location.lat_lng)?;
            let start = wall_clock(seg.start_time.as_deref()?)?;
            let end = wall_clock(seg.end_time.as_deref()?)?;
            let name = visit.top_candidate.name.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            Some(TimelineVisit { lat, lon, start, end, name })
        })
        .collect())
}

#[derive(Deserialize)]
struct TakeoutExport {
    #[serde(default, rename = "timelineObjects")]
    timeline_objects: Vec<TimelineObject>,
}

#[derive(Deserialize)]
struct TimelineObject {
    #[serde(default, rename = "placeVisit")]
    place_visit: Option<PlaceVisit>,
}

#[derive(Deserialize)]
struct PlaceVisit {
    location: TakeoutLocation,
    duration: TakeoutDuration,
}

#[derive(Deserialize)]
struct TakeoutLocation {
    #[serde(rename = "latitudeE7")]
    latitude_e7: i64,
    #[serde(rename = "longitudeE7")]
    longitude_e7: i64,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct TakeoutDuration {
    #[serde(rename = "startTimestamp")]
    start_timestamp: String,
    #[serde(rename = "endTimestamp")]
    end_timestamp: String,
}

fn parse_takeout(json: &str) -> Result<Vec<TimelineVisit>> {
    let export: TakeoutExport = serde_json::from_str(json)?;
    Ok(export
        .timeline_objects
        .into_iter()
        .filter_map(|obj| {
            let pv = obj.place_visit?;
            let start = wall_clock(&pv.duration.start_timestamp)?;
            let end = wall_clock(&pv.duration.end_timestamp)?;
            let lat = pv.location.latitude_e7 as f64 / 1e7;
            let lon = pv.location.longitude_e7 as f64 / 1e7;
            let name = pv.location.name.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            Some(TimelineVisit { lat, lon, start, end, name })
        })
        .collect())
}

pub fn parse_timeline_export(json: &str) -> Result<Vec<TimelineVisit>> {
    let value: Value = serde_json::from_str(json)?;
    let obj = value.as_object().ok_or_else(|| Error::Invalid("timeline export is not a JSON object".into()))?;
    if obj.contains_key("semanticSegments") {
        parse_on_device(json)
    } else if obj.contains_key("timelineObjects") {
        parse_takeout(json)
    } else {
        Err(Error::Invalid("unrecognized timeline export: expected semanticSegments or timelineObjects".into()))
    }
}

pub fn to_raw_log(v: &TimelineVisit) -> RawLog {
    let source_id = format!("{:.6},{:.6}@{}", v.lat, v.lon, v.start.format("%Y%m%dT%H%M%S"));
    RawLog {
        source: Source::Timeline,
        source_id,
        occurred_at: v.start,
        ended_at: Some(v.end),
        lat: Some(v.lat),
        lon: Some(v.lon),
        text: v.name.clone(),
    }
}

pub fn ingest_timeline_file(conn: &Connection, path: &Path) -> Result<usize> {
    let visits = parse_timeline_export(&std::fs::read_to_string(path)?)?;
    for v in &visits {
        upsert_raw_log(conn, &to_raw_log(v))?;
    }
    Ok(visits.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON_DEVICE_JSON: &str = r#"{
      "semanticSegments": [
        {
          "startTime": "2026-09-01T12:00:00.000+09:00",
          "endTime": "2026-09-01T13:00:00.000+09:00",
          "visit": {
            "topCandidate": {
              "placeLocation": {"latLng": "35.681236°, 139.767125°"},
              "name": "カフェ丸の内"
            }
          }
        },
        {
          "startTime": "2026-09-02T09:00:00.000+09:00",
          "endTime": "2026-09-02T09:30:00.000+09:00",
          "visit": {
            "topCandidate": {
              "placeLocation": {"latLng": "geo:35.714800,139.774500"}
            }
          }
        },
        {
          "startTime": "2026-09-02T10:00:00.000+09:00",
          "endTime": "2026-09-02T10:40:00.000+09:00",
          "activity": {"start": {"latLng": "35.7°, 139.7°"}}
        }
      ]
    }"#;

    const TAKEOUT_JSON: &str = r#"{
      "timelineObjects": [
        {
          "placeVisit": {
            "location": {"latitudeE7": 356812360, "longitudeE7": 1397671250, "name": "カフェ丸の内"},
            "duration": {"startTimestamp": "2026-09-01T03:00:00.000Z", "endTimestamp": "2026-09-01T04:00:00.000Z"}
          }
        },
        {
          "placeVisit": {
            "location": {"latitudeE7": 357148000, "longitudeE7": 1397745000},
            "duration": {"startTimestamp": "2026-09-02T00:00:00.000Z", "endTimestamp": "2026-09-02T00:30:00.000Z"}
          }
        },
        {
          "activitySegment": {"distance": 1200}
        }
      ]
    }"#;

    #[test]
    fn on_device_export_reads_degree_symbol_lat_lng_and_name() {
        let visits = parse_timeline_export(ON_DEVICE_JSON).unwrap();
        assert_eq!(visits.len(), 2, "the activity segment without a visit must be skipped");
        assert_eq!(visits[0].name.as_deref(), Some("カフェ丸の内"));
        assert!((visits[0].lat - 35.681236).abs() < 1e-6);
        assert!((visits[0].lon - 139.767125).abs() < 1e-6);
    }

    #[test]
    fn on_device_export_reads_geo_uri_lat_lng_and_allows_no_name() {
        let visits = parse_timeline_export(ON_DEVICE_JSON).unwrap();
        assert_eq!(visits[1].name, None, "a visit without a name must still be imported");
        assert!((visits[1].lat - 35.7148).abs() < 1e-6);
        assert!((visits[1].lon - 139.7745).abs() < 1e-6);
    }

    #[test]
    fn takeout_export_reads_e7_coordinates_and_skips_non_place_visit_entries() {
        let visits = parse_timeline_export(TAKEOUT_JSON).unwrap();
        assert_eq!(visits.len(), 2, "the activitySegment entry must be skipped");
        assert_eq!(visits[0].name.as_deref(), Some("カフェ丸の内"));
        assert!((visits[0].lat - 35.681236).abs() < 1e-6);
        assert!((visits[0].lon - 139.767125).abs() < 1e-6);
        assert_eq!(visits[1].name, None);
    }

    #[test]
    fn unrecognized_format_is_an_error() {
        let err = parse_timeline_export(r#"{"somethingElse": []}"#).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_timeline_export("{").is_err());
    }

    #[test]
    fn to_raw_log_carries_source_timeline_and_name_as_text() {
        let v = TimelineVisit {
            lat: 35.6812,
            lon: 139.7671,
            start: wall_clock("2026-09-01T12:00:00+09:00").unwrap(),
            end: wall_clock("2026-09-01T13:00:00+09:00").unwrap(),
            name: Some("カフェ丸の内".to_owned()),
        };
        let log = to_raw_log(&v);
        assert_eq!(log.source, Source::Timeline);
        assert_eq!(log.text.as_deref(), Some("カフェ丸の内"));
        assert_eq!(log.lat, Some(35.6812));
    }

    #[test]
    fn ingest_timeline_file_inserts_raw_logs_and_returns_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Timeline.json");
        std::fs::write(&path, ON_DEVICE_JSON).unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        let inserted = ingest_timeline_file(&conn, &path).unwrap();
        assert_eq!(inserted, 2);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'timeline'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn reimporting_the_same_file_does_not_duplicate_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Timeline.json");
        std::fs::write(&path, ON_DEVICE_JSON).unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        ingest_timeline_file(&conn, &path).unwrap();
        ingest_timeline_file(&conn, &path).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM raw_logs WHERE source = 'timeline'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }
}
```

- [ ] **Step 2: モジュールを登録してテストを実行する**

`crates/areitu-core/src/lib.rs` の `pub mod store;` の下に追加:

```rust
pub mod timeline;
```

Run: `cargo test -p areitu-core --lib timeline::`
Expected: PASS（8テスト。この時点で既にモジュール本体を書いているので、このステップは「失敗するテストを先に書く」ではなく実装とテストを一体で導入する。理由: パーサーはテストなしでは正しさを主張できず、かつフォーマット分岐のロジック自体が小さいため、フィールド定義とテストを分離して2段階にする意味が薄い）

- [ ] **Step 3: ワークスペース全体のビルドを確認する**

Run: `cargo build --workspace && cargo test -p areitu-core`
Expected: エラーなく終了し、`areitu-core` の全テストが PASS する

- [ ] **Step 4: デスクトップアプリに `import_timeline_file` コマンドを追加する**

`apps/desktop/src-tauri/src/commands.rs` の `setup_completed` の直後に追加:

```rust
#[tauri::command]
pub fn import_timeline_file(state: State<AppState>, path: String) -> Result<usize, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    areitu_core::timeline::ingest_timeline_file(&conn, std::path::Path::new(&path)).map_err(|e| e.to_string())
}
```

`apps/desktop/src-tauri/src/lib.rs` の `invoke_handler![...]` に `commands::import_timeline_file,` を `commands::setup_completed,` の直後に追加する。

- [ ] **Step 5: デスクトップアプリ側のテストを書く**

`apps/desktop/src-tauri/src/commands.rs` の `#[cfg(test)] mod tests` に追加:

```rust
    #[test]
    fn import_timeline_file_dto_round_trip_is_covered_by_areitu_core() {
        // import_timeline_file コマンド自体は AppState.conn のロックと
        // areitu_core::timeline::ingest_timeline_file への委譲のみで、
        // パースロジックは areitu-core 側の timeline::tests で検証済み。
        // ここでは委譲先のシグネチャが変わっていないことだけを型で確認する。
        fn _assert_signature(
            f: fn(&rusqlite::Connection, &std::path::Path) -> areitu_core::Result<usize>,
        ) {
            let _ = f;
        }
        _assert_signature(areitu_core::timeline::ingest_timeline_file);
    }
```

- [ ] **Step 6: テストとビルドを確認する**

Run: `cargo test -p areitu-desktop --lib commands:: && cargo build -p areitu-desktop`
Expected: PASS・エラーなく終了する

- [ ] **Step 7: コミット**

```bash
git add crates/areitu-core/src/timeline.rs crates/areitu-core/src/lib.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(areitu-core): import Google Timeline exports as raw_logs

Adds areitu_core::timeline, parsing both the current on-device
Timeline.json (semanticSegments, degree-symbol or geo: coordinates)
and the legacy Takeout Semantic Location History monthly export
(timelineObjects, E7 integer coordinates). Visits import even without
a place name, matching how photos already work; a name becomes the
raw_log's text hint. Wires the new desktop import_timeline_file
command to it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 15: デスクトップ — フォルダ／ファイル選択ダイアログプラグインの導入

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/package.json`

**Interfaces:**
- Consumes: なし
- Produces: `@tauri-apps/plugin-dialog` の `open()` がフロントエンドから使えるようになる。Task 16・Task 18 がこれを使う

このタスクはネイティブダイアログの登録・権限付与であり、ユニットテストで検証できるロジックを持たない。ビルドが通ることを確認の基準とする。

- [ ] **Step 1: Rust 側のプラグインを追加する**

Run: `cd apps/desktop/src-tauri && cargo add tauri-plugin-dialog`
Expected: `Cargo.toml` に `tauri-plugin-dialog` が追加される

`apps/desktop/src-tauri/src/lib.rs` の `.plugin(tauri_plugin_autostart::init(...))` の直後に追加:

```rust
        .plugin(tauri_plugin_dialog::init())
```

- [ ] **Step 2: 権限を追加する**

`apps/desktop/src-tauri/capabilities/default.json` の `"permissions"` 配列に `"dialog:default"` を追加する:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "dialog:default"
  ]
}
```

- [ ] **Step 3: ビルドを確認する**

Run: `cargo build -p areitu-desktop`
Expected: エラーなく終了する

- [ ] **Step 4: フロントエンド側のパッケージを追加する**

Run: `cd apps/desktop && npm install @tauri-apps/plugin-dialog`
Expected: `package.json`/`package-lock.json` に `@tauri-apps/plugin-dialog` が追加される

- [ ] **Step 5: フロントエンドのビルドを確認する**

Run: `cd apps/desktop && npm run build`
Expected: エラーなく終了する（この時点ではまだ `@tauri-apps/plugin-dialog` を import しているコードはないため、単に依存関係が解決できることを確認する）

- [ ] **Step 6: コミット**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/capabilities/default.json apps/desktop/package.json apps/desktop/package-lock.json
git commit -m "$(cat <<'EOF'
chore(desktop): add the native folder/file picker dialog plugin

Registers tauri-plugin-dialog and grants dialog:default so the
onboarding screen's folder picker and the timeline file import button
(Task 16 onward) can use @tauri-apps/plugin-dialog's open().

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 16: フロントエンド — `importTimelineFile` の API 追加と設定画面への取り込みボタン

**Files:**
- Modify: `apps/desktop/src/api/tauri.ts`
- Modify: `apps/desktop/src/api/tauri.test.ts`
- Modify: `apps/desktop/src/screens/SettingsScreen.tsx`
- Modify: `apps/desktop/src/screens/SettingsScreen.test.tsx`

**Interfaces:**
- Consumes: Task 14 の `import_timeline_file` コマンド、Task 15 の `@tauri-apps/plugin-dialog`
- Produces: `importTimelineFile(path: string): Promise<number>`。設定画面に「Google タイムラインを取り込む」ボタンが表示される

- [ ] **Step 1: 失敗するテストを書く（API レイヤー）**

`apps/desktop/src/api/tauri.test.ts` の `describe("tauri api layer", ...)` ブロックに追加:

```ts
  it("importTimelineFile invokes import_timeline_file with the path argument", async () => {
    const spy = vi.spyOn(core, "invoke").mockResolvedValue(3);
    await expect(importTimelineFile("/Users/me/Timeline.json")).resolves.toBe(3);
    expect(spy).toHaveBeenCalledWith("import_timeline_file", { path: "/Users/me/Timeline.json" });
  });
```

`apps/desktop/src/api/tauri.test.ts` の import 文に `importTimelineFile` を追加する:

```ts
import {
  driveSyncNow,
  getSettings,
  googleSignIn,
  googleSignOut,
  googleStatus,
  importTimelineFile,
  saveSettings,
  setupCompleted,
} from "./tauri";
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- tauri.test.ts`
Expected: FAIL（`importTimelineFile` が存在しない）

- [ ] **Step 3: `importTimelineFile` を実装する**

`apps/desktop/src/api/tauri.ts` の末尾に追加:

```ts
export async function importTimelineFile(path: string): Promise<number> {
  return invoke<number>("import_timeline_file", { path });
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- tauri.test.ts`
Expected: PASS（8テスト）

- [ ] **Step 5: 失敗するテストを書く（設定画面のボタン）**

`apps/desktop/src/screens/SettingsScreen.test.tsx` に追加（先頭の import に `import * as dialog from "@tauri-apps/plugin-dialog";` を足す）:

```ts
import * as dialog from "@tauri-apps/plugin-dialog";
```

`describe` ブロックの末尾に追加:

```tsx
describe("SettingsScreen — Google タイムライン取り込み", () => {
  it("opens a file picker and shows how many visits were imported", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.spyOn(dialog, "open").mockResolvedValue("/Users/me/Timeline.json");
    const importFile = vi.spyOn(tauriApi, "importTimelineFile").mockResolvedValue(5);
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Google タイムラインを取り込む" }));
    await waitFor(() => expect(importFile).toHaveBeenCalledWith("/Users/me/Timeline.json"));
    expect(await screen.findByText("5 件の訪問を取り込みました")).toBeInTheDocument();
  });

  it("does nothing when the file picker is cancelled", async () => {
    vi.spyOn(tauriApi, "getSettings").mockResolvedValue(baseSettings);
    vi.spyOn(tauriApi, "googleStatus").mockResolvedValue("signed_out");
    vi.spyOn(dialog, "open").mockResolvedValue(null);
    const importFile = vi.spyOn(tauriApi, "importTimelineFile");
    render(<SettingsScreen onBack={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Google タイムラインを取り込む" }));
    await waitFor(() => expect(dialog.open).toHaveBeenCalledTimes(1));
    expect(importFile).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 6: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- SettingsScreen.test.tsx`
Expected: FAIL（「Google タイムラインを取り込む」ボタンが存在しない）

- [ ] **Step 7: `SettingsScreen.tsx` に取り込みボタンを実装する**

`apps/desktop/src/screens/SettingsScreen.tsx` の import に追加:

```tsx
import { open } from "@tauri-apps/plugin-dialog";
import { driveSyncNow, getSettings, googleSignIn, googleSignOut, googleStatus, importTimelineFile, saveSettings } from "../api/tauri";
```

state 宣言に追加:

```tsx
  const [timelineImportResult, setTimelineImportResult] = useState<string | null>(null);
  const [timelineImportError, setTimelineImportError] = useState<string | null>(null);
```

`handleDriveSyncNow` の直後にハンドラを追加する:

```tsx
  async function handleImportTimeline() {
    setTimelineImportError(null);
    const selected = await open({
      multiple: false,
      filters: [{ name: "Google Timeline / Takeout JSON", extensions: ["json"] }],
    });
    if (selected === null || Array.isArray(selected)) {
      return;
    }
    try {
      const count = await importTimelineFile(selected);
      setTimelineImportResult(`${count} 件の訪問を取り込みました`);
    } catch (e) {
      setTimelineImportError(String(e));
    }
  }
```

「Google アカウント」の `<section>` の直後に新しい `<section>` を追加する:

```tsx
      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">データの取り込み</h2>
        <p className="text-sm text-slate-500">
          Google タイムラインのエクスポート（Timeline.json、または Google Takeout の位置情報履歴）を読み込みます。
        </p>
        <button
          type="button"
          onClick={handleImportTimeline}
          className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
        >
          Google タイムラインを取り込む
        </button>
        {timelineImportResult !== null && <p className="text-sm text-slate-600">{timelineImportResult}</p>}
        {timelineImportError !== null && <p className="text-sm text-red-600">{timelineImportError}</p>}
      </section>
```

- [ ] **Step 8: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- SettingsScreen.test.tsx`
Expected: PASS（5テスト）

- [ ] **Step 9: 全体のテストとビルドを確認する**

Run: `cd apps/desktop && npm run test && npm run build`
Expected: PASS・エラーなく終了する

- [ ] **Step 10: コミット**

```bash
git add apps/desktop/src/api/tauri.ts apps/desktop/src/api/tauri.test.ts apps/desktop/src/screens/SettingsScreen.tsx apps/desktop/src/screens/SettingsScreen.test.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add a Google Timeline import button to the settings screen

importTimelineFile wraps the import_timeline_file command. The
settings screen's new "データの取り込み" section opens the native file
picker (tauri-plugin-dialog), imports the selected export, and shows
how many visits were added; a cancelled picker does nothing.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 17: フロントエンド — 初回セットアップ画面（`OnboardingScreen`）と `App.tsx` の分岐

**Files:**
- Create: `apps/desktop/src/screens/OnboardingScreen.tsx`
- Create: `apps/desktop/src/screens/OnboardingScreen.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Create: `apps/desktop/src/App.test.tsx`

**Interfaces:**
- Consumes: Task 11 の `setupCompleted`/`googleSignIn`/`saveSettings`、Task 15 の `@tauri-apps/plugin-dialog`
- Produces: `OnboardingScreen`（`{ onFinish: () => void }`）。`App.tsx` は起動時に `setupCompleted()` を呼び、`false` なら `OnboardingScreen` を、`true` なら従来の一覧画面を表示する

- [ ] **Step 1: 失敗するテストを書く（`OnboardingScreen`）**

`apps/desktop/src/screens/OnboardingScreen.test.tsx` を新規作成する:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import * as dialog from "@tauri-apps/plugin-dialog";
import { OnboardingScreen } from "./OnboardingScreen";
import * as tauriApi from "../api/tauri";

describe("OnboardingScreen", () => {
  it("adds a folder chosen from the native picker to the watched list", async () => {
    vi.spyOn(dialog, "open").mockResolvedValue(["/Users/me/Pictures"]);
    render(<OnboardingScreen onFinish={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "フォルダを選択" }));
    expect(await screen.findByText("/Users/me/Pictures")).toBeInTheDocument();
  });

  it("skip saves a minimal config and calls onFinish without requiring any input", async () => {
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "スキップ" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ watchedDirs: [], llmProvider: "none", calendarEnabled: false });
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it("finish saves the chosen folder and llm provider and calls onFinish", async () => {
    vi.spyOn(dialog, "open").mockResolvedValue(["/Users/me/Pictures"]);
    const save = vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    const onFinish = vi.fn();
    render(<OnboardingScreen onFinish={onFinish} />);

    fireEvent.click(screen.getByRole("button", { name: "フォルダを選択" }));
    await screen.findByText("/Users/me/Pictures");
    fireEvent.click(screen.getByRole("button", { name: "はじめる" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][0]).toMatchObject({ watchedDirs: ["/Users/me/Pictures"] });
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it("google sign-in errors are shown but do not block finishing", async () => {
    vi.spyOn(tauriApi, "googleSignIn").mockRejectedValue(new Error("user closed the browser"));
    vi.spyOn(tauriApi, "saveSettings").mockResolvedValue(undefined);
    render(<OnboardingScreen onFinish={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Google でサインイン" }));
    expect(await screen.findByText("Error: user closed the browser")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "はじめる" })).not.toBeDisabled();
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- OnboardingScreen.test.tsx`
Expected: FAIL（`OnboardingScreen` が存在しない）

- [ ] **Step 3: `OnboardingScreen.tsx` を実装する**

`apps/desktop/src/screens/OnboardingScreen.tsx` を新規作成する:

```tsx
import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { googleSignIn, saveSettings } from "../api/tauri";
import type { LlmProvider } from "../api/types";

interface Props {
  onFinish: () => void;
}

export function OnboardingScreen({ onFinish }: Props) {
  const [watchedDirs, setWatchedDirs] = useState<string[]>([]);
  const [llmProvider, setLlmProvider] = useState<LlmProvider>("none");
  const [calendarEnabled, setCalendarEnabled] = useState(false);
  const [googleSignedIn, setGoogleSignedIn] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handlePickFolder() {
    const selected = await open({ directory: true, multiple: true });
    if (selected === null) {
      return;
    }
    const picked = Array.isArray(selected) ? selected : [selected];
    setWatchedDirs((prev) => Array.from(new Set([...prev, ...picked])));
  }

  async function handleGoogleSignIn() {
    setGoogleError(null);
    try {
      await googleSignIn();
      setGoogleSignedIn(true);
    } catch (e) {
      setGoogleError(String(e));
    }
  }

  async function finishWith(dirs: string[], provider: LlmProvider, calendar: boolean) {
    setBusy(true);
    setError(null);
    try {
      await saveSettings({
        watchedDirs: dirs,
        llmProvider: provider,
        ollamaUrl: "http://localhost:11434",
        ollamaModel: "",
        openaiModel: "gpt-4o-mini",
        geminiModel: "gemini-1.5-flash",
        googlePlacesEnabled: false,
        minConfidence: 0.6,
        pollIntervalMinutes: 30,
        calendarEnabled: calendar,
        openaiApiKey: null,
        geminiApiKey: null,
        googlePlacesApiKey: null,
      });
      onFinish();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-6 p-8">
      <h1 className="text-xl font-semibold text-slate-900">AREITU へようこそ</h1>
      <p className="text-sm text-slate-600">
        写真の保存フォルダを選ぶだけで、いつどこに何回行ったかを検索できるようになります。Google 連携と店舗名の推論方法は後から設定画面でいつでも変更できます。
      </p>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">監視フォルダ</h2>
        <ul className="flex flex-col gap-1">
          {watchedDirs.map((dir) => (
            <li key={dir} className="rounded-md border border-slate-200 px-3 py-2 text-sm text-slate-700">
              {dir}
            </li>
          ))}
        </ul>
        <button
          type="button"
          onClick={handlePickFolder}
          className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
        >
          フォルダを選択
        </button>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google 連携（任意）</h2>
        <button
          type="button"
          onClick={handleGoogleSignIn}
          className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
        >
          Google でサインイン
        </button>
        {googleSignedIn && <p className="text-sm text-slate-600">サインインしました</p>}
        {googleError !== null && <p className="text-sm text-red-600">{googleError}</p>}
        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input type="checkbox" checked={calendarEnabled} onChange={(e) => setCalendarEnabled(e.target.checked)} />
          Google カレンダーを自動で取り込む
        </label>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">店舗名の推論（任意）</h2>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          プロバイダ
          <select
            value={llmProvider}
            onChange={(e) => setLlmProvider(e.target.value as LlmProvider)}
            className="rounded-md border border-slate-300 px-3 py-2"
          >
            <option value="none">使わない</option>
            <option value="ollama">Ollama（ローカル）</option>
            <option value="openai">OpenAI</option>
            <option value="gemini">Gemini</option>
          </select>
        </label>
        <p className="text-sm text-slate-500">API キーは設定画面から後で登録できます。</p>
      </section>

      {error !== null && <p className="text-sm text-red-600">{error}</p>}

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => finishWith(watchedDirs, llmProvider, calendarEnabled)}
          disabled={busy}
          className="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50"
        >
          はじめる
        </button>
        <button
          type="button"
          onClick={() => finishWith([], "none", false)}
          disabled={busy}
          className="text-sm text-slate-500 hover:text-slate-700 disabled:opacity-50"
        >
          スキップ
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- OnboardingScreen.test.tsx`
Expected: PASS（4テスト）

- [ ] **Step 5: 失敗するテストを書く（`App.tsx` の分岐）**

`apps/desktop/src/App.test.tsx` を新規作成する:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import App from "./App";
import * as tauriApi from "./api/tauri";

describe("App", () => {
  it("shows the onboarding screen when setup has not been completed", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockResolvedValue(false);
    render(<App />);
    expect(await screen.findByText("AREITU へようこそ")).toBeInTheDocument();
  });

  it("shows the normal list screen when setup has already been completed", async () => {
    vi.spyOn(tauriApi, "setupCompleted").mockResolvedValue(true);
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([]);
    render(<App />);
    expect(await screen.findByText("設定")).toBeInTheDocument();
    expect(screen.queryByText("AREITU へようこそ")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 6: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test -- App.test.tsx`
Expected: FAIL（`setup_completed` を判定する分岐がまだない）

- [ ] **Step 7: `App.tsx` にオンボーディング分岐を実装する**

`apps/desktop/src/App.tsx` を次に置き換える:

```tsx
import { useEffect, useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { OnboardingScreen } from "./screens/OnboardingScreen";
import { setupCompleted } from "./api/tauri";
import type { Place } from "./api/types";

type View = { kind: "list" } | { kind: "detail"; place: Place } | { kind: "settings" };

export default function App() {
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);
  const [view, setView] = useState<View>({ kind: "list" });

  useEffect(() => {
    setupCompleted().then((completed) => setNeedsOnboarding(!completed));
  }, []);

  if (needsOnboarding === null) {
    return null;
  }

  if (needsOnboarding) {
    return <OnboardingScreen onFinish={() => setNeedsOnboarding(false)} />;
  }

  if (view.kind === "settings") {
    return <SettingsScreen onBack={() => setView({ kind: "list" })} />;
  }

  if (view.kind === "detail") {
    return (
      <PlaceDetailScreen
        place={view.place}
        onBack={() => setView({ kind: "list" })}
        onRenamed={(newPlaceId, newName) =>
          setView({ kind: "detail", place: { ...view.place, id: newPlaceId, name: newName } })
        }
      />
    );
  }

  return (
    <div className="flex flex-col">
      <div className="flex justify-end p-2">
        <button
          type="button"
          onClick={() => setView({ kind: "settings" })}
          className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
        >
          設定
        </button>
      </div>
      <SearchListScreen
        onSelectPlace={(id) => setView({ kind: "detail", place: { id, name: "", visitCount: 0, lastVisit: "" } })}
      />
    </div>
  );
}
```

- [ ] **Step 8: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test -- App.test.tsx`
Expected: PASS（2テスト）

- [ ] **Step 9: 全体のテストとビルドを確認する**

Run: `cd apps/desktop && npm run test && npm run build`
Expected: PASS・エラーなく終了する

- [ ] **Step 10: コミット**

```bash
git add apps/desktop/src/screens/OnboardingScreen.tsx apps/desktop/src/screens/OnboardingScreen.test.tsx apps/desktop/src/App.tsx apps/desktop/src/App.test.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add first-run onboarding and gate App.tsx behind it

OnboardingScreen lets a new user pick watched photo folders (native
picker), optionally sign in to Google and enable calendar import, and
choose an LLM provider, or skip entirely. App.tsx calls
setup_completed on mount and shows onboarding only when no config
exists yet; skip or finish both leave a valid, non-blank config so the
app never gets stuck showing onboarding again.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 18: 手動スモークチェック（実アプリの起動確認）

**Files:**
- なし（コード変更を伴わない検証タスク）

**Interfaces:**
- Consumes: Task 1〜17 の全成果物
- Produces: なし。ここで見つかった不具合は該当タスクに戻って直す

これまでのタスクはすべて自動テストとビルド確認で進めてきたが、Tauri のネイティブダイアログ・システムブラウザ起動・トレイ・実際の Google OAuth 同意画面は自動化できない。このタスクは実行者（人間、または実行環境で GUI ブラウザ操作が可能なエージェント）が手動で行うこと。**自動化できない旨をここに明記する。**

- [ ] **Step 1: 環境変数を設定する（任意。未設定でも起動はできるが Google 関連は失敗する）**

`AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` を実際の値に設定できる場合は設定する。設定しない場合、Google サインインは「AREITU_GOOGLE_CLIENT_ID is not set at build time」というエラーになることを確認するだけでよい。

- [ ] **Step 2: アプリを起動する**

Run: `cd apps/desktop && npm run tauri dev`
Expected: ウィンドウが起動し、ターミナルに Rust の panic やコンパイルエラーが出ない

- [ ] **Step 3: 初回起動時のオンボーディング画面を確認する**

`config.json` が存在しない状態（初回、またはアプリデータディレクトリを削除した状態）で起動し、次を確認する:
- 「AREITU へようこそ」の見出しが表示される（真っ白な画面にならない）
- 「フォルダを選択」ボタンを押すとネイティブのフォルダ選択ダイアログが開く
- 「スキップ」を押すとオンボーディングが終わり、一覧画面（「場所はまだありません」または既存データ）が表示される

- [ ] **Step 4: 2回目以降の起動でオンボーディングが出ないことを確認する**

Step 3 の後にアプリを再起動し、一覧画面が直接表示される（オンボーディングに戻らない）ことを確認する。

- [ ] **Step 5: 設定画面を確認する**

「設定」ボタンから設定画面を開き、次を確認する:
- 「Google アカウント」区画に「未サインイン」と「Google でサインイン」ボタンが表示される
- 「カレンダー連携」のチェックボックスが操作できる
- 「データの取り込み」区画の「Google タイムラインを取り込む」ボタンを押すとネイティブのファイル選択ダイアログが開く（キャンセルしてよい）
- 画面がクリーム一色の背景・過剰な斜体・意味のない "01/02" のような番号・多用された等幅フォント・角丸ピル型ボタンになっていないこと（構想の否定パターンに抵触していないこと）を目視確認する

- [ ] **Step 6: ターミナルログを確認する**

Step 2〜5 の操作中、ターミナルに Rust の `panic!`・`unwrap` failure・`thread '...' panicked` が出ていないことを確認する。

- [ ] **Step 7: 結果を記録する**

上記で見つかった不具合があれば、該当するタスク（Task 1〜17）に戻って修正し、テストを追加してから再度このタスクをやり直す。問題がなければこのタスクは完了とする。

---

## Self-Review

**1. 仕様カバレッジ:**
- #18 Google Calendar 自動取得（incremental authorization・syncToken 差分・410 時のフルシンク・`parse_events` への合流）→ Task 2, 4, 5, 6, 8, 9
- #19 初回セットアップ画面（フォルダ選択・Google サインイン・カレンダートグル・LLM 選択・スキップ可）→ Task 17
- #19 Google アカウント UI の欠落分（サインイン/サインアウト/状態/今すぐ Drive 同期）→ Task 12
- #20 Google タイムライン取り込み（オンデバイス export・レガシー Takeout の両形式、inline JSON フィクスチャ）→ Task 13, 14, 16
- クリーンアップ（`greet` 削除・`App.css` 削除・edition 2024 統一・キーチェーン「未設定」と「読めない」の区別）→ Task 1, 7
- 手動スモークチェック → Task 18

**2. プレースホルダ検査:** 「TBD」「後で実装」「適切なエラーハンドリングを追加」等のパターンは含まれていない。各タスクのコードはすべて具体的な実装・テストコードで書かれている。

**3. 型の一貫性:** `CalendarIngestSummary`（Task 6）→ `ingest_calendar`（Task 8）→ `SyncSummary.calendar_synced/calendar_removed/calendar_errors`（Task 9）→ `SyncSummary`（フロント、Task 11）の型名・フィールド名は一貫させた。`AppConfig.calendar_enabled`（Task 7）は `google.rs::calendar_scope_for`（Task 8）・`SettingsDto`/`SaveSettingsDto.calendarEnabled`（Task 10）・フロント `Settings.calendarEnabled`（Task 11, 17）まで同じ意味・同じ camelCase 変換で通している。`KeyStatus`（Task 7 で定義、Task 10 で DTO に採用、Task 11 でフロント型に反映）も同様。`Source::Timeline`（Task 13）→ `timeline::to_raw_log`（Task 14）→ `cluster()` のヒント化（Task 13）まで一貫。

**4. Review Focus の網羅:** 冒頭の5項目はそれぞれ Task 8（未許可でのカレンダー取得）・Task 4/6（410 時のフルシンク）・Task 5/6（キャンセル済みイベントの安全な削除）・Task 14（複数エクスポート形式）・Task 17/18（オンボーディングのスキップ・失敗時も使える状態を保つ）で、対応するテストを該当タスク内に明記した。
