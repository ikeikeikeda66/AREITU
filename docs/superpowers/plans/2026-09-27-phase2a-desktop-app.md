# Phase 2A: デスクトップアプリ (Tauri + React) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `areitu-core` を利用する Tauri 2 + React デスクトップアプリを `apps/desktop` に作り、場所の検索・詳細表示・名前修正・同期実行と、トレイ常駐・自動起動・定期ポーリング・設定画面（監視フォルダ・LLM プロバイダ・API キー）を提供する。

**Architecture:** `apps/desktop/src-tauri` を Cargo ワークスペースに追加し `areitu-core` に依存させる。Tauri コマンドは薄いラッパーとし、実処理は `cargo test` だけで検証できる素の Rust 関数（`logic.rs` / `sync.rs` / `config.rs`）に置く。フロントエンドは React + TypeScript + Vite + Tailwind で、`@tauri-apps/api` の `invoke` を薄いラッパー関数越しに呼ぶ。秘密情報（LLM API キー）は OS キーチェーンに `keyring` crate 経由で保存し、非秘密設定（監視フォルダ・プロバイダ選択・ポーリング間隔など）はアプリ設定ディレクトリの JSON ファイルに保存する。バックグラウンド同期（定期ポーリング、トレイの「今すぐ同期」）と手動の `sync_now` コマンドは同じ `rusqlite::Connection` を `Mutex` で共有し、同じ `run_sync` 関数を呼ぶことで重複実装を避ける。

**Tech Stack:** Rust (stable, edition 2024) / Tauri 2 / tauri-plugin-autostart / keyring / reqwest (blocking, json) / rusqlite / React 18 + TypeScript + Vite / Tailwind CSS / Vitest + @testing-library/react

**Spec:** AREITU 基本構想書 https://docs.google.com/document/d/1OPiPQLFQdxd31uZOQzWQORTyrvs1roQS_XW_ykYPj6E/edit 、および GitHub issues #11–#15。全体ロードマップ: `docs/superpowers/plans/2026-09-26-roadmap.md`。Phase 1 の実装と設計判断: `docs/superpowers/plans/2026-09-26-phase1-core-engine.md`（`crates/areitu-core` の全 API はこの Phase 1 で確定済みで、本計画はその上に積む）。

## Global Constraints

- ライセンス: MIT License
- Rust: stable、edition 2024（`crates/areitu-core`, `crates/areitu-cli` は既にこの edition）
- 配布対象: Mac・Windows。CI は ubuntu / macos / windows の3 OS で `cargo test --workspace` と `cargo clippy --workspace --all-targets -- -D warnings` を通す（clippy 警告ゼロを維持）
- DB は単一ファイル。パスは Tauri のアプリデータディレクトリ配下 `areitu.db`
- 非秘密設定（監視フォルダ・LLM プロバイダ選択・モデル名・Ollama URL・Google Places 利用フラグ・ポーリング間隔・最小信頼度）はアプリ設定ディレクトリの `config.json` に保存する
- 秘密情報（OpenAI / Gemini / Google Places の API キー）はファイルに書かず、`keyring` crate で OS キーチェーンに保存する
- Tauri コマンドハンドラは薄いラッパーとし、実処理は Tauri ランタイムなしで `cargo test` できる素の Rust 関数に置く
- フロントエンドのテストは Vitest + @testing-library/react。`@tauri-apps/api/core` の `invoke` はテストでモックする（実プロセス呼び出しはしない）
- 推論順序は Phase 1 と同じ: ユーザー辞書 → （Nominatim または Google Places の）逆ジオコーディング → LLM（フォールバック）
- Google OAuth・Google Drive 連携は別計画（Phase 2B）。設定画面には「Google アカウント」セクションの見出しだけを置き、ロジックは実装しない
- UI で避けるパターン: 単色の off-white/クリーム背景、過剰なイタリック、意味のない "01/02" のようなセクション番号、等幅フォントの多用、角丸ピル型ボタン
- コミットメッセージは空行の後に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` を付ける
- Rust 側のプロジェクト新規作成は `cargo new --vcs none` 相当（ネストした `.git` を作らない）。フロントエンドのスキャフォルドも同様にネストした `.git` を残さないことを各タスクの手順で確認する
- 正規表現・複雑なパターンでの検索には `/usr/bin/grep` を直接使う（このリポジトリでは RTK フックが `grep` を書き換えるため）

## Review Focus

1. 検索ボックスに連続して文字を入力したとき、遅れて返ってきた古いリクエストの結果が新しい入力の結果を上書きしない（Task 8 でテスト）
2. 場所の名前を空白だけにリネームしようとしたとき、UI がクラッシュせずエラーメッセージを表示し、入力欄の文字は失われない（Task 9 でテスト）
3. 「今すぐ同期」を連続でクリックする、または自動ポーリングと手動同期が同時に走っても、DB 破損やデッドロックが起きない（Task 4, Task 10 でテスト）
4. 監視フォルダのうち一つが削除・アクセス不能になっていても、残りのフォルダの同期は継続し、失敗件数として報告される（Task 4 でテスト）
5. LLM プロバイダを選択したのに API キーが未設定、または Ollama の URL が起動していない場合でも、同期は落ちず Nominatim / Google Places の結果に縮退する（Task 11 でテスト）

## File Structure

```
Cargo.toml                                    workspace（メンバー追加）
.github/workflows/ci.yml                      フロントエンドテスト・Tauri 依存を追加
README.md                                     デスクトップアプリの起動手順を追加
crates/areitu-core/src/resolve/llm.rs         OpenAi, Gemini の LlmClient 実装を追加
crates/areitu-core/src/resolve/geocode.rs     GooglePlaces の ReverseGeocoder 実装を追加
apps/desktop/
  package.json                                 React/Vite/Tailwind/Vitest
  vite.config.ts
  tsconfig.json
  index.html
  src/
    main.tsx
    App.tsx
    styles.css                                 Tailwind エントリ
    api/tauri.ts                                invoke の薄いラッパー
    api/types.ts                                共有型（Place, Visit, Settings 等）
    screens/SearchListScreen.tsx                #12
    screens/PlaceDetailScreen.tsx                #13
    screens/SettingsScreen.tsx                   #15
    components/SearchBar.tsx
    components/SortToggle.tsx
    components/PlaceListItem.tsx
    test/setup.ts                                Vitest セットアップ（invoke モック共通化）
    screens/SearchListScreen.test.tsx
    screens/PlaceDetailScreen.test.tsx
  src-tauri/
    Cargo.toml
    tauri.conf.json
    build.rs
    icons/                                       スキャフォルドが生成する既定アイコン
    src/
      main.rs                                    エントリポイント、AppState、コマンド登録、tray/poll 起動
      commands.rs                                 #[tauri::command] の薄いラッパー
      logic.rs                                    list_places/visits_of/rename_place の DTO 変換ロジック（Tauri 非依存）
      config.rs                                   AppConfig, SecretStore, KeyringSecretStore, FakeSecretStore, load/save
      sync.rs                                     run_sync, run_sync_with_config, build_geocoder/build_llm, ポーリングスレッド
      tray.rs                                     トレイアイコンとメニュー
```

---

### Task 1: モノレポ骨格 — Tauri 2 + React + Vite + TS + Tailwind スキャフォルド

Tauri アプリの土台を作り、`src-tauri` を Cargo ワークスペースに加え、`areitu-core` に依存させる。GUI を実際に起動するテストはしないが、`cargo build --workspace` と `npm run build` が通ることを「テスト」として確認する。

**Files:**
- Create: `apps/desktop/` 一式（`npm create tauri-app` の出力、後述の手順で調整）
- Modify: `Cargo.toml`（workspace members に `apps/desktop/src-tauri` を追加）
- Modify: `apps/desktop/src-tauri/Cargo.toml`（パッケージ名を `areitu-desktop` にし、`areitu-core` を path 依存に追加）

**Interfaces:**
- Produces: ワークスペースメンバー `apps/desktop/src-tauri`（パッケージ名 `areitu-desktop`）。以降のタスクはこのクレートに `src/*.rs` を追加していく
- Produces: フロントエンドの Vite プロジェクト（`apps/desktop`、npm スクリプト `dev` / `build` / `test`）

- [ ] **Step 1: スキャフォルドを生成する**

リポジトリルートで実行する。

```bash
npm create tauri-app@latest apps-desktop-tmp
```

対話プロンプトが出た場合は次のように答える（バージョンによって質問文言や非対話フラグ名が変わるため、フラグでの一括指定が失敗したら対話で答えること）:
- Project name: `desktop`
- Identifier: `com.areitu.desktop`
- Choose which language to use for your frontend: `TypeScript / JavaScript`
- Choose your package manager: `npm`
- Choose your UI template: `React`
- Choose your UI flavor: `TypeScript`

Expected: `apps-desktop-tmp/` に `src/`（React+Vite）と `src-tauri/`（Rust）が生成される。

- [ ] **Step 2: 生成物を `apps/desktop` に移動し、ネストした `.git` を除去する**

```bash
rm -rf apps-desktop-tmp/.git
mkdir -p apps/desktop
mv apps-desktop-tmp/* apps-desktop-tmp/.[!.]* apps/desktop/ 2>/dev/null
rmdir apps-desktop-tmp
ls apps/desktop/src-tauri
```

Expected: `apps/desktop/.git` が存在しない。`apps/desktop/src-tauri/Cargo.toml` と `apps/desktop/package.json` が存在する。

- [ ] **Step 3: `src-tauri` パッケージ名を統一し、`areitu-core` を依存に加える**

`apps/desktop/src-tauri/Cargo.toml` を読み、`[package] name = "..."` を `areitu-desktop` に変更する。次に `areitu-core` への path 依存を追加する。

```bash
cd apps/desktop/src-tauri
cargo add areitu-core --path ../../../crates/areitu-core
```

Expected: `apps/desktop/src-tauri/Cargo.toml` の `[dependencies]` に `areitu-core = { path = "../../../crates/areitu-core" }` の行が追加される。

- [ ] **Step 4: ルートの Cargo workspace にメンバーを追加する**

`/Users/ikedashinichi/AREITU/Cargo.toml` を次のように変更する。

```toml
[workspace]
resolver = "2"
members = ["crates/areitu-core", "crates/areitu-cli", "apps/desktop/src-tauri"]
```

- [ ] **Step 5: ワークスペースがビルドできることを確認する**

Run: `cargo build --workspace`
Expected: `Compiling areitu-desktop ...` を含み、エラーなく終了する（`Finished` が出力される）。

- [ ] **Step 6: Tailwind を導入する**

```bash
cd apps/desktop
npm install
npm install tailwindcss @tailwindcss/vite
```

`apps/desktop/vite.config.ts` に Tailwind の Vite プラグインを追加する。

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
```

`apps/desktop/src/styles.css` を作成する。

```css
@import "tailwindcss";
```

`apps/desktop/src/main.tsx` の先頭で `styles.css` を import する（既存の CSS import 行を置き換える）。

```ts
import "./styles.css";
```

もし解決した `tailwindcss` のバージョンが v4 系の Vite プラグインを持たない v3 系だった場合は、代わりに `npx tailwindcss init -p` を実行して `tailwind.config.js` / `postcss.config.js` を生成し、`styles.css` の先頭を `@tailwind base; @tailwind components; @tailwind utilities;` に変更する（v3 と v4 のどちらが解決されたかは `npm ls tailwindcss` で確認する）。

- [ ] **Step 7: フロントエンドがビルドできることを確認する**

Run: `cd apps/desktop && npm run build`
Expected: `dist/` が生成され、エラーなく終了する。

- [ ] **Step 8: Vitest を導入する**

```bash
cd apps/desktop
npm install -D vitest @testing-library/react @testing-library/jest-dom jsdom
```

`apps/desktop/vite.config.ts` に `test` ブロックを追加する。

```ts
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});
```

`apps/desktop/src/test/setup.ts` を作成する。

```ts
import "@testing-library/jest-dom/vitest";
```

`apps/desktop/package.json` の `scripts` に追加する。

```json
"test": "vitest run"
```

- [ ] **Step 9: Vitest が空のスイートでも動くことを確認する**

Run: `cd apps/desktop && npm run test`
Expected: `No test files found` またはテスト0件のまま正常終了（exit code 0）。後続タスクでテストファイルを追加していく。

- [ ] **Step 10: commit**

```bash
git add Cargo.toml apps/desktop
git commit -m "$(cat <<'EOF'
feat: scaffold Tauri 2 + React + Tailwind desktop app in apps/desktop

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: AppState・DB パスと、場所一覧・詳細・リネームの3コマンド

`areitu-core::db::open` で開いた `Connection` を `Mutex` に入れて `tauri::State` として管理し、DB パスを Tauri のアプリデータディレクトリ配下 `areitu.db` にする。`list_places` / `visits_of` / `rename_place` を、DTO 変換をする素の関数（`logic.rs`）＋薄いコマンド（`commands.rs`）の形で実装する。

**Files:**
- Create: `apps/desktop/src-tauri/src/logic.rs`
- Create: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: `areitu_core::db::open(&Path) -> areitu_core::Result<rusqlite::Connection>`、`areitu_core::query::{list_places, visits_of, SortBy}`、`areitu_core::store::rename_place`
- Produces: `pub struct AppState { pub conn: std::sync::Mutex<rusqlite::Connection>, pub config_path: std::path::PathBuf }`（`main.rs`）
- Produces: `logic::list_places_dto(conn: &rusqlite::Connection, sort: &str, keyword: Option<&str>) -> Result<Vec<PlaceDto>, String>`
- Produces: `logic::visits_of_dto(conn: &rusqlite::Connection, place_id: i64) -> Result<Vec<VisitDto>, String>`
- Produces: `logic::rename_place_dto(conn: &mut rusqlite::Connection, place_id: i64, name: &str) -> Result<i64, String>`
- Produces: `#[derive(serde::Serialize)] pub struct PlaceDto { pub id: i64, pub name: String, pub visit_count: i64, pub last_visit: String }`
- Produces: `#[derive(serde::Serialize)] pub struct VisitDto { pub started_at: String, pub ended_at: String }`
- Produces: Tauri コマンド `list_places`, `visits_of`, `rename_place`（`commands.rs`、`main.rs` の `invoke_handler` に登録）

- [ ] **Step 1: 依存を追加する**

```bash
cd apps/desktop/src-tauri
cargo add serde --features derive
cargo add chrono
```

- [ ] **Step 2: `logic.rs` を作成し、失敗するテストを書く**

`apps/desktop/src-tauri/src/logic.rs`:

```rust
use areitu_core::query::{list_places, visits_of, SortBy};
use areitu_core::store::rename_place;
use rusqlite::Connection;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaceDto {
    pub id: i64,
    pub name: String,
    pub visit_count: i64,
    pub last_visit: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VisitDto {
    pub started_at: String,
    pub ended_at: String,
}

const DATETIME_FMT: &str = "%Y-%m-%dT%H:%M:%S";

fn parse_sort(sort: &str) -> Result<SortBy, String> {
    match sort {
        "count" => Ok(SortBy::Count),
        "recent" => Ok(SortBy::Recent),
        other => Err(format!("unknown sort: {other}")),
    }
}

pub fn list_places_dto(
    conn: &Connection,
    sort: &str,
    keyword: Option<&str>,
) -> Result<Vec<PlaceDto>, String> {
    let sort_by = parse_sort(sort)?;
    list_places(conn, sort_by, keyword)
        .map(|places| {
            places
                .into_iter()
                .map(|p| PlaceDto {
                    id: p.id,
                    name: p.name,
                    visit_count: p.visit_count,
                    last_visit: p.last_visit.format(DATETIME_FMT).to_string(),
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

pub fn visits_of_dto(conn: &Connection, place_id: i64) -> Result<Vec<VisitDto>, String> {
    visits_of(conn, place_id)
        .map(|visits| {
            visits
                .into_iter()
                .map(|(started_at, ended_at)| VisitDto {
                    started_at: started_at.format(DATETIME_FMT).to_string(),
                    ended_at: ended_at.format(DATETIME_FMT).to_string(),
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

pub fn rename_place_dto(conn: &mut Connection, place_id: i64, name: &str) -> Result<i64, String> {
    rename_place(conn, place_id, name).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use areitu_core::db::open_in_memory;
    use areitu_core::store::{find_or_create_place, insert_visit};
    use areitu_core::testutil_ext::candidate;

    fn seed_place(conn: &Connection, name: &str, at: &str) -> i64 {
        let id = find_or_create_place(conn, name, 35.0, 139.0).unwrap();
        let mut cand = candidate(&[]);
        let t = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap();
        cand.started_at = t;
        cand.ended_at = t;
        insert_visit(conn, id, &cand, "nominatim").unwrap();
        id
    }

    #[test]
    fn list_places_dto_maps_fields() {
        let c = open_in_memory().unwrap();
        seed_place(&c, "カフェ丸の内", "2026-09-01 12:00");
        let places = list_places_dto(&c, "count", None).unwrap();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].name, "カフェ丸の内");
        assert_eq!(places[0].visit_count, 1);
        assert_eq!(places[0].last_visit, "2026-09-01T12:00:00");
    }

    #[test]
    fn list_places_dto_rejects_unknown_sort() {
        let c = open_in_memory().unwrap();
        assert!(list_places_dto(&c, "bogus", None).is_err());
    }

    #[test]
    fn visits_of_dto_is_newest_first() {
        let c = open_in_memory().unwrap();
        let id = seed_place(&c, "A", "2026-01-01 12:00");
        let mut cand = candidate(&[]);
        let t = chrono::NaiveDateTime::parse_from_str("2026-09-01 12:00", "%Y-%m-%d %H:%M").unwrap();
        cand.started_at = t;
        cand.ended_at = t;
        insert_visit(&c, id, &cand, "nominatim").unwrap();
        let visits = visits_of_dto(&c, id).unwrap();
        assert_eq!(visits[0].started_at, "2026-09-01T12:00:00");
    }

    #[test]
    fn rename_place_dto_rejects_blank_name() {
        let mut c = open_in_memory().unwrap();
        let id = seed_place(&c, "A", "2026-09-01 12:00");
        assert!(rename_place_dto(&mut c, id, "   ").is_err());
    }
}
```

このテストは `areitu_core::testutil_ext::candidate` を参照しているが、この関数はまだ存在しない（`areitu-core` の `testutil` は `pub(crate)` で外部クレートから使えないため、テスト専用の小さな公開ヘルパーを別途用意する必要がある）。次のステップで先に `areitu-core` 側にこのヘルパーを追加する。

- [ ] **Step 3: `areitu-core` にテスト専用の公開ヘルパーを追加する**

`crates/areitu-core/src/lib.rs` の `pub mod store;` の下に追加する。

```rust
/// 外部クレート（Tauri バックエンドなど）のテストから `VisitCandidate` を組み立てるための
/// 最小限の公開ヘルパー。本体ロジックは含まない。
#[cfg(any(test, feature = "test-util"))]
pub mod testutil_ext {
    use crate::cluster::VisitCandidate;

    pub fn candidate(hints: &[&str]) -> VisitCandidate {
        let t = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        VisitCandidate {
            started_at: t("2026-09-01 12:00"),
            ended_at: t("2026-09-01 12:45"),
            lat: 35.0,
            lon: 139.0,
            log_ids: vec![],
            hints: hints.iter().map(|s| s.to_string()).collect(),
        }
    }
}
```

`crates/areitu-core/Cargo.toml` に feature を追加する。

```toml
[features]
test-util = []
```

`apps/desktop/src-tauri/Cargo.toml` の `areitu-core` 依存に `features = ["test-util"]` を付ける。

```bash
cd apps/desktop/src-tauri
cargo add areitu-core --path ../../../crates/areitu-core --features test-util
```

Expected: `Cargo.toml` の `areitu-core` 行が `areitu-core = { path = "../../../crates/areitu-core", features = ["test-util"] }` になる。

- [ ] **Step 4: `logic.rs` のテストを走らせて失敗を確認する**

Run: `cd apps/desktop/src-tauri && cargo test logic::`
Expected: FAIL（`logic` モジュールがまだ `main.rs` から `mod logic;` されておらずコンパイルエラー、または `cargo build` 自体が通らない）

- [ ] **Step 5: `commands.rs` と `main.rs` を実装する**

`apps/desktop/src-tauri/src/commands.rs`:

```rust
use tauri::State;

use crate::logic::{list_places_dto, rename_place_dto, visits_of_dto, PlaceDto, VisitDto};
use crate::AppState;

#[tauri::command]
pub fn list_places(
    state: State<AppState>,
    sort: String,
    keyword: Option<String>,
) -> Result<Vec<PlaceDto>, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    list_places_dto(&conn, &sort, keyword.as_deref())
}

#[tauri::command]
pub fn visits_of(state: State<AppState>, place_id: i64) -> Result<Vec<VisitDto>, String> {
    let conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    visits_of_dto(&conn, place_id)
}

#[tauri::command]
pub fn rename_place(state: State<AppState>, place_id: i64, name: String) -> Result<i64, String> {
    let mut conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    rename_place_dto(&mut conn, place_id, &name)
}
```

`apps/desktop/src-tauri/src/main.rs`（スキャフォルドが生成した内容を次のように置き換える）:

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod logic;

use std::sync::Mutex;

use tauri::Manager;

pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub config_path: std::path::PathBuf,
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            let conn = areitu_core::db::open(&data_dir.join("areitu.db"))
                .expect("failed to open areitu.db");

            let config_dir = app
                .path()
                .app_config_dir()
                .expect("failed to resolve app config dir");
            std::fs::create_dir_all(&config_dir).expect("failed to create app config dir");
            let config_path = config_dir.join("config.json");

            app.manage(AppState { conn: Mutex::new(conn), config_path });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: テストを実行して通ることを確認する**

Run: `cd apps/desktop/src-tauri && cargo test logic::`
Expected: PASS（4 件のテストが成功する）

- [ ] **Step 7: clippy を確認する**

Run: `cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: warning ゼロで終了する

- [ ] **Step 8: commit**

```bash
git add crates/areitu-core apps/desktop/src-tauri
git commit -m "$(cat <<'EOF'
feat: add AppState, DB path resolution, and list_places/visits_of/rename_place commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: 設定ファイルとシークレットストア（`config.rs`）

非秘密設定を JSON ファイルに、秘密情報（API キー）を OS キーチェーンに保存する仕組みを作る。`SecretStore` トレイトで抽象化し、テストでは実キーチェーンに触れない `FakeSecretStore` を使う。

**Files:**
- Create: `apps/desktop/src-tauri/src/config.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`（`mod config;` を追加）
- Modify: `apps/desktop/src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)] #[serde(rename_all = "snake_case")] pub enum LlmProvider { None, Ollama, OpenAi, Gemini }`
- Produces: `#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)] pub struct AppConfig { pub watched_dirs: Vec<String>, pub llm_provider: LlmProvider, pub ollama_url: String, pub ollama_model: String, pub openai_model: String, pub gemini_model: String, pub google_places_enabled: bool, pub min_confidence: f64, pub poll_interval_minutes: u32 }`（`Default` 実装あり）
- Produces: `pub fn load_config(path: &std::path::Path) -> AppConfig`（読めない・壊れている場合は `AppConfig::default()`）
- Produces: `pub fn save_config(path: &std::path::Path, config: &AppConfig) -> std::io::Result<()>`
- Produces: `pub trait SecretStore { fn get(&self, key: &str) -> Option<String>; fn set(&self, key: &str, value: &str) -> Result<(), String>; fn delete(&self, key: &str) -> Result<(), String>; }`
- Produces: `pub const OPENAI_KEY: &str = "openai_api_key";` / `pub const GEMINI_KEY: &str = "gemini_api_key";` / `pub const GOOGLE_PLACES_KEY: &str = "google_places_api_key";`
- Produces: `pub struct KeyringSecretStore;`（`SecretStore` 実装、`keyring::Entry::new("AREITU", key)` を使う）
- Consumes (Task 4以降): 上記すべて

- [ ] **Step 1: 依存を追加する**

```bash
cd apps/desktop/src-tauri
cargo add serde_json
cargo add keyring
cargo add tempfile --dev
```

- [ ] **Step 2: 失敗するテストを書く**

`apps/desktop/src-tauri/src/config.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    None,
    Ollama,
    OpenAi,
    Gemini,
}

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
        }
    }
}

pub fn load_config(path: &Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(path: &Path, config: &AppConfig) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(config).expect("AppConfig is always serializable");
    std::fs::write(path, json)
}

pub const OPENAI_KEY: &str = "openai_api_key";
pub const GEMINI_KEY: &str = "gemini_api_key";
pub const GOOGLE_PLACES_KEY: &str = "google_places_api_key";
const SERVICE: &str = "AREITU";

pub trait SecretStore {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, key).ok()?.get_password().ok()
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

#[cfg(test)]
pub struct FakeSecretStore(pub std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl FakeSecretStore {
    pub fn new() -> Self {
        FakeSecretStore(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

#[cfg(test)]
impl SecretStore for FakeSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(load_config(&path), AppConfig::default());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_config(&path), AppConfig::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut config = AppConfig::default();
        config.watched_dirs.push("/photos".to_owned());
        config.llm_provider = LlmProvider::OpenAi;
        config.poll_interval_minutes = 15;
        save_config(&path, &config).unwrap();
        assert_eq!(load_config(&path), config);
    }

    #[test]
    fn fake_secret_store_set_get_delete() {
        let store = FakeSecretStore::new();
        assert_eq!(store.get(OPENAI_KEY), None);
        store.set(OPENAI_KEY, "sk-test").unwrap();
        assert_eq!(store.get(OPENAI_KEY).as_deref(), Some("sk-test"));
        store.delete(OPENAI_KEY).unwrap();
        assert_eq!(store.get(OPENAI_KEY), None);
    }
}
```

- [ ] **Step 3: `main.rs` に `mod config;` を追加する**

`apps/desktop/src-tauri/src/main.rs` の先頭のモジュール宣言に追加する。

```rust
mod commands;
mod config;
mod logic;
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cd apps/desktop/src-tauri && cargo test config::`
Expected: PASS（4 件のテストが成功する）。`FakeSecretStore` を使うテストは実 OS キーチェーンに一切アクセスしない。

- [ ] **Step 5: commit**

```bash
git add apps/desktop/src-tauri
git commit -m "$(cat <<'EOF'
feat: add JSON app config and keyring-backed secret store

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: 同期ロジックと `sync_now` コマンド

写真フォルダの走査（`scan_photos`）と訪問生成（`build_visits`）を、複数フォルダ・部分失敗に対応する形でまとめる `run_sync` を実装する。これは Review Focus の「フォルダが一つ壊れていても続行する」「多重実行しても壊れない」を満たす中核。`sync_now` コマンドはこれを呼ぶだけの薄いラッパー。

**Files:**
- Create: `apps/desktop/src-tauri/src/sync.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`

**Interfaces:**
- Consumes: `areitu_core::scan::scan_photos`, `areitu_core::pipeline::build_visits`, `areitu_core::resolve::{Resolver, geocode::ReverseGeocoder, llm::LlmClient}`
- Produces: `#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)] pub struct SyncSummary { pub scanned: usize, pub scan_errors: Vec<String>, pub visits_created: usize, pub resolve_failed: usize }`
- Produces: `pub fn run_sync(conn: &mut rusqlite::Connection, dirs: &[String], geocoder: &dyn ReverseGeocoder, llm: Option<&dyn LlmClient>, min_confidence: f64) -> Result<SyncSummary, String>`
- Produces（Task 10, 11 が使う）: `pub fn run_sync_with_config(conn: &mut rusqlite::Connection, config: &crate::config::AppConfig, secrets: &dyn crate::config::SecretStore) -> Result<SyncSummary, String>`（Task 11 で `build_geocoder`/`build_llm` を実装してから完成させる。本タスクでは Nominatim 固定・LLM なしの最小実装を置く）
- Produces: Tauri コマンド `sync_now`

- [ ] **Step 1: `run_sync` の失敗するテストを書く**

`apps/desktop/src-tauri/src/sync.rs`:

```rust
use std::path::Path;

use areitu_core::pipeline::build_visits;
use areitu_core::resolve::geocode::ReverseGeocoder;
use areitu_core::resolve::llm::LlmClient;
use areitu_core::resolve::Resolver;
use areitu_core::scan::scan_photos;
use rusqlite::Connection;

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct SyncSummary {
    pub scanned: usize,
    pub scan_errors: Vec<String>,
    pub visits_created: usize,
    pub resolve_failed: usize,
}

pub fn run_sync(
    conn: &mut Connection,
    dirs: &[String],
    geocoder: &dyn ReverseGeocoder,
    llm: Option<&dyn LlmClient>,
    min_confidence: f64,
) -> Result<SyncSummary, String> {
    let mut summary = SyncSummary::default();
    for dir in dirs {
        match scan_photos(conn, Path::new(dir)) {
            Ok(report) => summary.scanned += report.inserted,
            Err(e) => summary.scan_errors.push(format!("{dir}: {e}")),
        }
    }
    let resolver = Resolver { geocoder, llm, min_confidence };
    let report = build_visits(conn, &resolver).map_err(|e| e.to_string())?;
    summary.visits_created = report.visits;
    summary.resolve_failed = report.failed;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use areitu_core::db::open_in_memory;
    use areitu_core::resolve::geocode::PoiGuess;
    use areitu_core::testutil_ext::candidate;
    use areitu_core::{Error, Result};

    struct FakeGeocoder(Option<PoiGuess>);
    impl ReverseGeocoder for FakeGeocoder {
        fn reverse(&self, _lat: f64, _lon: f64) -> Result<Option<PoiGuess>> {
            Ok(self.0.clone())
        }
    }

    fn geo(name: &str) -> FakeGeocoder {
        FakeGeocoder(Some(PoiGuess {
            name: Some(name.to_owned()),
            display_name: name.to_owned(),
            category: None,
        }))
    }

    fn photo_dir_with_no_photos() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn missing_dir_is_reported_but_does_not_abort_other_dirs() {
        let mut c = open_in_memory().unwrap();
        let good = photo_dir_with_no_photos();
        let g = geo("カフェ丸の内");
        let dirs = vec!["/no/such/dir/areitu".to_owned(), good.path().to_string_lossy().into_owned()];
        let summary = run_sync(&mut c, &dirs, &g, None, 0.6).unwrap();
        assert_eq!(summary.scan_errors.len(), 1);
        assert!(summary.scan_errors[0].starts_with("/no/such/dir/areitu"));
        assert_eq!(summary.visits_created, 0);
    }

    #[test]
    fn empty_dirs_list_still_builds_visits_from_calendar_ingest() {
        let mut c = open_in_memory().unwrap();
        // カレンダー取り込み済みの未処理ログを1件だけ模擬する
        areitu_core::store::upsert_raw_log(
            &c,
            &areitu_core::model::RawLog {
                source: areitu_core::model::Source::Calendar,
                source_id: "e1".into(),
                occurred_at: candidate(&[]).started_at,
                ended_at: Some(candidate(&[]).ended_at),
                lat: Some(35.0),
                lon: Some(139.0),
                text: Some("ランチ".into()),
            },
        )
        .unwrap();
        let g = geo("カフェ丸の内");
        let summary = run_sync(&mut c, &[], &g, None, 0.6).unwrap();
        assert_eq!(summary.scanned, 0);
        assert_eq!(summary.visits_created, 1);
    }

    #[test]
    fn concurrent_calls_do_not_deadlock() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let shared = Arc::new(Mutex::new(open_in_memory().unwrap()));
        let g = Arc::new(geo("カフェ丸の内"));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let shared = Arc::clone(&shared);
            let g = Arc::clone(&g);
            handles.push(thread::spawn(move || {
                let mut conn = shared.lock().unwrap();
                run_sync(&mut conn, &[], g.as_ref(), None, 0.6).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // デッドロックせずに全スレッドが完了すればテストは成功
    }
}
```

- [ ] **Step 2: `main.rs` に `mod sync;` を追加する**

```rust
mod commands;
mod config;
mod logic;
mod sync;
```

- [ ] **Step 3: テストを実行して通ることを確認する**

Run: `cd apps/desktop/src-tauri && cargo test sync::`
Expected: PASS（3 件のテストが成功する）

- [ ] **Step 4: `sync_now` コマンドを実装する**

`apps/desktop/src-tauri/src/commands.rs` に追加する。

```rust
use crate::config::{load_config, KeyringSecretStore};
use crate::sync::{run_sync, SyncSummary};
use areitu_core::resolve::geocode::Nominatim;

const USER_AGENT: &str = concat!(
    "AREITU-desktop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

#[tauri::command]
pub fn sync_now(state: State<AppState>) -> Result<SyncSummary, String> {
    let config = load_config(&state.config_path);
    let geocoder = Nominatim::new(USER_AGENT).map_err(|e| e.to_string())?;
    let mut conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    // LLM プロバイダの切り替えは Task 11 (build_geocoder/build_llm) で完成させる。
    // ここでは Nominatim 固定・LLM 無しで動作する最小実装。
    let _ = KeyringSecretStore; // Task 11 で使用する
    run_sync(&mut conn, &config.watched_dirs, &geocoder, None, config.min_confidence)
}
```

`main.rs` の `invoke_handler!` に `commands::sync_now` を追加する。

```rust
.invoke_handler(tauri::generate_handler![
    commands::list_places,
    commands::visits_of,
    commands::rename_place,
    commands::sync_now,
])
```

- [ ] **Step 5: ビルドを確認する**

Run: `cd apps/desktop/src-tauri && cargo build && cargo clippy --all-targets -- -D warnings`
Expected: エラー・warning なしで終了する

- [ ] **Step 6: commit**

```bash
git add apps/desktop/src-tauri
git commit -m "$(cat <<'EOF'
feat: add run_sync (multi-dir, partial-failure-tolerant) and sync_now command

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `areitu-core` — OpenAI の `LlmClient` 実装

Chat Completions API（JSON レスポンス強制）で `LlmClient` を実装する。リクエスト組み立てとレスポンス解析を純粋関数に分け、実ネットワークなしでテストする。

**Files:**
- Modify: `crates/areitu-core/src/resolve/llm.rs`（末尾、145行目の後に追記）

**Interfaces:**
- Consumes: 既存の `trait LlmClient { fn complete_json(&self, prompt: &str) -> Result<String>; }`
- Produces: `pub fn openai_request_body(model: &str, prompt: &str) -> serde_json::Value`
- Produces: `pub fn parse_openai_response(json: &str) -> Result<String>`
- Produces: `pub struct OpenAi { .. }`（`pub fn new(api_key: &str, model: &str) -> Result<OpenAi>`、`impl LlmClient for OpenAi`）

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/resolve/llm.rs` の既存 `mod tests` ブロックの直前（145行目、ファイル末尾）に追記する。

```rust
pub fn openai_request_body(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "response_format": {"type": "json_object"},
    })
}

pub fn parse_openai_response(json: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Response {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(Deserialize)]
    struct Message {
        content: String,
    }
    let resp: Response = serde_json::from_str(json)?;
    resp.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| Error::Invalid("openai: no choices in response".into()))
}

pub struct OpenAi {
    client: reqwest::blocking::Client,
    api_key: String,
    model: String,
}

impl OpenAi {
    pub fn new(api_key: &str, model: &str) -> Result<OpenAi> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(OpenAi { client, api_key: api_key.to_owned(), model: model.to_owned() })
    }
}

impl LlmClient for OpenAi {
    fn complete_json(&self, prompt: &str) -> Result<String> {
        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&openai_request_body(&self.model, prompt))
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("openai status {}", resp.status())));
        }
        parse_openai_response(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}
```

`crates/areitu-core/src/resolve/llm.rs` の既存 `mod tests` の中（`use super::*; use crate::testutil::candidate;` の直後）に、次のテストを追加する。

```rust
    #[test]
    fn openai_request_body_has_json_response_format() {
        let body = openai_request_body("gpt-4o-mini", "こんにちは");
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "こんにちは");
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn parses_openai_chat_completion_response() {
        let json = r#"{"choices":[{"message":{"content":"{\"name\":\"カフェ丸の内\",\"confidence\":0.9}"}}]}"#;
        let content = parse_openai_response(json).unwrap();
        let answer = parse_answer(&content).unwrap();
        assert_eq!(answer.name, "カフェ丸の内");
        assert_eq!(answer.confidence, 0.9);
    }

    #[test]
    fn openai_response_without_choices_is_error() {
        assert!(parse_openai_response(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn openai_response_broken_json_is_error() {
        assert!(parse_openai_response("not json").is_err());
    }
```

- [ ] **Step 2: テストを実行して通ることを確認する**

Run: `cd crates/areitu-core && cargo test resolve::llm::`
Expected: PASS（既存テストに加え、新規4件が成功する）。このテストはネットワークに一切アクセスしない（`OpenAi::complete_json` 自体は呼ばず、`openai_request_body` と `parse_openai_response` だけを検証している）。

- [ ] **Step 3: clippy を確認する**

Run: `cd crates/areitu-core && cargo clippy --all-targets -- -D warnings`
Expected: warning ゼロ

- [ ] **Step 4: commit**

```bash
git add crates/areitu-core/src/resolve/llm.rs
git commit -m "$(cat <<'EOF'
feat: add OpenAI chat completions LlmClient impl to areitu-core

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: `areitu-core` — Gemini の `LlmClient` 実装

`generateContent` API で `LlmClient` を実装する。Task 5 と同じ形で、リクエスト組み立て・レスポンス解析を純粋関数に分ける。

**Files:**
- Modify: `crates/areitu-core/src/resolve/llm.rs`（Task 5 で追記した内容の後に追記）

**Interfaces:**
- Consumes: `trait LlmClient`（既存）
- Produces: `pub fn gemini_request_body(prompt: &str) -> serde_json::Value`
- Produces: `pub fn parse_gemini_response(json: &str) -> Result<String>`
- Produces: `pub struct Gemini { .. }`（`pub fn new(api_key: &str, model: &str) -> Result<Gemini>`、`impl LlmClient for Gemini`）

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/resolve/llm.rs` の `impl LlmClient for OpenAi { .. }` の後（`#[cfg(test)]` の直前）に追記する。

```rust
pub fn gemini_request_body(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
    })
}

pub fn parse_gemini_response(json: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Response {
        candidates: Vec<Candidate>,
    }
    #[derive(Deserialize)]
    struct Candidate {
        content: Content,
    }
    #[derive(Deserialize)]
    struct Content {
        parts: Vec<Part>,
    }
    #[derive(Deserialize)]
    struct Part {
        text: String,
    }
    let resp: Response = serde_json::from_str(json)?;
    resp.candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .ok_or_else(|| Error::Invalid("gemini: no candidates in response".into()))
}

pub struct Gemini {
    client: reqwest::blocking::Client,
    api_key: String,
    model: String,
}

impl Gemini {
    pub fn new(api_key: &str, model: &str) -> Result<Gemini> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Gemini { client, api_key: api_key.to_owned(), model: model.to_owned() })
    }
}

impl LlmClient for Gemini {
    fn complete_json(&self, prompt: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let resp = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&gemini_request_body(prompt))
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("gemini status {}", resp.status())));
        }
        parse_gemini_response(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}
```

テストは既存の `mod tests` ブロックに追加する。

```rust
    #[test]
    fn gemini_request_body_wraps_prompt_as_text_part() {
        let body = gemini_request_body("こんにちは");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "こんにちは");
    }

    #[test]
    fn parses_gemini_generate_content_response() {
        let json = r#"{"candidates":[{"content":{"parts":[{"text":"{\"name\":\"カフェ丸の内\",\"confidence\":0.9}"}]}}]}"#;
        let content = parse_gemini_response(json).unwrap();
        let answer = parse_answer(&content).unwrap();
        assert_eq!(answer.name, "カフェ丸の内");
    }

    #[test]
    fn gemini_response_without_candidates_is_error() {
        assert!(parse_gemini_response(r#"{"candidates":[]}"#).is_err());
    }
```

- [ ] **Step 2: テストを実行して通ることを確認する**

Run: `cd crates/areitu-core && cargo test resolve::llm::`
Expected: PASS（Task 5 の4件 + 新規3件、合計既存分含めすべて成功する）

- [ ] **Step 3: commit**

```bash
git add crates/areitu-core/src/resolve/llm.rs
git commit -m "$(cat <<'EOF'
feat: add Gemini generateContent LlmClient impl to areitu-core

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: `areitu-core` — Google Places (Nearby Search) の `ReverseGeocoder` 実装

Google Places API (New) の Nearby Search で `ReverseGeocoder` を実装する。

**Files:**
- Modify: `crates/areitu-core/src/resolve/geocode.rs`（末尾、148行目の後に追記）

**Interfaces:**
- Consumes: 既存の `trait ReverseGeocoder { fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>>; }`、`struct PoiGuess { name: Option<String>, display_name: String, category: Option<String> }`
- Produces: `pub fn google_places_request_body(lat: f64, lon: f64) -> serde_json::Value`
- Produces: `pub fn parse_google_places_response(json: &str) -> Result<Option<PoiGuess>>`
- Produces: `pub struct GooglePlaces { .. }`（`pub fn new(api_key: &str) -> Result<GooglePlaces>`、`impl ReverseGeocoder for GooglePlaces`）

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/resolve/geocode.rs` の `impl ReverseGeocoder for Nominatim { .. }` の後（`#[cfg(test)]` の直前、148行目付近）に追記する。

```rust
const GOOGLE_PLACES_RADIUS_M: f64 = 50.0;

pub fn google_places_request_body(lat: f64, lon: f64) -> serde_json::Value {
    serde_json::json!({
        "locationRestriction": {
            "circle": {
                "center": {"latitude": lat, "longitude": lon},
                "radius": GOOGLE_PLACES_RADIUS_M,
            }
        },
        "maxResultCount": 1,
    })
}

pub fn parse_google_places_response(json: &str) -> Result<Option<PoiGuess>> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        places: Vec<Place>,
    }
    #[derive(Deserialize)]
    struct Place {
        #[serde(default)]
        display_name: Option<DisplayName>,
        #[serde(default)]
        formatted_address: Option<String>,
        #[serde(default)]
        types: Vec<String>,
    }
    #[derive(Deserialize)]
    struct DisplayName {
        text: String,
    }
    let resp: Response = serde_json::from_str(json)?;
    let Some(place) = resp.places.into_iter().next() else {
        return Ok(None);
    };
    let Some(display_name) = place.formatted_address else {
        return Ok(None);
    };
    Ok(Some(PoiGuess {
        name: place.display_name.map(|d| d.text),
        display_name,
        category: place.types.into_iter().next(),
    }))
}

pub struct GooglePlaces {
    client: reqwest::blocking::Client,
    api_key: String,
}

impl GooglePlaces {
    pub fn new(api_key: &str) -> Result<GooglePlaces> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(GooglePlaces { client, api_key: api_key.to_owned() })
    }
}

impl ReverseGeocoder for GooglePlaces {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>> {
        let resp = self
            .client
            .post("https://places.googleapis.com/v1/places:searchNearby")
            .header("X-Goog-Api-Key", &self.api_key)
            .header(
                "X-Goog-FieldMask",
                "places.displayName,places.formattedAddress,places.types",
            )
            .json(&google_places_request_body(lat, lon))
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("google places status {}", resp.status())));
        }
        parse_google_places_response(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}
```

`serde` の `rename_all = "camelCase"` を構造体に付ける必要がある点に注意する（Google Places API のフィールド名は `displayName` / `formattedAddress` のようにキャメルケース）。上の `Response` / `Place` / `DisplayName` の直前にそれぞれ `#[serde(rename_all = "camelCase")]` を付け直す。

```rust
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Place {
        #[serde(default)]
        display_name: Option<DisplayName>,
        #[serde(default)]
        formatted_address: Option<String>,
        #[serde(default)]
        types: Vec<String>,
    }
```

（`Response` と `DisplayName` はフィールド名がすでにスネークケースと一致する部分のみなので変更不要。`Place` だけ `display_name`/`formatted_address` がキャメルケース対応が必要）

既存の `mod tests` に次を追加する。

```rust
    #[test]
    fn google_places_request_body_has_circle_restriction() {
        let body = google_places_request_body(35.6812, 139.7671);
        assert_eq!(body["locationRestriction"]["circle"]["center"]["latitude"], 35.6812);
        assert_eq!(body["maxResultCount"], 1);
    }

    #[test]
    fn parses_named_place() {
        let json = r#"{"places":[{"displayName":{"text":"ブルーボトルコーヒー","languageCode":"ja"},
            "formattedAddress":"日本、東京都千代田区丸の内","types":["cafe","food"]}]}"#;
        let g = parse_google_places_response(json).unwrap().unwrap();
        assert_eq!(g.name.as_deref(), Some("ブルーボトルコーヒー"));
        assert_eq!(g.category.as_deref(), Some("cafe"));
    }

    #[test]
    fn empty_places_is_none() {
        assert_eq!(parse_google_places_response(r#"{"places":[]}"#).unwrap(), None);
    }

    #[test]
    fn google_places_broken_json_is_error() {
        assert!(parse_google_places_response("<html>").is_err());
    }
```

- [ ] **Step 2: テストを実行して通ることを確認する**

Run: `cd crates/areitu-core && cargo test resolve::geocode::`
Expected: PASS（既存分 + 新規4件が成功する）

- [ ] **Step 3: clippy を確認する**

Run: `cd crates/areitu-core && cargo clippy --all-targets -- -D warnings`
Expected: warning ゼロ

- [ ] **Step 4: commit**

```bash
git add crates/areitu-core/src/resolve/geocode.rs
git commit -m "$(cat <<'EOF'
feat: add Google Places Nearby Search ReverseGeocoder impl to areitu-core

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: フロントエンド API 層 + 検索/一覧画面（#12）

`invoke` を薄くラップした関数と型を作り、検索キーワード（インクリメンタル検索）とソート切り替え（訪問数／最新）を持つ一覧画面を作る。Review Focus 1（古いリクエストの結果で新しい結果を上書きしない）をここでテストする。

**Files:**
- Create: `apps/desktop/src/api/types.ts`
- Create: `apps/desktop/src/api/tauri.ts`
- Create: `apps/desktop/src/components/SearchBar.tsx`
- Create: `apps/desktop/src/components/SortToggle.tsx`
- Create: `apps/desktop/src/components/PlaceListItem.tsx`
- Create: `apps/desktop/src/screens/SearchListScreen.tsx`
- Create: `apps/desktop/src/screens/SearchListScreen.test.tsx`
- Modify: `apps/desktop/src/App.tsx`

**Interfaces:**
- Produces: `export interface Place { id: number; name: string; visitCount: number; lastVisit: string }`
- Produces: `export type SortMode = "count" | "recent";`
- Produces: `export async function listPlaces(sort: SortMode, keyword: string): Promise<Place[]>`（`apps/desktop/src/api/tauri.ts`）
- Produces: `<SearchListScreen onSelectPlace={(id: number) => void} />`（Task 9 がこの `onSelectPlace` を使って画面遷移する）

- [ ] **Step 1: 共有型と API ラッパーを作る**

`apps/desktop/src/api/types.ts`:

```ts
export interface Place {
  id: number;
  name: string;
  visitCount: number;
  lastVisit: string;
}

export interface Visit {
  startedAt: string;
  endedAt: string;
}

export type SortMode = "count" | "recent";

export interface SyncSummary {
  scanned: number;
  scanErrors: string[];
  visitsCreated: number;
  resolveFailed: number;
}
```

`apps/desktop/src/api/tauri.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { Place, SortMode, SyncSummary, Visit } from "./types";

export async function listPlaces(sort: SortMode, keyword: string): Promise<Place[]> {
  const trimmed = keyword.trim();
  return invoke<Place[]>("list_places", { sort, keyword: trimmed === "" ? null : trimmed });
}

export async function visitsOf(placeId: number): Promise<Visit[]> {
  return invoke<Visit[]>("visits_of", { placeId });
}

export async function renamePlace(placeId: number, name: string): Promise<number> {
  return invoke<number>("rename_place", { placeId, name });
}

export async function syncNow(): Promise<SyncSummary> {
  return invoke<SyncSummary>("sync_now");
}
```

- [ ] **Step 2: 小さなコンポーネントを作る**

`apps/desktop/src/components/SearchBar.tsx`:

```tsx
interface Props {
  value: string;
  onChange: (value: string) => void;
}

export function SearchBar({ value, onChange }: Props) {
  return (
    <input
      type="text"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder="店名で検索"
      aria-label="店名で検索"
      className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-slate-900 placeholder:text-slate-400 focus:border-slate-500 focus:outline-none"
    />
  );
}
```

`apps/desktop/src/components/SortToggle.tsx`:

```tsx
import type { SortMode } from "../api/types";

interface Props {
  value: SortMode;
  onChange: (value: SortMode) => void;
}

export function SortToggle({ value, onChange }: Props) {
  const base = "rounded-md px-3 py-1.5 text-sm font-medium transition-colors";
  const active = "bg-slate-800 text-white";
  const inactive = "bg-slate-100 text-slate-600 hover:bg-slate-200";
  return (
    <div className="flex gap-2" role="group" aria-label="並び替え">
      <button
        type="button"
        className={`${base} ${value === "count" ? active : inactive}`}
        onClick={() => onChange("count")}
      >
        訪問回数
      </button>
      <button
        type="button"
        className={`${base} ${value === "recent" ? active : inactive}`}
        onClick={() => onChange("recent")}
      >
        最近訪問
      </button>
    </div>
  );
}
```

`apps/desktop/src/components/PlaceListItem.tsx`:

```tsx
import type { Place } from "../api/types";

interface Props {
  place: Place;
  onSelect: (id: number) => void;
}

export function PlaceListItem({ place, onSelect }: Props) {
  return (
    <li>
      <button
        type="button"
        onClick={() => onSelect(place.id)}
        className="flex w-full items-center justify-between rounded-md border border-slate-200 bg-white px-4 py-3 text-left hover:border-slate-400"
      >
        <span className="font-medium text-slate-900">{place.name}</span>
        <span className="text-sm text-slate-500">{place.visitCount} 回訪問</span>
      </button>
    </li>
  );
}
```

- [ ] **Step 3: 一覧画面の失敗するテストを書く**

`apps/desktop/src/screens/SearchListScreen.test.tsx`:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SearchListScreen } from "./SearchListScreen";
import * as tauriApi from "../api/tauri";

describe("SearchListScreen", () => {
  it("loads and shows places on mount", async () => {
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([
      { id: 1, name: "カフェ丸の内", visitCount: 3, lastVisit: "2026-09-01T12:00:00" },
    ]);
    render(<SearchListScreen onSelectPlace={vi.fn()} />);
    expect(await screen.findByText("カフェ丸の内")).toBeInTheDocument();
  });

  it("keeps only the latest search result when requests resolve out of order", async () => {
    let resolveFirst: (places: Awaited<ReturnType<typeof tauriApi.listPlaces>>) => void = () => {};
    let resolveSecond: (places: Awaited<ReturnType<typeof tauriApi.listPlaces>>) => void = () => {};
    const spy = vi.spyOn(tauriApi, "listPlaces");
    spy.mockResolvedValueOnce([]); // 初回マウント時の読み込み
    render(<SearchListScreen onSelectPlace={vi.fn()} />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));

    spy.mockImplementationOnce(() => new Promise((resolve) => (resolveFirst = resolve)));
    fireEvent.change(screen.getByLabelText("店名で検索"), { target: { value: "コ" } });
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));

    spy.mockImplementationOnce(() => new Promise((resolve) => (resolveSecond = resolve)));
    fireEvent.change(screen.getByLabelText("店名で検索"), { target: { value: "コー" } });
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(3));

    // 新しい入力（「コー」）のリクエストより後に、古い入力（「コ」）のリクエストが解決する
    resolveSecond([{ id: 2, name: "コーヒー専門店", visitCount: 1, lastVisit: "2026-09-01T12:00:00" }]);
    await screen.findByText("コーヒー専門店");
    resolveFirst([{ id: 1, name: "コンビニ", visitCount: 5, lastVisit: "2026-09-01T12:00:00" }]);

    await waitFor(() => {
      expect(screen.queryByText("コンビニ")).not.toBeInTheDocument();
      expect(screen.getByText("コーヒー専門店")).toBeInTheDocument();
    });
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test`
Expected: FAIL（`./SearchListScreen` が存在しない）

- [ ] **Step 4: `SearchListScreen` を実装する**

`apps/desktop/src/screens/SearchListScreen.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { listPlaces } from "../api/tauri";
import type { Place, SortMode } from "../api/types";
import { SearchBar } from "../components/SearchBar";
import { SortToggle } from "../components/SortToggle";
import { PlaceListItem } from "../components/PlaceListItem";

interface Props {
  onSelectPlace: (id: number) => void;
}

export function SearchListScreen({ onSelectPlace }: Props) {
  const [keyword, setKeyword] = useState("");
  const [sort, setSort] = useState<SortMode>("count");
  const [places, setPlaces] = useState<Place[]>([]);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  useEffect(() => {
    const id = ++requestId.current;
    listPlaces(sort, keyword)
      .then((result) => {
        if (id === requestId.current) {
          setPlaces(result);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (id === requestId.current) {
          setError(String(e));
        }
      });
  }, [sort, keyword]);

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <SearchBar value={keyword} onChange={setKeyword} />
        <SortToggle value={sort} onChange={setSort} />
      </div>
      {error !== null && <p className="text-sm text-red-600">{error}</p>}
      {places.length === 0 && error === null ? (
        <p className="text-sm text-slate-500">場所はまだありません</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {places.map((place) => (
            <PlaceListItem key={place.id} place={place} onSelect={onSelectPlace} />
          ))}
        </ul>
      )}
    </div>
  );
}
```

`requestId` による「最新のリクエストだけを反映する」実装が Review Focus 1 を満たす要点である。

- [ ] **Step 5: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test`
Expected: PASS（2 件のテストが成功する）

- [ ] **Step 6: `App.tsx` に組み込む**

`apps/desktop/src/App.tsx`:

```tsx
import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";

export default function App() {
  const [selectedPlaceId, setSelectedPlaceId] = useState<number | null>(null);

  if (selectedPlaceId === null) {
    return <SearchListScreen onSelectPlace={setSelectedPlaceId} />;
  }
  // PlaceDetailScreen は Task 9 で接続する
  return <SearchListScreen onSelectPlace={setSelectedPlaceId} />;
}
```

- [ ] **Step 7: commit**

```bash
git add apps/desktop/src
git commit -m "$(cat <<'EOF'
feat: add search/list screen with incremental search and sort toggle

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: 場所詳細画面（#13）

名前・総訪問数・訪問日時一覧（新しい順）を表示し、インライン編集で `rename_place` を呼ぶ画面を作る。Review Focus 2（空白リネームでクラッシュしない）をここでテストする。

**Files:**
- Create: `apps/desktop/src/screens/PlaceDetailScreen.tsx`
- Create: `apps/desktop/src/screens/PlaceDetailScreen.test.tsx`
- Modify: `apps/desktop/src/App.tsx`

**Interfaces:**
- Consumes: `visitsOf(placeId: number): Promise<Visit[]>`, `renamePlace(placeId: number, name: string): Promise<number>`（Task 8）
- Produces: `<PlaceDetailScreen place={Place} onRenamed={(newId: number, newName: string) => void} onBack={() => void} />`

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/src/screens/PlaceDetailScreen.test.tsx`:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PlaceDetailScreen } from "./PlaceDetailScreen";
import * as tauriApi from "../api/tauri";
import type { Place } from "../api/types";

const place: Place = { id: 1, name: "カフェ丸の内", visitCount: 2, lastVisit: "2026-09-01T12:00:00" };

describe("PlaceDetailScreen", () => {
  it("shows name, visit count, and visits newest first", async () => {
    vi.spyOn(tauriApi, "visitsOf").mockResolvedValue([
      { startedAt: "2026-09-08T12:00:00", endedAt: "2026-09-08T12:45:00" },
      { startedAt: "2026-01-01T12:00:00", endedAt: "2026-01-01T12:45:00" },
    ]);
    render(<PlaceDetailScreen place={place} onRenamed={vi.fn()} onBack={vi.fn()} />);
    expect(screen.getByText("カフェ丸の内")).toBeInTheDocument();
    expect(screen.getByText("2 回")).toBeInTheDocument();
    const items = await screen.findAllByRole("listitem");
    expect(items[0].textContent).toContain("2026-09-08");
    expect(items[1].textContent).toContain("2026-01-01");
  });

  it("shows an error and keeps the input when renaming to blank fails", async () => {
    vi.spyOn(tauriApi, "visitsOf").mockResolvedValue([]);
    const renameSpy = vi
      .spyOn(tauriApi, "renamePlace")
      .mockRejectedValue("place name must not be empty");
    render(<PlaceDetailScreen place={place} onRenamed={vi.fn()} onBack={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "名前を編集" }));
    const input = screen.getByLabelText("新しい名前");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(renameSpy).toHaveBeenCalledWith(1, "   "));
    expect(await screen.findByText("place name must not be empty")).toBeInTheDocument();
    expect(screen.getByLabelText("新しい名前")).toHaveValue("   ");
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npm run test`
Expected: FAIL（`./PlaceDetailScreen` が存在しない）

- [ ] **Step 3: `PlaceDetailScreen` を実装する**

`apps/desktop/src/screens/PlaceDetailScreen.tsx`:

```tsx
import { useEffect, useState } from "react";
import { renamePlace, visitsOf } from "../api/tauri";
import type { Place, Visit } from "../api/types";

interface Props {
  place: Place;
  onRenamed: (newPlaceId: number, newName: string) => void;
  onBack: () => void;
}

export function PlaceDetailScreen({ place, onRenamed, onBack }: Props) {
  const [visits, setVisits] = useState<Visit[]>([]);
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(place.name);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraftName(place.name);
    setEditing(false);
    setError(null);
    visitsOf(place.id).then(setVisits);
  }, [place.id, place.name]);

  async function handleSave() {
    try {
      const newId = await renamePlace(place.id, draftName);
      setError(null);
      setEditing(false);
      onRenamed(newId, draftName.trim());
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      <button type="button" onClick={onBack} className="self-start text-sm text-slate-500 hover:text-slate-700">
        一覧に戻る
      </button>

      {editing ? (
        <div className="flex items-center gap-2">
          <label className="sr-only" htmlFor="place-name-input">
            新しい名前
          </label>
          <input
            id="place-name-input"
            aria-label="新しい名前"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            className="rounded-md border border-slate-300 px-3 py-2"
          />
          <button
            type="button"
            onClick={handleSave}
            className="rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white hover:bg-slate-700"
          >
            保存
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold text-slate-900">{place.name}</h1>
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
          >
            名前を編集
          </button>
        </div>
      )}

      {error !== null && <p className="text-sm text-red-600">{error}</p>}

      <p className="text-sm text-slate-600">{place.visitCount} 回</p>

      <ul className="flex flex-col gap-2">
        {visits.map((visit) => (
          <li key={visit.startedAt} className="rounded-md border border-slate-200 px-4 py-2 text-sm text-slate-700">
            {visit.startedAt.replace("T", " ")} 〜 {visit.endedAt.slice(11, 16)}
          </li>
        ))}
      </ul>
    </div>
  );
}
```

- [ ] **Step 4: テストを実行して通ることを確認する**

Run: `cd apps/desktop && npm run test`
Expected: PASS（Task 8 の2件 + 本タスクの2件、合計4件が成功する）

- [ ] **Step 5: `App.tsx` に接続する**

`apps/desktop/src/App.tsx`:

```tsx
import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import type { Place } from "./api/types";

export default function App() {
  const [selectedPlace, setSelectedPlace] = useState<Place | null>(null);

  if (selectedPlace === null) {
    return (
      <SearchListScreen
        onSelectPlace={(id) => setSelectedPlace({ id, name: "", visitCount: 0, lastVisit: "" })}
      />
    );
  }

  return (
    <PlaceDetailScreen
      place={selectedPlace}
      onBack={() => setSelectedPlace(null)}
      onRenamed={(newPlaceId, newName) =>
        setSelectedPlace({ ...selectedPlace, id: newPlaceId, name: newName })
      }
    />
  );
}
```

`SearchListScreen` は `Place` 全体ではなく `id` しか渡さない実装のままなので、詳細画面の見出しが一瞬空になる。これは Task 11 までの暫定であり、Task 11 で `SearchListScreen` の `onSelectPlace` を `(place: Place) => void` に変更して解消する。

- [ ] **Step 6: commit**

```bash
git add apps/desktop/src
git commit -m "$(cat <<'EOF'
feat: add place detail screen with inline rename

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: トレイ常駐・自動起動・定期ポーリング（#14）

トレイアイコン（「今すぐ同期」「AREITU を開く」「終了」）、`tauri-plugin-autostart` によるOS自動起動、30分（設定可能）ごとの `run_sync` 実行を実装する。DB は `Mutex<Connection>` で共有し、手動同期・トレイ同期・定期同期のいずれが重なっても安全（Review Focus 3）。

**Files:**
- Create: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src-tauri/src/sync.rs`（`spawn_poll_thread` を追加）
- Modify: `apps/desktop/src-tauri/src/main.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: `crate::sync::run_sync`, `crate::config::load_config`, `crate::AppState`
- Produces: `pub fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()>`（`tray.rs`）
- Produces: `pub fn spawn_poll_thread(app: tauri::AppHandle)`（`sync.rs`。30秒ごとに設定を再読込し、設定されたポーリング間隔に達したら同期する）

- [ ] **Step 1: 依存を追加する**

```bash
cd apps/desktop/src-tauri
cargo add tauri-plugin-autostart
```

- [ ] **Step 2: `tauri.conf.json` にトレイと自動起動プラグインの設定を追加する**

`apps/desktop/src-tauri/tauri.conf.json` を読み、`app` セクションに `trayIcon` を、`plugins` セクションに `autostart` を追加する（既存の `app.windows` や他フィールドは変更しない）。

```json
{
  "app": {
    "trayIcon": {
      "iconPath": "icons/icon.png"
    }
  },
  "plugins": {
    "autostart": {}
  }
}
```

- [ ] **Step 3: `spawn_poll_thread` を実装する**

`apps/desktop/src-tauri/src/sync.rs` の末尾（`#[cfg(test)]` の直前）に追記する。

```rust
use std::time::Duration;
use tauri::{AppHandle, Manager};

const POLL_CHECK_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn_poll_thread(app: AppHandle) {
    std::thread::spawn(move || {
        let mut elapsed = Duration::ZERO;
        loop {
            std::thread::sleep(POLL_CHECK_INTERVAL);
            elapsed += POLL_CHECK_INTERVAL;

            let state = app.state::<crate::AppState>();
            let config = crate::config::load_config(&state.config_path);
            let target = Duration::from_secs(u64::from(config.poll_interval_minutes.max(1)) * 60);
            if elapsed < target {
                continue;
            }
            elapsed = Duration::ZERO;

            let geocoder = match areitu_core::resolve::geocode::Nominatim::new(
                "AREITU-desktop-poll/0.1 (+https://github.com/ikeikeikeda66/AREITU)",
            ) {
                Ok(g) => g,
                Err(_) => continue,
            };
            if let Ok(mut conn) = state.conn.lock() {
                let _ = run_sync(&mut conn, &config.watched_dirs, &geocoder, None, config.min_confidence);
            }
        }
    });
}
```

（Nominatim 固定・LLM なしのこの実装は Task 11 で `build_geocoder`/`build_llm` に置き換える）

- [ ] **Step 4: `tray.rs` を実装する**

`apps/desktop/src-tauri/src/tray.rs`:

```rust
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "AREITU を開く", true, None::<&str>)?;
    let sync_item = MenuItem::with_id(app, "sync_now", "今すぐ同期", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &sync_item, &quit_item])?;

    TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "sync_now" => {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = app.state::<crate::AppState>();
                    let config = crate::config::load_config(&state.config_path);
                    if let (Ok(geocoder), Ok(mut conn)) = (
                        areitu_core::resolve::geocode::Nominatim::new(
                            "AREITU-desktop-tray/0.1 (+https://github.com/ikeikeikeda66/AREITU)",
                        ),
                        state.conn.lock(),
                    ) {
                        let _ = crate::sync::run_sync(
                            &mut conn,
                            &config.watched_dirs,
                            &geocoder,
                            None,
                            config.min_confidence,
                        );
                    }
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
```

- [ ] **Step 5: `main.rs` に組み込む**

```rust
mod commands;
mod config;
mod logic;
mod sync;
mod tray;

// ...

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // (Task 2 の内容はそのまま)
            let data_dir = app.path().app_data_dir().expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            let conn = areitu_core::db::open(&data_dir.join("areitu.db")).expect("failed to open areitu.db");

            let config_dir = app.path().app_config_dir().expect("failed to resolve app config dir");
            std::fs::create_dir_all(&config_dir).expect("failed to create app config dir");
            let config_path = config_dir.join("config.json");

            app.manage(AppState { conn: Mutex::new(conn), config_path });

            crate::tray::setup_tray(app.handle())?;
            crate::sync::spawn_poll_thread(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_places,
            commands::visits_of,
            commands::rename_place,
            commands::sync_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: ビルドを確認する**

Run: `cd apps/desktop/src-tauri && cargo build`
Expected: エラーなく終了する（Linux 実行環境では webkit2gtk 等のシステム依存が必要になる場合がある。ローカルでビルドできない場合は Task 12 の CI 実行結果で確認する）

- [ ] **Step 7: すでにある `sync::` テストがすべて通ることを再確認する**

Run: `cd apps/desktop/src-tauri && cargo test sync::`
Expected: PASS（Task 4 で書いた3件、`spawn_poll_thread` 追加後も変わらず成功する）

- [ ] **Step 8: clippy を確認する**

Run: `cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: warning ゼロ

- [ ] **Step 9: commit**

```bash
git add apps/desktop/src-tauri
git commit -m "$(cat <<'EOF'
feat: add tray icon, autostart, and periodic background sync polling

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: 設定コマンド・LLM/ジオコーダー切り替え・設定画面（#15）

`AppConfig`／`SecretStore` を土台に、`get_settings` / `save_settings` コマンドと設定画面を実装する。`build_geocoder` / `build_llm` で `run_sync_with_config` を完成させ、Task 4・10 で仮置きしていた Nominatim 固定を置き換える。Review Focus 5（プロバイダを選んだのにキー未設定でも同期は落ちない）をここでテストする。

**Files:**
- Modify: `apps/desktop/src-tauri/src/sync.rs`（`build_geocoder` / `build_llm` / `run_sync_with_config` を追加）
- Modify: `apps/desktop/src-tauri/src/commands.rs`（`sync_now` を `run_sync_with_config` に切り替え、`get_settings` / `save_settings` を追加）
- Modify: `apps/desktop/src-tauri/src/tray.rs`、`main.rs`（`run_sync_with_config` に切り替え）
- Create: `apps/desktop/src/screens/SettingsScreen.tsx`
- Modify: `apps/desktop/src/api/types.ts`、`apps/desktop/src/api/tauri.ts`
- Modify: `apps/desktop/src/App.tsx`

**Interfaces:**
- Produces: `pub fn build_geocoder(config: &AppConfig, secrets: &dyn SecretStore) -> Box<dyn ReverseGeocoder>`
- Produces: `pub fn build_llm(config: &AppConfig, secrets: &dyn SecretStore) -> Option<Box<dyn LlmClient>>`
- Produces: `pub fn run_sync_with_config(conn: &mut Connection, config: &AppConfig, secrets: &dyn SecretStore) -> Result<SyncSummary, String>`
- Produces: Tauri コマンド `get_settings`, `save_settings`
- Produces: `export interface SettingsDto { watchedDirs: string[]; llmProvider: "none"|"ollama"|"openai"|"gemini"; ollamaUrl: string; ollamaModel: string; openaiModel: string; geminiModel: string; googlePlacesEnabled: boolean; minConfidence: number; pollIntervalMinutes: number; hasOpenaiKey: boolean; hasGeminiKey: boolean; hasGooglePlacesKey: boolean }`

- [ ] **Step 1: `build_geocoder` / `build_llm` / `run_sync_with_config` の失敗するテストを書く**

`apps/desktop/src-tauri/src/sync.rs` の `pub fn run_sync(..)` の後に追記する。

```rust
use areitu_core::resolve::geocode::{GooglePlaces, Nominatim};
use areitu_core::resolve::llm::{Gemini, OpenAi, Ollama};
use crate::config::{AppConfig, LlmProvider, SecretStore, GEMINI_KEY, GOOGLE_PLACES_KEY, OPENAI_KEY};

const USER_AGENT: &str = concat!(
    "AREITU-desktop/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

pub fn build_geocoder(config: &AppConfig, secrets: &dyn SecretStore) -> Box<dyn ReverseGeocoder> {
    if config.google_places_enabled {
        if let Some(key) = secrets.get(GOOGLE_PLACES_KEY) {
            if let Ok(g) = GooglePlaces::new(&key) {
                return Box::new(g);
            }
        }
    }
    Box::new(Nominatim::new(USER_AGENT).expect("building a Nominatim client never fails"))
}

pub fn build_llm(config: &AppConfig, secrets: &dyn SecretStore) -> Option<Box<dyn LlmClient>> {
    match config.llm_provider {
        LlmProvider::None => None,
        LlmProvider::Ollama => {
            if config.ollama_model.trim().is_empty() {
                return None;
            }
            Ollama::new(&config.ollama_url, &config.ollama_model)
                .ok()
                .map(|c| Box::new(c) as Box<dyn LlmClient>)
        }
        LlmProvider::OpenAi => secrets
            .get(OPENAI_KEY)
            .and_then(|key| OpenAi::new(&key, &config.openai_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
        LlmProvider::Gemini => secrets
            .get(GEMINI_KEY)
            .and_then(|key| Gemini::new(&key, &config.gemini_model).ok())
            .map(|c| Box::new(c) as Box<dyn LlmClient>),
    }
}

pub fn run_sync_with_config(
    conn: &mut Connection,
    config: &AppConfig,
    secrets: &dyn SecretStore,
) -> Result<SyncSummary, String> {
    let geocoder = build_geocoder(config, secrets);
    let llm = build_llm(config, secrets);
    run_sync(conn, &config.watched_dirs, geocoder.as_ref(), llm.as_deref(), config.min_confidence)
}
```

`apps/desktop/src-tauri/src/sync.rs` の `mod tests` に追加する。

```rust
    use crate::config::{AppConfig, FakeSecretStore, LlmProvider, OPENAI_KEY};

    #[test]
    fn openai_provider_without_api_key_degrades_to_nominatim_only() {
        let mut c = open_in_memory().unwrap();
        let mut config = AppConfig::default();
        config.llm_provider = LlmProvider::OpenAi; // キーは設定しない
        let secrets = FakeSecretStore::new();
        // ネットワークに出る前提のテストは避け、build_llm が None を返すことだけを確認する
        assert!(build_llm(&config, &secrets).is_none());
        let summary = run_sync_with_config(&mut c, &config, &secrets);
        assert!(summary.is_ok());
    }

    #[test]
    fn openai_provider_with_api_key_builds_a_client() {
        let config = {
            let mut c = AppConfig::default();
            c.llm_provider = LlmProvider::OpenAi;
            c.openai_model = "gpt-4o-mini".to_owned();
            c
        };
        let secrets = FakeSecretStore::new();
        secrets.set(OPENAI_KEY, "sk-test").unwrap();
        assert!(build_llm(&config, &secrets).is_some());
    }

    #[test]
    fn ollama_provider_with_blank_model_degrades_to_none() {
        let mut config = AppConfig::default();
        config.llm_provider = LlmProvider::Ollama;
        config.ollama_model = "".to_owned();
        let secrets = FakeSecretStore::new();
        assert!(build_llm(&config, &secrets).is_none());
    }
```

`run_sync_with_config` は `openai_provider_without_api_key_degrades_to_nominatim_only` の中で `Nominatim` の実オブジェクトを作って `build_visits` に渡すが、DB に未処理ログが無いため `reverse()` は一度も呼ばれず、実ネットワークアクセスは発生しない点に注意する（`unassigned_raw_logs` が空なら `cluster()` は空の候補列を返し、`Resolver::resolve` は呼ばれない）。

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop/src-tauri && cargo test sync::`
Expected: FAIL（`GooglePlaces` などまだ import 経路が繋がっていない、あるいはこの時点ではまだ Step 1 のコードそのものが実装なのでコンパイルは通るはず。もしテストがすでに全部 PASS する場合は Step 1 の実装が既に十分であることの確認とする）

- [ ] **Step 3: テストを実行して通ることを確認する**

Run: `cd apps/desktop/src-tauri && cargo test sync::`
Expected: PASS（Task 4/10 分と合わせて全件成功する）

- [ ] **Step 4: `commands.rs` の `sync_now` を `run_sync_with_config` に切り替え、`get_settings`/`save_settings` を追加する**

`apps/desktop/src-tauri/src/commands.rs` の `sync_now` を次に置き換える。

```rust
use crate::config::{
    load_config, save_config, AppConfig, KeyringSecretStore, LlmProvider, SecretStore,
    GEMINI_KEY, GOOGLE_PLACES_KEY, OPENAI_KEY,
};
use crate::sync::{run_sync_with_config, SyncSummary};

#[tauri::command]
pub fn sync_now(state: State<AppState>) -> Result<SyncSummary, String> {
    let config = load_config(&state.config_path);
    let mut conn = state.conn.lock().map_err(|_| "db lock poisoned".to_string())?;
    run_sync_with_config(&mut conn, &config, &KeyringSecretStore)
}

#[derive(serde::Serialize)]
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
    pub has_openai_key: bool,
    pub has_gemini_key: bool,
    pub has_google_places_key: bool,
}

#[derive(serde::Deserialize)]
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
        has_openai_key: secrets.get(OPENAI_KEY).is_some(),
        has_gemini_key: secrets.get(GEMINI_KEY).is_some(),
        has_google_places_key: secrets.get(GOOGLE_PLACES_KEY).is_some(),
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
    };
    let secrets = KeyringSecretStore;
    apply_secret(&secrets, OPENAI_KEY, settings.openai_api_key)?;
    apply_secret(&secrets, GEMINI_KEY, settings.gemini_api_key)?;
    apply_secret(&secrets, GOOGLE_PLACES_KEY, settings.google_places_api_key)?;
    save_config(&state.config_path, &config).map_err(|e| e.to_string())
}
```

`main.rs` の `invoke_handler!` と `tray.rs` の `sync_now` メニューを `run_sync_with_config` を使うように更新する。

```rust
.invoke_handler(tauri::generate_handler![
    commands::list_places,
    commands::visits_of,
    commands::rename_place,
    commands::sync_now,
    commands::get_settings,
    commands::save_settings,
])
```

`tray.rs` の `"sync_now" => { .. }` ブロックを次に置き換える。

```rust
            "sync_now" => {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let state = app.state::<crate::AppState>();
                    let config = crate::config::load_config(&state.config_path);
                    if let Ok(mut conn) = state.conn.lock() {
                        let _ = crate::sync::run_sync_with_config(
                            &mut conn,
                            &config,
                            &crate::config::KeyringSecretStore,
                        );
                    }
                });
            }
```

`sync.rs` の `spawn_poll_thread` も同様に `run_sync_with_config(&mut conn, &config, &crate::config::KeyringSecretStore)` を呼ぶように置き換える。

- [ ] **Step 5: ビルドとテストを確認する**

Run: `cd apps/desktop/src-tauri && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: すべてエラー・warning なしで終了する

- [ ] **Step 6: フロントエンドの型とAPIラッパーを追加する**

`apps/desktop/src/api/types.ts` に追記する。

```ts
export type LlmProvider = "none" | "ollama" | "openai" | "gemini";

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
  hasOpenaiKey: boolean;
  hasGeminiKey: boolean;
  hasGooglePlacesKey: boolean;
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
  openaiApiKey: string | null;
  geminiApiKey: string | null;
  googlePlacesApiKey: string | null;
}
```

`apps/desktop/src/api/tauri.ts` に追記する。

```ts
import type { SaveSettingsInput, Settings } from "./types";

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function saveSettings(settings: SaveSettingsInput): Promise<void> {
  return invoke<void>("save_settings", { settings });
}
```

- [ ] **Step 7: 設定画面を作る**

`apps/desktop/src/screens/SettingsScreen.tsx`:

```tsx
import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "../api/tauri";
import type { LlmProvider, Settings } from "../api/types";

interface Props {
  onBack: () => void;
}

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
  hasOpenaiKey: false,
  hasGeminiKey: false,
  hasGooglePlacesKey: false,
};

export function SettingsScreen({ onBack }: Props) {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [newDir, setNewDir] = useState("");
  const [openaiKeyInput, setOpenaiKeyInput] = useState("");
  const [geminiKeyInput, setGeminiKeyInput] = useState("");
  const [placesKeyInput, setPlacesKeyInput] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then(setSettings);
  }, []);

  async function handleSave() {
    try {
      await saveSettings({
        watchedDirs: settings.watchedDirs,
        llmProvider: settings.llmProvider,
        ollamaUrl: settings.ollamaUrl,
        ollamaModel: settings.ollamaModel,
        openaiModel: settings.openaiModel,
        geminiModel: settings.geminiModel,
        googlePlacesEnabled: settings.googlePlacesEnabled,
        minConfidence: settings.minConfidence,
        pollIntervalMinutes: settings.pollIntervalMinutes,
        openaiApiKey: openaiKeyInput === "" ? null : openaiKeyInput,
        geminiApiKey: geminiKeyInput === "" ? null : geminiKeyInput,
        googlePlacesApiKey: placesKeyInput === "" ? null : placesKeyInput,
      });
      setStatus("保存しました");
      setOpenaiKeyInput("");
      setGeminiKeyInput("");
      setPlacesKeyInput("");
      getSettings().then(setSettings);
    } catch (e) {
      setStatus(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-6 p-6">
      <button type="button" onClick={onBack} className="self-start text-sm text-slate-500 hover:text-slate-700">
        一覧に戻る
      </button>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">監視フォルダ</h2>
        <ul className="flex flex-col gap-1">
          {settings.watchedDirs.map((dir) => (
            <li key={dir} className="flex items-center justify-between rounded-md border border-slate-200 px-3 py-2">
              <span className="text-sm text-slate-700">{dir}</span>
              <button
                type="button"
                onClick={() =>
                  setSettings({ ...settings, watchedDirs: settings.watchedDirs.filter((d) => d !== dir) })
                }
                className="text-sm text-red-600 hover:underline"
              >
                削除
              </button>
            </li>
          ))}
        </ul>
        <div className="flex gap-2">
          <input
            value={newDir}
            onChange={(e) => setNewDir(e.target.value)}
            placeholder="フォルダのパス"
            aria-label="フォルダのパス"
            className="flex-1 rounded-md border border-slate-300 px-3 py-2"
          />
          <button
            type="button"
            onClick={() => {
              if (newDir.trim() !== "") {
                setSettings({ ...settings, watchedDirs: [...settings.watchedDirs, newDir.trim()] });
                setNewDir("");
              }
            }}
            className="rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white hover:bg-slate-700"
          >
            追加
          </button>
        </div>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">店舗名の推論（LLM）</h2>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          プロバイダ
          <select
            value={settings.llmProvider}
            onChange={(e) => setSettings({ ...settings, llmProvider: e.target.value as LlmProvider })}
            className="rounded-md border border-slate-300 px-3 py-2"
          >
            <option value="none">使わない</option>
            <option value="ollama">Ollama（ローカル）</option>
            <option value="openai">OpenAI</option>
            <option value="gemini">Gemini</option>
          </select>
        </label>

        {settings.llmProvider === "ollama" && (
          <>
            <label className="flex flex-col gap-1 text-sm text-slate-700">
              Ollama URL
              <input
                value={settings.ollamaUrl}
                onChange={(e) => setSettings({ ...settings, ollamaUrl: e.target.value })}
                className="rounded-md border border-slate-300 px-3 py-2"
              />
            </label>
            <label className="flex flex-col gap-1 text-sm text-slate-700">
              モデル名
              <input
                value={settings.ollamaModel}
                onChange={(e) => setSettings({ ...settings, ollamaModel: e.target.value })}
                className="rounded-md border border-slate-300 px-3 py-2"
              />
            </label>
          </>
        )}

        {settings.llmProvider === "openai" && (
          <label className="flex flex-col gap-1 text-sm text-slate-700">
            OpenAI API キー（{settings.hasOpenaiKey ? "設定済み" : "未設定"}）
            <input
              type="password"
              value={openaiKeyInput}
              onChange={(e) => setOpenaiKeyInput(e.target.value)}
              aria-label="OpenAI API キー"
              placeholder="変更する場合のみ入力"
              className="rounded-md border border-slate-300 px-3 py-2"
            />
          </label>
        )}

        {settings.llmProvider === "gemini" && (
          <label className="flex flex-col gap-1 text-sm text-slate-700">
            Gemini API キー（{settings.hasGeminiKey ? "設定済み" : "未設定"}）
            <input
              type="password"
              value={geminiKeyInput}
              onChange={(e) => setGeminiKeyInput(e.target.value)}
              aria-label="Gemini API キー"
              placeholder="変更する場合のみ入力"
              className="rounded-md border border-slate-300 px-3 py-2"
            />
          </label>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">場所の検索</h2>
        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={settings.googlePlacesEnabled}
            onChange={(e) => setSettings({ ...settings, googlePlacesEnabled: e.target.checked })}
          />
          Google Places API を優先的に使う
        </label>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          Google Places API キー（{settings.hasGooglePlacesKey ? "設定済み" : "未設定"}）
          <input
            type="password"
            value={placesKeyInput}
            onChange={(e) => setPlacesKeyInput(e.target.value)}
            aria-label="Google Places API キー"
            placeholder="変更する場合のみ入力"
            className="rounded-md border border-slate-300 px-3 py-2"
          />
        </label>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">同期</h2>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          ポーリング間隔（分）
          <input
            type="number"
            min={1}
            value={settings.pollIntervalMinutes}
            onChange={(e) => setSettings({ ...settings, pollIntervalMinutes: Number(e.target.value) })}
            className="w-32 rounded-md border border-slate-300 px-3 py-2"
          />
        </label>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google アカウント</h2>
        <p className="text-sm text-slate-500">Google Drive 連携は今後のバージョンで対応予定です。</p>
      </section>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={handleSave}
          className="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700"
        >
          保存
        </button>
        {status !== null && <span className="text-sm text-slate-600">{status}</span>}
      </div>
    </div>
  );
}
```

- [ ] **Step 8: `App.tsx` に組み込み、`SearchListScreen` に設定画面への入口を追加する**

`apps/desktop/src/App.tsx`:

```tsx
import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import type { Place } from "./api/types";

type View = { kind: "list" } | { kind: "detail"; place: Place } | { kind: "settings" };

export default function App() {
  const [view, setView] = useState<View>({ kind: "list" });

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

- [ ] **Step 9: フロントエンドのビルドとテストを確認する**

Run: `cd apps/desktop && npm run build && npm run test`
Expected: どちらもエラーなく終了する

- [ ] **Step 10: commit**

```bash
git add apps/desktop
git commit -m "$(cat <<'EOF'
feat: add settings screen, provider-aware sync (build_geocoder/build_llm), and settings commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: CI 拡張 — フロントエンドテストと Tauri システム依存

既存の3OSマトリクスに `apps/desktop/src-tauri`（ワークスペースメンバー）のビルドを含める。Tauri v2 の Linux ビルドには追加のシステムライブラリが必要なため、ubuntu ジョブにだけインストールステップを追加する。フロントエンドの Vitest はワークスペース外（npm 管理下）にあるため、別ジョブとして追加する。

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `apps/desktop/package.json` の `test` / `build` スクリプト（Task 1, 8 で追加済み）
- Consumes: `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings`（Task 1 でワークスペースに `areitu-desktop` を追加済み）

- [ ] **Step 1: `ci.yml` を更新する**

`/Users/ikedashinichi/AREITU/.github/workflows/ci.yml` 全体を次に置き換える。

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Install Tauri Linux system dependencies
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libgtk-3-dev \
            libayatana-appindicator3-dev \
            librsvg2-dev \
            libssl-dev \
            libxdo-dev \
            patchelf \
            build-essential \
            curl \
            wget \
            file
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - run: cargo test --workspace
      - run: cargo clippy --workspace --all-targets -- -D warnings

  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - working-directory: apps/desktop
        run: npm ci
      - working-directory: apps/desktop
        run: npm run test
      - working-directory: apps/desktop
        run: npm run build
```

Ubuntu ランナーのイメージが将来 `libwebkit2gtk-4.1-dev` を含まないバージョンに変わった場合は、Tauri v2 公式の Linux 前提条件ページのパッケージ名に合わせて調整する（本計画作成時点の Tauri v2 ドキュメントに基づく想定）。

- [ ] **Step 2: ローカルで YAML の構文だけ確認する**

Run: `cd /Users/ikedashinichi/AREITU && python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))" 2>&1 || cat .github/workflows/ci.yml`
Expected: エラーが出ない（`python3` に `PyYAML` が無い環境では代わりにファイル内容がそのまま表示されるので、インデントを目視確認する）

- [ ] **Step 3: ワークスペース全体のテストと clippy をローカルで再確認する**

Run: `cd /Users/ikedashinichi/AREITU && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: すべて PASS・warning ゼロ

- [ ] **Step 4: commit**

```bash
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci: add Tauri Linux system deps and a frontend (Vitest) job

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 13: README 更新とワークスペース全体の最終確認

デスクトップアプリの起動・ビルド・テスト手順を README に追記し、Rust ワークスペース全体とフロントエンドの最終確認を行う。

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: なし（ドキュメントのみ）

- [ ] **Step 1: README にデスクトップアプリの節を追加する**

`/Users/ikedashinichi/AREITU/README.md` の末尾に追記する。

```markdown

## デスクトップアプリ (apps/desktop)

Tauri 2 + React + TypeScript + Tailwind のデスクトップアプリです。`areitu-core` に依存します。

```bash
cd apps/desktop
npm install
npm run tauri dev      # 開発起動（トレイ常駐・自動起動・定期ポーリングが有効になる）
npm run build           # フロントエンドのビルド確認
npm run test             # Vitest
cd src-tauri && cargo test   # Rust 側ロジックのテスト（Tauri ランタイム不要）
```

DB は OS のアプリデータディレクトリ配下の `areitu.db` に、非秘密設定は同 config ディレクトリの `config.json` に保存されます。LLM／Google Places の API キーは OS のキーチェーンに保存され、リポジトリやファイルには書き出されません。
```

- [ ] **Step 2: ワークスペース全体を最終確認する**

Run: `cd /Users/ikedashinichi/AREITU && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && (cd apps/desktop && npm run build && npm run test)`
Expected: すべて成功する（Rust テスト全件 PASS、clippy warning ゼロ、フロントエンドビルドとテスト成功）

- [ ] **Step 3: commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs: document desktop app dev/build/test commands in README

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- #11（Tauri 2 + React scaffold, src-tauri がワークスペース参加, list_places/visits_of/rename_place/sync_now コマンド）→ Task 1, 2, 4
- #12（検索/一覧、インクリメンタル検索、ソート切替）→ Task 8
- #13（詳細カード、総訪問数、訪問日時一覧が新しい順、インライン編集で辞書登録）→ Task 9（`rename_place` が `record_correction` を呼ぶことは Phase 1 の `crates/areitu-core/src/store.rs` の `rename_place` に実装済みで、そのまま再利用している）
- #14（トレイ常駐、OS自動起動、30分ごとの `scan_photos`→`build_visits`、バックグラウンドスレッド、`Mutex<Connection>` 共有、今すぐ同期メニュー）→ Task 10
- #15（監視フォルダ・LLMプロバイダ・API キー・Google Places キー・ポーリング間隔の設定画面、非秘密設定はJSON、秘密はキーチェーン、OpenAI/Gemini の LlmClient、Google Places の ReverseGeocoder、それぞれ fixture JSON でユニットテスト・実ネットワークなし）→ Task 3, 5, 6, 7, 11
- CI拡張（フロントエンドテスト・Tauri system deps on ubuntu）→ Task 12
- Google OAuth/Drive を含めない、設定画面に「Google アカウント」見出しだけ置く → Task 11 の `SettingsScreen` に反映済み

ギャップなし。

**Placeholder scan:** 「TBD」「後で実装」「同様に」等の記述は無い。各タスクのコードブロックはすべて実際に書く内容そのもの。

**Type consistency:** `Place`（id, name, visitCount, lastVisit）と Rust 側 `PlaceDto`（id, name, visit_count, last_visit）は Tauri のキャメルケース⇄スネークケース自動変換前提で対応している。`Visit`/`VisitDto`、`SyncSummary`/`SyncSummary`（Rust の `serde(Serialize)` によりフロント側は camelCase の `scanErrors`/`visitsCreated`/`resolveFailed` として受け取る）、`Settings`/`SettingsDto`、`SaveSettingsInput`/`SaveSettingsDto` も同様にフィールド名を全タスクで揃えている。`run_sync` → `run_sync_with_config` → `build_geocoder`/`build_llm` の関数シグネチャは Task 4 で仮実装したものを Task 11 で一貫した形に差し替えている。

**Review Focus:** 5項目すべてに対応するテストをタスク内に明記済み（検索の競合＝Task 8、空白リネーム＝Task 9、多重同期＝Task 4/10、フォルダ欠損＝Task 4、プロバイダ未設定での縮退＝Task 11）。
