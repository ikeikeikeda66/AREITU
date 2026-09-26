# Phase 1: コアエンジン & DB設計 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 写真Exifとカレンダー予定から訪問スポットを生成し、店舗名を推論して SQLite に保存・検索できる Rust コアライブラリと検証用 CLI を作る。

**Architecture:** Cargo workspace に `areitu-core`（ライブラリ）と `areitu-cli`（検証用バイナリ）を置く。Phase 2 の Tauri バックエンドは `areitu-core` をそのまま依存に加える。データの流れは「取り込み（raw_logs）→ クラスタリング（訪問候補）→ 推論（辞書 → Nominatim → LLM）→ places / visits 保存 → 検索」。外部サービス（Nominatim, Ollama）はトレイト越しに呼び、テストではフェイクに差し替える。

**Tech Stack:** Rust (stable, edition 2021), rusqlite (bundled, chrono), kamadak-exif, walkdir, chrono, serde / serde_json, reqwest (blocking, json), thiserror, clap, anyhow, tempfile（テスト用）

**Spec:** AREITU 基本構想書 https://docs.google.com/document/d/1OPiPQLFQdxd31uZOQzWQORTyrvs1roQS_XW_ykYPj6E/edit （特に「5. データソース」「6. 推論エンジン仕様」「9. 実装ロードマップ Phase 1」）。全体ロードマップ: `docs/superpowers/plans/2026-09-26-roadmap.md`

## Global Constraints

- ライセンス: MIT License
- Rust: stable 1.82 以上（`Option::is_none_or` を使う）。edition 2021
- 配布対象: Mac・Windows（CI は ubuntu / macos / windows の3 OS で `cargo test` を通す）
- DB は単一ファイル `areitu.db`（SQLite）
- テーブル: `places`, `visits`, `raw_logs`, `user_dictionary`
- 同一日でも複数スポットをタイムスタンプ順に別々の訪問として保持する
- 推論順序: ユーザー辞書（最優先）→ Nominatim（無料 OSM）→ LLM（フォールバック、ローカル Ollama）
- Nominatim 利用規約: 1 リクエスト/秒以下、識別可能な User-Agent を必ず付ける
- 独自サーバーを持たない。外部送信は Nominatim と（ユーザーが選んだ）LLM エンドポイントのみ
- 日時はすべて「現地の壁時計時刻」の `NaiveDateTime` で扱う（写真 Exif にタイムゾーンがないため。カレンダーのオフセット付き時刻も現地時刻に揃える）

## Review Focus

1. GPS なし・GPS が (0, 0) の写真 → 訪問を作らず、クラッシュもしない（Task 3 でテスト）
2. 検索キーワードに `%` `_` を含む／空白のみ → `%` `_` は文字どおり一致、空白のみは全件（Task 9 でテスト）
3. 終日予定・キャンセル済み予定・`+09:00` 付き時刻 → 終日とキャンセルは無視、時刻は現地時刻のまま（Task 4 でテスト）
4. 同じフォルダ・同じカレンダーを再取り込み、`build` を再実行 → raw_logs も visits も重複しない（Task 2, Task 9 でテスト）
5. Nominatim / Ollama がオフライン → `build` は落ちず、失敗件数を返し、未処理ログは次回再試行される。LLM 失敗は Nominatim 結果に縮退（Task 8, Task 9 でテスト）

## File Structure

```
Cargo.toml                          workspace
LICENSE                             MIT
.github/workflows/ci.yml            3 OS で test + clippy
crates/areitu-core/
  Cargo.toml
  src/lib.rs                        モジュール宣言, Error / Result, testutil
  src/db.rs                         接続 open とスキーマ作成
  src/model.rs                      RawLog, Source
  src/store.rs                      raw_logs / places / visits の読み書き
  src/geo.rs                        haversine 距離
  src/exif.rs                       Exif → PhotoMeta
  src/scan.rs                       フォルダ走査 → raw_logs
  src/calendar.rs                   Google Calendar events JSON → raw_logs
  src/cluster.rs                    raw_logs → VisitCandidate
  src/resolve/mod.rs                Resolver（3段階の推論チェーン）
  src/resolve/dictionary.rs         ユーザー辞書
  src/resolve/geocode.rs            ReverseGeocoder トレイト + Nominatim
  src/resolve/llm.rs                LlmClient トレイト + Ollama + プロンプト
  src/pipeline.rs                   build_visits
  src/query.rs                      一覧・検索・訪問履歴
crates/areitu-cli/
  Cargo.toml
  src/main.rs                       検証用 CLI
  tests/cli.rs
```

---

### Task 1: Workspace・DB スキーマ・CI

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `.github/workflows/ci.yml`
- Create: `crates/areitu-core/Cargo.toml`, `crates/areitu-core/src/lib.rs`, `crates/areitu-core/src/db.rs`
- Modify: `.gitignore`（末尾に追記）

**Interfaces:**
- Produces: `areitu_core::Error`, `areitu_core::Result<T>`, `db::open(&Path) -> Result<Connection>`, `db::open_in_memory() -> Result<Connection>`

- [ ] **Step 1: workspace と crate を作る**

```bash
cargo new --lib crates/areitu-core
```

`Cargo.toml`（リポジトリ直下）:

```toml
[workspace]
resolver = "2"
members = ["crates/areitu-core"]
```

```bash
cd crates/areitu-core
cargo add rusqlite --features bundled,chrono
cargo add chrono serde_json thiserror
cargo add serde --features derive
cd ../..
```

`.gitignore` 末尾に追記:

```
# Rust
target/

# Local DB
*.db
```

`LICENSE` に MIT License 全文（`Copyright (c) 2026 ikeikeikeda6`）を書く。

- [ ] **Step 2: 失敗するテストを書く**

`crates/areitu-core/src/db.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(c: &rusqlite::Connection) -> Vec<String> {
        let mut stmt = c
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn creates_all_tables() {
        let c = open_in_memory().unwrap();
        assert_eq!(
            table_names(&c),
            vec!["places", "raw_logs", "user_dictionary", "visits"]
        );
    }

    #[test]
    fn open_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("areitu.db");
        open(&path).unwrap();
        let c = open(&path).unwrap();
        assert_eq!(table_names(&c).len(), 4);
    }

    #[test]
    fn foreign_keys_enabled() {
        let c = open_in_memory().unwrap();
        let on: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(on, 1);
    }
}
```

`crates/areitu-core/src/lib.rs`（`cargo new` の中身を置き換え）:

```rust
pub mod db;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

```bash
cd crates/areitu-core && cargo add --dev tempfile && cd ../..
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p areitu-core db::`
Expected: FAIL（`open_in_memory` / `open` が未定義でコンパイルエラー）

- [ ] **Step 4: 最小実装**

`crates/areitu-core/src/db.rs` の先頭（tests モジュールの上）:

```rust
use rusqlite::Connection;
use std::path::Path;

use crate::Result;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS places (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS visits (
  id INTEGER PRIMARY KEY,
  place_id INTEGER NOT NULL REFERENCES places(id) ON DELETE CASCADE,
  started_at TEXT NOT NULL,
  ended_at TEXT NOT NULL,
  method TEXT NOT NULL,
  UNIQUE(place_id, started_at)
);
CREATE TABLE IF NOT EXISTS raw_logs (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL,
  source_id TEXT NOT NULL,
  occurred_at TEXT NOT NULL,
  ended_at TEXT,
  lat REAL,
  lon REAL,
  text TEXT,
  visit_id INTEGER REFERENCES visits(id) ON DELETE SET NULL,
  UNIQUE(source, source_id)
);
CREATE TABLE IF NOT EXISTS user_dictionary (
  id INTEGER PRIMARY KEY,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  guessed_name TEXT NOT NULL,
  corrected_name TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_visits_place ON visits(place_id);
CREATE INDEX IF NOT EXISTS idx_raw_logs_time ON raw_logs(occurred_at);
"#;

pub fn open(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    init(&c)?;
    Ok(c)
}

pub fn open_in_memory() -> Result<Connection> {
    let c = Connection::open_in_memory()?;
    init(&c)?;
    Ok(c)
}

fn init(c: &Connection) -> Result<()> {
    c.pragma_update(None, "foreign_keys", true)?;
    c.execute_batch(SCHEMA)?;
    Ok(())
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p areitu-core db::`
Expected: PASS（3 tests）

- [ ] **Step 6: CI を追加**

`.github/workflows/ci.yml`:

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
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - run: cargo test --workspace
      - run: cargo clippy --workspace --all-targets -- -D warnings
```

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 警告なし

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock LICENSE .gitignore .github crates/areitu-core
git commit -m "feat(core): add workspace, SQLite schema and CI"
```

---

### Task 2: RawLog モデルと raw_logs の保存

**Files:**
- Create: `crates/areitu-core/src/model.rs`, `crates/areitu-core/src/store.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod model; pub mod store;` を追加）

**Interfaces:**
- Consumes: `db::open_in_memory`, `Result`
- Produces:
  - `model::Source { Photo, Calendar }`、`Source::as_str(&self) -> &'static str`、`Source::parse(&str) -> Option<Source>`
  - `model::RawLog { source: Source, source_id: String, occurred_at: NaiveDateTime, ended_at: Option<NaiveDateTime>, lat: Option<f64>, lon: Option<f64>, text: Option<String> }`
  - `store::upsert_raw_log(&Connection, &RawLog) -> Result<()>`
  - `store::unassigned_raw_logs(&Connection) -> Result<Vec<(i64, RawLog)>>`（`visit_id IS NULL`、`occurred_at` 昇順）

- [ ] **Step 1: モデルを書く**

`crates/areitu-core/src/model.rs`:

```rust
use chrono::NaiveDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Photo,
    Calendar,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Photo => "photo",
            Source::Calendar => "calendar",
        }
    }

    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "photo" => Some(Source::Photo),
            "calendar" => Some(Source::Calendar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawLog {
    pub source: Source,
    pub source_id: String,
    pub occurred_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub text: Option<String>,
}
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/areitu-core/src/store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{RawLog, Source};
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn event(id: &str, at: &str, text: &str) -> RawLog {
        RawLog {
            source: Source::Calendar,
            source_id: id.into(),
            occurred_at: t(at),
            ended_at: Some(t(at)),
            lat: None,
            lon: None,
            text: Some(text.into()),
        }
    }

    #[test]
    fn upsert_twice_keeps_one_row() {
        let c = open_in_memory().unwrap();
        let log = event("e1", "2026-09-01 12:00", "ランチ");
        upsert_raw_log(&c, &log).unwrap();
        upsert_raw_log(&c, &log).unwrap();
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_changed_fields() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 12:00", "ランチ")).unwrap();
        upsert_raw_log(&c, &event("e1", "2026-09-01 13:00", "ランチ（変更）")).unwrap();
        let logs = unassigned_raw_logs(&c).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1.occurred_at, t("2026-09-01 13:00"));
        assert_eq!(logs[0].1.text.as_deref(), Some("ランチ（変更）"));
    }

    #[test]
    fn unassigned_is_sorted_by_time() {
        let c = open_in_memory().unwrap();
        upsert_raw_log(&c, &event("b", "2026-09-02 10:00", "B")).unwrap();
        upsert_raw_log(&c, &event("a", "2026-09-01 10:00", "A")).unwrap();
        let ids: Vec<String> = unassigned_raw_logs(&c)
            .unwrap()
            .into_iter()
            .map(|(_, l)| l.source_id)
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
```

`lib.rs` に `pub mod model;` と `pub mod store;` を追加。

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p areitu-core store::`
Expected: FAIL（`upsert_raw_log` 未定義）

- [ ] **Step 4: 最小実装**

`store.rs` の先頭:

```rust
use rusqlite::{params, Connection};

use crate::model::{RawLog, Source};
use crate::{Error, Result};

pub fn upsert_raw_log(conn: &Connection, log: &RawLog) -> Result<()> {
    conn.execute(
        "INSERT INTO raw_logs (source, source_id, occurred_at, ended_at, lat, lon, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(source, source_id) DO UPDATE SET
           occurred_at = excluded.occurred_at,
           ended_at = excluded.ended_at,
           lat = excluded.lat,
           lon = excluded.lon,
           text = excluded.text
         WHERE raw_logs.visit_id IS NULL",
        params![
            log.source.as_str(),
            log.source_id,
            log.occurred_at,
            log.ended_at,
            log.lat,
            log.lon,
            log.text
        ],
    )?;
    Ok(())
}

pub fn unassigned_raw_logs(conn: &Connection) -> Result<Vec<(i64, RawLog)>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, source_id, occurred_at, ended_at, lat, lon, text
         FROM raw_logs WHERE visit_id IS NULL ORDER BY occurred_at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        let source: String = r.get(1)?;
        Ok((
            r.get::<_, i64>(0)?,
            source,
            RawLog {
                source: Source::Photo,
                source_id: r.get(2)?,
                occurred_at: r.get(3)?,
                ended_at: r.get(4)?,
                lat: r.get(5)?,
                lon: r.get(6)?,
                text: r.get(7)?,
            },
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, source, mut log) = row?;
        log.source = Source::parse(&source)
            .ok_or_else(|| Error::Invalid(format!("unknown source: {source}")))?;
        out.push((id, log));
    }
    Ok(out)
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test -p areitu-core store::`
Expected: PASS（3 tests）

- [ ] **Step 6: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): add RawLog model and raw_logs upsert"
```

---

### Task 3: 写真 Exif 抽出とフォルダ走査

**Files:**
- Create: `crates/areitu-core/src/exif.rs`, `crates/areitu-core/src/scan.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod exif; pub mod scan;` と `testutil` を追加）

**Interfaces:**
- Consumes: `store::upsert_raw_log`, `model::{RawLog, Source}`
- Produces:
  - `exif::PhotoMeta { taken_at: NaiveDateTime, lat: Option<f64>, lon: Option<f64> }`
  - `exif::parse_exif(&::exif::Exif) -> Option<PhotoMeta>`
  - `exif::read_photo(&Path) -> Option<PhotoMeta>`
  - `scan::ScanReport { seen: usize, inserted: usize, skipped: usize }`
  - `scan::scan_photos(&Connection, &Path) -> Result<ScanReport>`
  - `testutil::{ascii, dms, tiff, jpeg}`（テスト専用）

- [ ] **Step 1: 依存追加とテスト用ヘルパー**

```bash
cd crates/areitu-core && cargo add kamadak-exif walkdir && cd ../..
```

`lib.rs` 末尾に追加:

```rust
pub mod exif;
pub mod scan;

#[cfg(test)]
pub(crate) mod testutil {
    use ::exif::{experimental::Writer, Field, In, Rational, Tag, Value};

    pub fn ascii(tag: Tag, s: &str) -> Field {
        Field { tag, ifd_num: In::PRIMARY, value: Value::Ascii(vec![s.as_bytes().to_vec()]) }
    }

    pub fn dms(tag: Tag, d: u32, m: u32, s_num: u32, s_den: u32) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(vec![
                Rational { num: d, denom: 1 },
                Rational { num: m, denom: 1 },
                Rational { num: s_num, denom: s_den },
            ]),
        }
    }

    pub fn tiff(fields: &[Field]) -> Vec<u8> {
        let mut w = Writer::new();
        for f in fields {
            w.push_field(f);
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        w.write(&mut buf, false).unwrap();
        buf.into_inner()
    }

    pub fn jpeg(tiff: &[u8]) -> Vec<u8> {
        let len = (2 + 6 + tiff.len()) as u16;
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(b"Exif\0\0");
        v.extend_from_slice(tiff);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }
}
```

注意: crate 名 `kamadak-exif` はコード上 `exif` になり、自前モジュール `crate::exif` と名前が重なる。外部 crate は必ず `::exif::` と先頭 `::` 付きで参照する。

- [ ] **Step 2: Exif パースの失敗するテストを書く**

`crates/areitu-core/src/exif.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{ascii, dms, tiff};
    use ::exif::{Reader, Tag};

    fn parse(fields: &[::exif::Field]) -> Option<PhotoMeta> {
        let exif = Reader::new().read_raw(tiff(fields)).unwrap();
        parse_exif(&exif)
    }

    fn dt() -> ::exif::Field {
        ascii(Tag::DateTimeOriginal, "2026:09:01 12:34:56")
    }

    #[test]
    fn reads_time_and_north_east_gps() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 35, 40, 522, 10),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 139, 46, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ])
        .unwrap();
        assert_eq!(m.taken_at.to_string(), "2026-09-01 12:34:56");
        assert!((m.lat.unwrap() - 35.681166).abs() < 1e-5);
        assert!((m.lon.unwrap() - 139.766666).abs() < 1e-5);
    }

    #[test]
    fn south_and_west_are_negative() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 33, 52, 0, 1),
            ascii(Tag::GPSLatitudeRef, "S"),
            dms(Tag::GPSLongitude, 70, 40, 0, 1),
            ascii(Tag::GPSLongitudeRef, "W"),
        ])
        .unwrap();
        assert!(m.lat.unwrap() < 0.0);
        assert!(m.lon.unwrap() < 0.0);
    }

    #[test]
    fn no_gps_gives_time_only() {
        let m = parse(&[dt()]).unwrap();
        assert_eq!(m.lat, None);
        assert_eq!(m.lon, None);
    }

    #[test]
    fn zero_zero_gps_is_treated_as_missing() {
        let m = parse(&[
            dt(),
            dms(Tag::GPSLatitude, 0, 0, 0, 1),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 0, 0, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ])
        .unwrap();
        assert_eq!(m.lat, None);
    }

    #[test]
    fn missing_datetime_returns_none() {
        assert_eq!(parse(&[ascii(Tag::Make, "Apple")]), None);
    }
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p areitu-core exif::`
Expected: FAIL（`PhotoMeta` / `parse_exif` 未定義）

- [ ] **Step 4: Exif パースを実装**

`exif.rs` の先頭:

```rust
use chrono::NaiveDateTime;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use ::exif::{Exif, In, Reader, Tag, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct PhotoMeta {
    pub taken_at: NaiveDateTime,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

pub fn parse_exif(exif: &Exif) -> Option<PhotoMeta> {
    let field = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?;
    let taken_at = match &field.value {
        Value::Ascii(v) if !v.is_empty() => {
            let s = std::str::from_utf8(&v[0]).ok()?.trim_end_matches('\0');
            NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").ok()?
        }
        _ => return None,
    };
    let lat = coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S');
    let lon = coord(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W');
    let (lat, lon) = match (lat, lon) {
        (Some(a), Some(b)) if !(a == 0.0 && b == 0.0) => (Some(a), Some(b)),
        _ => (None, None),
    };
    Some(PhotoMeta { taken_at, lat, lon })
}

fn coord(exif: &Exif, tag: Tag, ref_tag: Tag, negative: u8) -> Option<f64> {
    let v = match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) if v.len() == 3 && v.iter().all(|r| r.denom != 0) => v,
        _ => return None,
    };
    let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    let is_negative = matches!(
        exif.get_field(ref_tag, In::PRIMARY).map(|f| &f.value),
        Some(Value::Ascii(a)) if a.first().and_then(|s| s.first()) == Some(&negative)
    );
    Some(if is_negative { -deg } else { deg })
}

pub fn read_photo(path: &Path) -> Option<PhotoMeta> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let exif = Reader::new().read_from_container(&mut reader).ok()?;
    parse_exif(&exif)
}
```

Run: `cargo test -p areitu-core exif::`
Expected: PASS（5 tests）

- [ ] **Step 5: フォルダ走査の失敗するテストを書く**

`crates/areitu-core/src/scan.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::store::unassigned_raw_logs;
    use crate::testutil::{ascii, dms, jpeg, tiff};
    use ::exif::Tag;

    fn photo_bytes() -> Vec<u8> {
        jpeg(&tiff(&[
            ascii(Tag::DateTimeOriginal, "2026:09:01 12:00:00"),
            dms(Tag::GPSLatitude, 35, 40, 0, 1),
            ascii(Tag::GPSLatitudeRef, "N"),
            dms(Tag::GPSLongitude, 139, 46, 0, 1),
            ascii(Tag::GPSLongitudeRef, "E"),
        ]))
    }

    #[test]
    fn scans_nested_photos_and_skips_junk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/IMG_0001.JPG"), photo_bytes()).unwrap();
        std::fs::write(dir.path().join("broken.jpg"), b"not a jpeg").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();

        let c = open_in_memory().unwrap();
        let r = scan_photos(&c, dir.path()).unwrap();

        assert_eq!(r, ScanReport { seen: 2, inserted: 1, skipped: 1 });
        let logs = unassigned_raw_logs(&c).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1.source, Source::Photo);
        assert!(logs[0].1.lat.is_some());
    }

    #[test]
    fn rescan_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), photo_bytes()).unwrap();
        let c = open_in_memory().unwrap();
        scan_photos(&c, dir.path()).unwrap();
        scan_photos(&c, dir.path()).unwrap();
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }

    #[test]
    fn missing_dir_is_an_error() {
        let c = open_in_memory().unwrap();
        assert!(scan_photos(&c, Path::new("/no/such/dir/areitu")).is_err());
    }
}
```

- [ ] **Step 6: テストが失敗することを確認**

Run: `cargo test -p areitu-core scan::`
Expected: FAIL（`scan_photos` 未定義）

- [ ] **Step 7: フォルダ走査を実装**

`scan.rs` の先頭:

```rust
use rusqlite::Connection;
use std::path::Path;
use walkdir::WalkDir;

use crate::exif::read_photo;
use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::{Error, Result};

const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "heic", "heif", "png", "tif", "tiff"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub seen: usize,
    pub inserted: usize,
    pub skipped: usize,
}

pub fn scan_photos(conn: &Connection, dir: &Path) -> Result<ScanReport> {
    if !dir.is_dir() {
        return Err(Error::Invalid(format!("not a directory: {}", dir.display())));
    }
    let mut report = ScanReport::default();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let is_photo = entry.file_type().is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| PHOTO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false);
        if !is_photo {
            continue;
        }
        report.seen += 1;
        let Some(meta) = read_photo(path) else {
            report.skipped += 1;
            continue;
        };
        upsert_raw_log(
            conn,
            &RawLog {
                source: Source::Photo,
                source_id: path.to_string_lossy().into_owned(),
                occurred_at: meta.taken_at,
                ended_at: None,
                lat: meta.lat,
                lon: meta.lon,
                text: None,
            },
        )?;
        report.inserted += 1;
    }
    Ok(report)
}
```

- [ ] **Step 8: テストが通ることを確認**

Run: `cargo test -p areitu-core`
Expected: PASS（全テスト）

- [ ] **Step 9: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): extract photo Exif time/GPS and scan folders"
```

---

### Task 4: Google カレンダー予定の取り込み

Phase 1 では OAuth を実装しない（Phase 3）。Google Calendar API `events.list` のレスポンス JSON をファイルから読む。Phase 3 では同じ `parse_events` に API レスポンスを渡す。

**Files:**
- Create: `crates/areitu-core/src/calendar.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod calendar;`）

**Interfaces:**
- Consumes: `store::upsert_raw_log`, `model::{RawLog, Source}`
- Produces:
  - `calendar::CalendarEvent { id: String, title: String, location: Option<String>, start: NaiveDateTime, end: NaiveDateTime }`
  - `calendar::parse_events(&str) -> Result<Vec<CalendarEvent>>`
  - `calendar::to_raw_log(&CalendarEvent) -> RawLog`（`text` = タイトル、場所があれば改行して場所）
  - `calendar::ingest_calendar_file(&Connection, &Path) -> Result<usize>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/calendar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
      "kind": "calendar#events",
      "items": [
        {"id": "a1", "status": "confirmed", "summary": "ランチ",
         "location": "丸の内ビルディング 5F",
         "start": {"dateTime": "2026-09-01T12:00:00+09:00"},
         "end": {"dateTime": "2026-09-01T13:00:00+09:00"}},
        {"id": "a2", "status": "confirmed", "summary": "休暇",
         "start": {"date": "2026-09-02"}, "end": {"date": "2026-09-03"}},
        {"id": "a3", "status": "cancelled", "summary": "中止",
         "start": {"dateTime": "2026-09-04T10:00:00+09:00"},
         "end": {"dateTime": "2026-09-04T11:00:00+09:00"}},
        {"id": "a4", "status": "confirmed",
         "start": {"dateTime": "2026-09-05T03:00:00Z"},
         "end": {"dateTime": "2026-09-05T04:00:00Z"}}
      ]
    }"#;

    #[test]
    fn keeps_timed_confirmed_events_only() {
        let ids: Vec<String> = parse_events(JSON).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["a1", "a4"]);
    }

    #[test]
    fn keeps_wall_clock_time_of_offset() {
        let e = &parse_events(JSON).unwrap()[0];
        assert_eq!(e.start.to_string(), "2026-09-01 12:00:00");
        assert_eq!(e.end.to_string(), "2026-09-01 13:00:00");
    }

    #[test]
    fn untitled_event_gets_placeholder() {
        assert_eq!(parse_events(JSON).unwrap()[1].title, "(無題)");
    }

    #[test]
    fn raw_log_text_contains_title_and_location() {
        let log = to_raw_log(&parse_events(JSON).unwrap()[0]);
        assert_eq!(log.source, Source::Calendar);
        assert_eq!(log.text.as_deref(), Some("ランチ\n丸の内ビルディング 5F"));
        assert_eq!(log.lat, None);
    }

    #[test]
    fn empty_response_is_ok() {
        assert!(parse_events(r#"{"kind":"calendar#events"}"#).unwrap().is_empty());
    }

    #[test]
    fn broken_json_is_error() {
        assert!(parse_events("{").is_err());
    }
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p areitu-core calendar::`
Expected: FAIL（`parse_events` 未定義）

- [ ] **Step 3: 最小実装**

`calendar.rs` の先頭:

```rust
use chrono::{DateTime, NaiveDateTime};
use rusqlite::Connection;
use serde::Deserialize;
use std::path::Path;

use crate::model::{RawLog, Source};
use crate::store::upsert_raw_log;
use crate::Result;

#[derive(Deserialize)]
struct EventsResponse {
    #[serde(default)]
    items: Vec<Event>,
}

#[derive(Deserialize)]
struct Event {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    location: Option<String>,
    start: When,
    end: When,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct When {
    #[serde(default)]
    date_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub location: Option<String>,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

fn wall_clock(s: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.naive_local())
}

pub fn parse_events(json: &str) -> Result<Vec<CalendarEvent>> {
    let resp: EventsResponse = serde_json::from_str(json)?;
    Ok(resp
        .items
        .into_iter()
        .filter(|e| e.status.as_deref() != Some("cancelled"))
        .filter_map(|e| {
            let start = wall_clock(e.start.date_time.as_deref()?)?;
            let end = wall_clock(e.end.date_time.as_deref()?)?;
            let title = e
                .summary
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "(無題)".to_owned());
            let location = e.location.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            Some(CalendarEvent { id: e.id, title, location, start, end })
        })
        .collect())
}

pub fn to_raw_log(e: &CalendarEvent) -> RawLog {
    let text = match &e.location {
        Some(loc) => format!("{}\n{}", e.title, loc),
        None => e.title.clone(),
    };
    RawLog {
        source: Source::Calendar,
        source_id: e.id.clone(),
        occurred_at: e.start,
        ended_at: Some(e.end),
        lat: None,
        lon: None,
        text: Some(text),
    }
}

pub fn ingest_calendar_file(conn: &Connection, path: &Path) -> Result<usize> {
    let events = parse_events(&std::fs::read_to_string(path)?)?;
    for e in &events {
        upsert_raw_log(conn, &to_raw_log(e))?;
    }
    Ok(events.len())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p areitu-core calendar::`
Expected: PASS（6 tests）

- [ ] **Step 5: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): ingest Google Calendar events JSON"
```

---

### Task 5: 訪問候補のクラスタリングとカレンダー突合

**Files:**
- Create: `crates/areitu-core/src/geo.rs`, `crates/areitu-core/src/cluster.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod geo; pub mod cluster;`）

**Interfaces:**
- Consumes: `model::{RawLog, Source}`
- Produces:
  - `geo::haversine_m(a: (f64, f64), b: (f64, f64)) -> f64`（メートル）
  - `cluster::VisitCandidate { started_at: NaiveDateTime, ended_at: NaiveDateTime, lat: f64, lon: f64, log_ids: Vec<i64>, hints: Vec<String> }`
  - `cluster::cluster(&[(i64, RawLog)]) -> Vec<VisitCandidate>`（`started_at` 昇順）
  - 定数 `RADIUS_M = 150.0`, `MAX_GAP_MINUTES = 120`, `HINT_MARGIN_MINUTES = 30`

ルール:
- 座標のあるログだけを時刻順に見る。直前の候補の重心から `RADIUS_M` 以内、かつ直前の候補の終了から `MAX_GAP_MINUTES` 以内なら同じ候補に入れる。それ以外は新しい候補。
- カレンダー予定 `[start, end]` が候補の `[started_at - 30分, ended_at + 30分]` と重なれば、予定の `text` の各行（空行を除く）を `hints` に足す。重なる候補が複数あれば全候補に hints を足し、`log_ids` には最初の候補にだけ足す。

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/geo.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokyo_to_shinjuku_is_about_6km() {
        let d = haversine_m((35.681236, 139.767125), (35.690921, 139.700258));
        assert!((d - 6_140.0).abs() < 100.0, "{d}");
    }

    #[test]
    fn same_point_is_zero() {
        assert_eq!(haversine_m((35.0, 139.0), (35.0, 139.0)), 0.0);
    }
}
```

`crates/areitu-core/src/cluster.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RawLog, Source};
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn photo(id: i64, at: &str, lat: f64, lon: f64) -> (i64, RawLog) {
        (id, RawLog {
            source: Source::Photo,
            source_id: format!("p{id}"),
            occurred_at: t(at),
            ended_at: None,
            lat: Some(lat),
            lon: Some(lon),
            text: None,
        })
    }

    fn event(id: i64, from: &str, to: &str, text: &str) -> (i64, RawLog) {
        (id, RawLog {
            source: Source::Calendar,
            source_id: format!("e{id}"),
            occurred_at: t(from),
            ended_at: Some(t(to)),
            lat: None,
            lon: None,
            text: Some(text.into()),
        })
    }

    const CAFE: (f64, f64) = (35.6812, 139.7671);
    const PARK: (f64, f64) = (35.7148, 139.7745);

    #[test]
    fn nearby_photos_close_in_time_form_one_visit() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
            photo(2, "2026-09-01 12:40", CAFE.0 + 0.0003, CAFE.1),
        ]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].log_ids, vec![1, 2]);
        assert_eq!(v[0].started_at, t("2026-09-01 12:00"));
        assert_eq!(v[0].ended_at, t("2026-09-01 12:40"));
    }

    #[test]
    fn same_place_after_long_gap_is_two_visits() {
        let v = cluster(&[
            photo(1, "2026-09-01 09:00", CAFE.0, CAFE.1),
            photo(2, "2026-09-01 18:00", CAFE.0, CAFE.1),
        ]);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn two_places_same_day_are_kept_in_order() {
        let v = cluster(&[
            photo(2, "2026-09-01 15:00", PARK.0, PARK.1),
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
        ]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].log_ids, vec![1]);
        assert_eq!(v[1].log_ids, vec![2]);
    }

    #[test]
    fn photos_without_gps_are_ignored() {
        let mut p = photo(1, "2026-09-01 12:00", 0.0, 0.0);
        p.1.lat = None;
        p.1.lon = None;
        assert!(cluster(&[p]).is_empty());
    }

    #[test]
    fn overlapping_event_adds_hints_and_log_id() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:10", CAFE.0, CAFE.1),
            event(9, "2026-09-01 12:00", "2026-09-01 13:00", "ランチ\n丸の内ビルディング 5F\n"),
        ]);
        assert_eq!(v[0].hints, vec!["ランチ", "丸の内ビルディング 5F"]);
        assert_eq!(v[0].log_ids, vec![1, 9]);
    }

    #[test]
    fn event_within_margin_matches_but_far_event_does_not() {
        let v = cluster(&[
            photo(1, "2026-09-01 12:00", CAFE.0, CAFE.1),
            event(8, "2026-09-01 12:20", "2026-09-01 12:50", "近い"),
            event(9, "2026-09-01 15:00", "2026-09-01 16:00", "遠い"),
        ]);
        assert_eq!(v[0].hints, vec!["近い"]);
    }
}
```

`lib.rs` に `pub mod geo;` と `pub mod cluster;` を追加。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p areitu-core geo:: cluster::`
Expected: FAIL（`haversine_m` / `cluster` 未定義）

- [ ] **Step 3: 最小実装**

`geo.rs` の先頭:

```rust
const EARTH_RADIUS_M: f64 = 6_371_000.0;

pub fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}
```

`cluster.rs` の先頭:

```rust
use chrono::{Duration, NaiveDateTime};

use crate::geo::haversine_m;
use crate::model::{RawLog, Source};

pub const RADIUS_M: f64 = 150.0;
pub const MAX_GAP_MINUTES: i64 = 120;
pub const HINT_MARGIN_MINUTES: i64 = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct VisitCandidate {
    pub started_at: NaiveDateTime,
    pub ended_at: NaiveDateTime,
    pub lat: f64,
    pub lon: f64,
    pub log_ids: Vec<i64>,
    pub hints: Vec<String>,
}

pub fn cluster(logs: &[(i64, RawLog)]) -> Vec<VisitCandidate> {
    let mut points: Vec<(i64, &RawLog, (f64, f64))> = logs
        .iter()
        .filter_map(|(id, l)| Some((*id, l, (l.lat?, l.lon?))))
        .collect();
    points.sort_by_key(|(id, l, _)| (l.occurred_at, *id));

    let mut out: Vec<VisitCandidate> = Vec::new();
    for (id, l, p) in points {
        let end = l.ended_at.unwrap_or(l.occurred_at);
        if let Some(c) = out.last_mut() {
            let near = haversine_m((c.lat, c.lon), p) <= RADIUS_M;
            let soon = l.occurred_at - c.ended_at <= Duration::minutes(MAX_GAP_MINUTES);
            if near && soon {
                let n = c.log_ids.len() as f64;
                c.lat = (c.lat * n + p.0) / (n + 1.0);
                c.lon = (c.lon * n + p.1) / (n + 1.0);
                c.ended_at = c.ended_at.max(end);
                c.log_ids.push(id);
                continue;
            }
        }
        out.push(VisitCandidate {
            started_at: l.occurred_at,
            ended_at: end,
            lat: p.0,
            lon: p.1,
            log_ids: vec![id],
            hints: Vec::new(),
        });
    }
    attach_calendar(&mut out, logs);
    out
}

fn attach_calendar(cands: &mut [VisitCandidate], logs: &[(i64, RawLog)]) {
    let margin = Duration::minutes(HINT_MARGIN_MINUTES);
    for (id, l) in logs.iter().filter(|(_, l)| l.source == Source::Calendar) {
        let start = l.occurred_at;
        let end = l.ended_at.unwrap_or(start);
        let mut assigned = false;
        for c in cands.iter_mut() {
            if start <= c.ended_at + margin && end >= c.started_at - margin {
                if let Some(text) = &l.text {
                    c.hints.extend(
                        text.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned),
                    );
                }
                if !assigned {
                    c.log_ids.push(*id);
                    assigned = true;
                }
            }
        }
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p areitu-core geo:: cluster::`
Expected: PASS（8 tests）

- [ ] **Step 5: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): cluster raw logs into visit candidates with calendar hints"
```

---

### Task 6: ユーザー辞書（推論 第3段階・最優先で適用）

**Files:**
- Create: `crates/areitu-core/src/resolve/mod.rs`, `crates/areitu-core/src/resolve/dictionary.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod resolve;`）

**Interfaces:**
- Consumes: `geo::haversine_m`
- Produces:
  - `resolve::dictionary::DICT_RADIUS_M = 50.0`
  - `resolve::dictionary::lookup(&Connection, lat: f64, lon: f64) -> Result<Option<String>>`（半径内で最も近い修正名。同距離なら新しい方）
  - `resolve::dictionary::record_correction(&Connection, lat, lon, guessed: &str, corrected: &str) -> Result<()>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/areitu-core/src/resolve/mod.rs`:

```rust
pub mod dictionary;
```

`crates/areitu-core/src/resolve/dictionary.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn empty_dictionary_returns_none() {
        let c = open_in_memory().unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap(), None);
    }

    #[test]
    fn nearby_correction_is_found() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.6812, 139.7671, "丸の内ビルディング", "カフェ丸の内").unwrap();
        assert_eq!(
            lookup(&c, 35.6813, 139.7671).unwrap().as_deref(),
            Some("カフェ丸の内")
        );
    }

    #[test]
    fn far_correction_is_ignored() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.6812, 139.7671, "A", "B").unwrap();
        assert_eq!(lookup(&c, 35.6830, 139.7671).unwrap(), None);
    }

    #[test]
    fn latest_correction_wins_at_same_point() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.0, 139.0, "x", "古い名前").unwrap();
        record_correction(&c, 35.0, 139.0, "x", "新しい名前").unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap().as_deref(), Some("新しい名前"));
    }

    #[test]
    fn nearest_correction_wins() {
        let c = open_in_memory().unwrap();
        record_correction(&c, 35.00030, 139.0, "x", "遠い方").unwrap();
        record_correction(&c, 35.00005, 139.0, "x", "近い方").unwrap();
        assert_eq!(lookup(&c, 35.0, 139.0).unwrap().as_deref(), Some("近い方"));
    }
}
```

`lib.rs` に `pub mod resolve;` を追加。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p areitu-core dictionary::`
Expected: FAIL（`lookup` 未定義）

- [ ] **Step 3: 最小実装**

`dictionary.rs` の先頭:

```rust
use rusqlite::{params, Connection};

use crate::geo::haversine_m;
use crate::Result;

pub const DICT_RADIUS_M: f64 = 50.0;
const PREFILTER_DEG: f64 = 0.005;

pub fn lookup(conn: &Connection, lat: f64, lon: f64) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT lat, lon, corrected_name FROM user_dictionary
         WHERE lat BETWEEN ?1 AND ?2 AND lon BETWEEN ?3 AND ?4
         ORDER BY id DESC",
    )?;
    let rows = stmt.query_map(
        params![lat - PREFILTER_DEG, lat + PREFILTER_DEG, lon - PREFILTER_DEG, lon + PREFILTER_DEG],
        |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?, r.get::<_, String>(2)?)),
    )?;
    let mut best: Option<(f64, String)> = None;
    for row in rows {
        let (la, lo, name) = row?;
        let d = haversine_m((lat, lon), (la, lo));
        if d <= DICT_RADIUS_M && best.as_ref().is_none_or(|(b, _)| d < *b) {
            best = Some((d, name));
        }
    }
    Ok(best.map(|(_, n)| n))
}

pub fn record_correction(
    conn: &Connection,
    lat: f64,
    lon: f64,
    guessed: &str,
    corrected: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO user_dictionary (lat, lon, guessed_name, corrected_name) VALUES (?1, ?2, ?3, ?4)",
        params![lat, lon, guessed, corrected],
    )?;
    Ok(())
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p areitu-core dictionary::`
Expected: PASS（5 tests）

- [ ] **Step 5: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): add user dictionary lookup and correction record"
```

---

### Task 7: Nominatim 逆ジオコーディング（推論 第1段階）

**Files:**
- Create: `crates/areitu-core/src/resolve/geocode.rs`
- Modify: `crates/areitu-core/src/resolve/mod.rs`（`pub mod geocode;`）

**Interfaces:**
- Produces:
  - `resolve::geocode::PoiGuess { name: Option<String>, display_name: String, category: Option<String> }`（`Clone`）
  - `trait ReverseGeocoder { fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>>; }`
  - `resolve::geocode::parse_reverse(&str) -> Result<Option<PoiGuess>>`
  - `resolve::geocode::Nominatim::new(user_agent: &str) -> Result<Nominatim>`、`Nominatim::with_base_url(self, url: &str) -> Nominatim`

- [ ] **Step 1: 依存追加と失敗するテストを書く**

```bash
cd crates/areitu-core && cargo add reqwest --features blocking,json && cd ../..
```

`crates/areitu-core/src/resolve/geocode.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_poi() {
        let g = parse_reverse(
            r#"{"place_id":1,"category":"amenity","type":"cafe","name":"ブルーボトルコーヒー",
                "display_name":"ブルーボトルコーヒー, 丸の内, 千代田区, 東京都, 日本"}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(g.name.as_deref(), Some("ブルーボトルコーヒー"));
        assert_eq!(g.category.as_deref(), Some("amenity"));
    }

    #[test]
    fn empty_name_becomes_none() {
        let g = parse_reverse(r#"{"name":"","display_name":"1-1, 丸の内, 千代田区"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(g.name, None);
        assert_eq!(g.display_name, "1-1, 丸の内, 千代田区");
    }

    #[test]
    fn error_response_is_none() {
        assert_eq!(parse_reverse(r#"{"error":"Unable to geocode"}"#).unwrap(), None);
    }

    #[test]
    fn broken_json_is_error() {
        assert!(parse_reverse("<html>").is_err());
    }

    #[test]
    #[ignore = "hits the real Nominatim API"]
    fn live_tokyo_station() {
        let n = Nominatim::new("AREITU-test/0.1 (+https://github.com/ikeikeikeda66/AREITU)").unwrap();
        let g = n.reverse(35.681236, 139.767125).unwrap().unwrap();
        assert!(g.display_name.contains("千代田区"), "{}", g.display_name);
    }
}
```

`resolve/mod.rs` に `pub mod geocode;` を追加。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p areitu-core geocode::`
Expected: FAIL（`parse_reverse` 未定義）

- [ ] **Step 3: 最小実装**

`geocode.rs` の先頭:

```rust
use serde::Deserialize;
use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::{Error, Result};

const MIN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq)]
pub struct PoiGuess {
    pub name: Option<String>,
    pub display_name: String,
    pub category: Option<String>,
}

pub trait ReverseGeocoder {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>>;
}

#[derive(Deserialize)]
struct Reverse {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub fn parse_reverse(json: &str) -> Result<Option<PoiGuess>> {
    let r: Reverse = serde_json::from_str(json)?;
    if r.error.is_some() {
        return Ok(None);
    }
    let Some(display_name) = r.display_name else {
        return Ok(None);
    };
    Ok(Some(PoiGuess {
        name: r.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()),
        display_name,
        category: r.category,
    }))
}

pub struct Nominatim {
    client: reqwest::blocking::Client,
    base_url: String,
    last_call: Cell<Option<Instant>>,
}

impl Nominatim {
    pub fn new(user_agent: &str) -> Result<Nominatim> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Nominatim {
            client,
            base_url: "https://nominatim.openstreetmap.org".to_owned(),
            last_call: Cell::new(None),
        })
    }

    pub fn with_base_url(mut self, url: &str) -> Nominatim {
        self.base_url = url.trim_end_matches('/').to_owned();
        self
    }

    fn throttle(&self) {
        if let Some(last) = self.last_call.get() {
            let elapsed = last.elapsed();
            if elapsed < MIN_INTERVAL {
                std::thread::sleep(MIN_INTERVAL - elapsed);
            }
        }
        self.last_call.set(Some(Instant::now()));
    }
}

impl ReverseGeocoder for Nominatim {
    fn reverse(&self, lat: f64, lon: f64) -> Result<Option<PoiGuess>> {
        self.throttle();
        let (lat, lon) = (lat.to_string(), lon.to_string());
        let resp = self
            .client
            .get(format!("{}/reverse", self.base_url))
            .query(&[
                ("format", "jsonv2"),
                ("lat", lat.as_str()),
                ("lon", lon.as_str()),
                ("zoom", "18"),
                ("accept-language", "ja"),
            ])
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("nominatim status {}", resp.status())));
        }
        parse_reverse(&resp.text().map_err(|e| Error::Http(e.to_string()))?)
    }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cargo test -p areitu-core geocode::`
Expected: PASS（4 passed, 1 ignored）

実 API を1回だけ確認: `cargo test -p areitu-core live_tokyo_station -- --ignored`
Expected: PASS（ネットワークがない環境ではスキップしてよい）

- [ ] **Step 5: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): add Nominatim reverse geocoder with 1 req/s throttle"
```

---

### Task 8: LLM フォールバック（推論 第2段階）と Resolver

**Files:**
- Create: `crates/areitu-core/src/resolve/llm.rs`
- Modify: `crates/areitu-core/src/resolve/mod.rs`（`pub mod llm;` と `Resolver` を追加）
- Modify: `crates/areitu-core/src/lib.rs`（`testutil` にフェイクを追加）

**Interfaces:**
- Consumes: `cluster::VisitCandidate`, `resolve::dictionary::lookup`, `resolve::geocode::{PoiGuess, ReverseGeocoder}`
- Produces:
  - `trait LlmClient { fn complete_json(&self, prompt: &str) -> Result<String>; }`
  - `resolve::llm::build_prompt(&VisitCandidate, Option<&PoiGuess>) -> String`
  - `resolve::llm::LlmAnswer { name: String, confidence: f64 }`、`parse_answer(&str) -> Option<LlmAnswer>`
  - `resolve::llm::Ollama::new(base_url: &str, model: &str) -> Result<Ollama>`
  - `resolve::Method { Dictionary, Nominatim, Llm, Fallback }`、`Method::as_str(&self) -> &'static str`
  - `resolve::Resolution { name: String, method: Method }`
  - `resolve::Resolver<'a> { geocoder: &'a dyn ReverseGeocoder, llm: Option<&'a dyn LlmClient>, min_confidence: f64 }`
  - `Resolver::resolve(&self, &Connection, &VisitCandidate) -> Result<Resolution>`
  - `testutil::{FakeGeocoder, FailingGeocoder, PanicGeocoder, FakeLlm, candidate}`

推論ルール（`Resolver::resolve`）:
1. 辞書に一致 → `Dictionary`（ジオコーダーを呼ばない）
2. ジオコーダーを呼ぶ。失敗はエラーとして返す（呼び出し側が再試行する）
3. LLM を使う条件: POI 名なし、または hints があり POI 名とどの hint も部分一致しない
4. LLM の回答が `min_confidence` 以上 → `Llm`。LLM の通信失敗・解釈不能・低確信度は無視して次へ
5. POI 名あり → `Nominatim`
6. それ以外 → `Fallback`（`display_name` の最初のカンマ区切り要素、なければ `不明な場所 (lat, lon)`）

- [ ] **Step 1: LLM 部分の失敗するテストを書く**

`crates/areitu-core/src/resolve/llm.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::candidate;

    #[test]
    fn prompt_contains_time_coords_poi_and_hints() {
        let c = candidate(&["ランチ", "丸の内ビルディング 5F"]);
        let poi = PoiGuess {
            name: Some("丸の内ビルディング".into()),
            display_name: "丸の内ビルディング, 千代田区".into(),
            category: None,
        };
        let p = build_prompt(&c, Some(&poi));
        assert!(p.contains("2026-09-01 12:00"));
        assert!(p.contains("35.681200"));
        assert!(p.contains("丸の内ビルディング, 千代田区"));
        assert!(p.contains("- 丸の内ビルディング 5F"));
        assert!(p.contains("\"confidence\""));
    }

    #[test]
    fn parses_plain_json_answer() {
        let a = parse_answer(r#"{"name": " カフェ丸の内 ", "confidence": 0.8}"#).unwrap();
        assert_eq!(a.name, "カフェ丸の内");
        assert_eq!(a.confidence, 0.8);
    }

    #[test]
    fn parses_json_wrapped_in_text() {
        let a = parse_answer("答え: {\"name\":\"A\",\"confidence\":1.5} 以上").unwrap();
        assert_eq!(a.confidence, 1.0);
    }

    #[test]
    fn rejects_empty_name_and_garbage() {
        assert_eq!(parse_answer(r#"{"name":"  ","confidence":0.9}"#), None);
        assert_eq!(parse_answer("わかりません"), None);
    }
}
```

`lib.rs` の `testutil` モジュール末尾に追加:

```rust
    use crate::cluster::VisitCandidate;
    use crate::resolve::geocode::{PoiGuess, ReverseGeocoder};
    use crate::resolve::llm::LlmClient;
    use crate::{Error, Result};

    pub fn candidate(hints: &[&str]) -> VisitCandidate {
        let t = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        VisitCandidate {
            started_at: t("2026-09-01 12:00"),
            ended_at: t("2026-09-01 12:45"),
            lat: 35.6812,
            lon: 139.7671,
            log_ids: vec![],
            hints: hints.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub struct FakeGeocoder(pub Option<PoiGuess>);
    impl ReverseGeocoder for FakeGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            Ok(self.0.clone())
        }
    }

    pub struct FailingGeocoder;
    impl ReverseGeocoder for FailingGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            Err(Error::Http("offline".into()))
        }
    }

    pub struct PanicGeocoder;
    impl ReverseGeocoder for PanicGeocoder {
        fn reverse(&self, _: f64, _: f64) -> Result<Option<PoiGuess>> {
            panic!("geocoder must not be called")
        }
    }

    pub struct FakeLlm(pub std::result::Result<String, String>);
    impl LlmClient for FakeLlm {
        fn complete_json(&self, _: &str) -> Result<String> {
            self.0.clone().map_err(Error::Http)
        }
    }
```

`resolve/mod.rs` に `pub mod llm;` を追加。

- [ ] **Step 2: テストが失敗することを確認**

Run: `cargo test -p areitu-core llm::`
Expected: FAIL（`build_prompt` / `LlmClient` 未定義）

- [ ] **Step 3: LLM 部分を実装**

`llm.rs` の先頭:

```rust
use serde::Deserialize;
use std::time::Duration;

use crate::cluster::VisitCandidate;
use crate::resolve::geocode::PoiGuess;
use crate::{Error, Result};

pub trait LlmClient {
    fn complete_json(&self, prompt: &str) -> Result<String>;
}

pub fn build_prompt(cand: &VisitCandidate, poi: Option<&PoiGuess>) -> String {
    let mut s = String::from(
        "あなたは訪問履歴から、訪れた店舗・施設の名前を特定するアシスタントです。\n",
    );
    s += &format!(
        "日時: {} 〜 {}\n",
        cand.started_at.format("%Y-%m-%d %H:%M"),
        cand.ended_at.format("%Y-%m-%d %H:%M")
    );
    s += &format!("座標: {:.6}, {:.6}\n", cand.lat, cand.lon);
    if let Some(p) = poi {
        s += &format!("逆ジオコーディング結果: {}\n", p.display_name);
        if let Some(n) = &p.name {
            s += &format!("候補施設名: {n}\n");
        }
    }
    if !cand.hints.is_empty() {
        s += "同じ時間帯のカレンダー予定:\n";
        for h in &cand.hints {
            s += &format!("- {h}\n");
        }
    }
    s += "最も可能性の高い店舗・施設名を1つ選び、次の形式のJSONだけで答えてください: \
          {\"name\": \"店舗名\", \"confidence\": 0.0から1.0の数値}\n";
    s
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct LlmAnswer {
    pub name: String,
    #[serde(default)]
    pub confidence: f64,
}

pub fn parse_answer(raw: &str) -> Option<LlmAnswer> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    let mut a: LlmAnswer = serde_json::from_str(&raw[start..=end]).ok()?;
    a.name = a.name.trim().to_owned();
    if a.name.is_empty() {
        return None;
    }
    a.confidence = a.confidence.clamp(0.0, 1.0);
    Some(a)
}

pub struct Ollama {
    client: reqwest::blocking::Client,
    base_url: String,
    model: String,
}

impl Ollama {
    pub fn new(base_url: &str, model: &str) -> Result<Ollama> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Ollama {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
        })
    }
}

impl LlmClient for Ollama {
    fn complete_json(&self, prompt: &str) -> Result<String> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "format": "json",
        });
        let resp = self
            .client
            .post(format!("{}/api/generate", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("ollama status {}", resp.status())));
        }
        let v: serde_json::Value = resp.json().map_err(|e| Error::Http(e.to_string()))?;
        v.get("response")
            .and_then(|r| r.as_str())
            .map(str::to_owned)
            .ok_or_else(|| Error::Invalid("ollama: missing response field".into()))
    }
}
```

Run: `cargo test -p areitu-core llm::`
Expected: PASS（4 tests）

- [ ] **Step 4: Resolver の失敗するテストを書く**

`resolve/mod.rs` を次の内容にする（テスト部分）:

```rust
pub mod dictionary;
pub mod geocode;
pub mod llm;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::testutil::{candidate, FailingGeocoder, FakeGeocoder, FakeLlm, PanicGeocoder};
    use geocode::PoiGuess;

    fn poi(name: Option<&str>) -> Option<PoiGuess> {
        Some(PoiGuess {
            name: name.map(str::to_owned),
            display_name: "丸の内ビルディング, 丸の内, 千代田区".into(),
            category: None,
        })
    }

    fn run(geo: &dyn geocode::ReverseGeocoder, llm: Option<&FakeLlm>, hints: &[&str]) -> Resolution {
        let c = open_in_memory().unwrap();
        let llm = llm.map(|l| l as &dyn llm::LlmClient);
        Resolver { geocoder: geo, llm, min_confidence: 0.6 }
            .resolve(&c, &candidate(hints))
            .unwrap()
    }

    #[test]
    fn dictionary_wins_without_calling_geocoder() {
        let c = open_in_memory().unwrap();
        dictionary::record_correction(&c, 35.6812, 139.7671, "x", "カフェ丸の内").unwrap();
        let r = Resolver { geocoder: &PanicGeocoder, llm: None, min_confidence: 0.6 }
            .resolve(&c, &candidate(&[]))
            .unwrap();
        assert_eq!(r, Resolution { name: "カフェ丸の内".into(), method: Method::Dictionary });
    }

    #[test]
    fn named_poi_without_hints_uses_nominatim() {
        let llm = FakeLlm(Ok(r#"{"name":"別の店","confidence":0.9}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("東京駅"))), Some(&llm), &[]);
        assert_eq!(r, Resolution { name: "東京駅".into(), method: Method::Nominatim });
    }

    #[test]
    fn hint_agreeing_with_poi_skips_llm() {
        let llm = FakeLlm(Ok(r#"{"name":"別の店","confidence":0.9}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["丸の内ビルディング 5F"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn conflicting_hint_uses_confident_llm() {
        let llm = FakeLlm(Ok(r#"{"name":"カフェ丸の内","confidence":0.8}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["カフェ丸の内でランチ"]);
        assert_eq!(r, Resolution { name: "カフェ丸の内".into(), method: Method::Llm });
    }

    #[test]
    fn low_confidence_llm_falls_back_to_nominatim() {
        let llm = FakeLlm(Ok(r#"{"name":"たぶんここ","confidence":0.3}"#.into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["ランチ"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn llm_failure_falls_back_to_nominatim() {
        let llm = FakeLlm(Err("connection refused".into()));
        let r = run(&FakeGeocoder(poi(Some("丸の内ビルディング"))), Some(&llm), &["ランチ"]);
        assert_eq!(r.method, Method::Nominatim);
    }

    #[test]
    fn nameless_poi_without_llm_uses_first_address_part() {
        let r = run(&FakeGeocoder(poi(None)), None, &[]);
        assert_eq!(r, Resolution { name: "丸の内ビルディング".into(), method: Method::Fallback });
    }

    #[test]
    fn nothing_found_gives_unknown_place() {
        let r = run(&FakeGeocoder(None), None, &[]);
        assert_eq!(r.name, "不明な場所 (35.68120, 139.76710)");
        assert_eq!(r.method, Method::Fallback);
    }

    #[test]
    fn geocoder_error_is_returned() {
        let c = open_in_memory().unwrap();
        let r = Resolver { geocoder: &FailingGeocoder, llm: None, min_confidence: 0.6 }
            .resolve(&c, &candidate(&[]));
        assert!(r.is_err());
    }
}
```

- [ ] **Step 5: テストが失敗することを確認**

Run: `cargo test -p areitu-core resolve::tests`
Expected: FAIL（`Resolver` / `Resolution` / `Method` 未定義）

- [ ] **Step 6: Resolver を実装**

`resolve/mod.rs` の `pub mod llm;` の下に追加:

```rust
use rusqlite::Connection;

use crate::cluster::VisitCandidate;
use crate::Result;
use geocode::ReverseGeocoder;
use llm::{build_prompt, parse_answer, LlmClient};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Dictionary,
    Nominatim,
    Llm,
    Fallback,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Dictionary => "dictionary",
            Method::Nominatim => "nominatim",
            Method::Llm => "llm",
            Method::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub name: String,
    pub method: Method,
}

pub struct Resolver<'a> {
    pub geocoder: &'a dyn ReverseGeocoder,
    pub llm: Option<&'a dyn LlmClient>,
    pub min_confidence: f64,
}

impl Resolver<'_> {
    pub fn resolve(&self, conn: &Connection, cand: &VisitCandidate) -> Result<Resolution> {
        if let Some(name) = dictionary::lookup(conn, cand.lat, cand.lon)? {
            return Ok(Resolution { name, method: Method::Dictionary });
        }
        let poi = self.geocoder.reverse(cand.lat, cand.lon)?;
        let poi_name = poi.as_ref().and_then(|p| p.name.clone());
        let agrees = |n: &str| cand.hints.iter().any(|h| h.contains(n) || n.contains(h.as_str()));
        let needs_llm = match &poi_name {
            None => true,
            Some(n) => !cand.hints.is_empty() && !agrees(n),
        };
        if needs_llm {
            if let Some(llm) = self.llm {
                if let Ok(raw) = llm.complete_json(&build_prompt(cand, poi.as_ref())) {
                    if let Some(a) = parse_answer(&raw) {
                        if a.confidence >= self.min_confidence {
                            return Ok(Resolution { name: a.name, method: Method::Llm });
                        }
                    }
                }
            }
        }
        if let Some(name) = poi_name {
            return Ok(Resolution { name, method: Method::Nominatim });
        }
        let name = poi
            .map(|p| p.display_name.split(',').next().unwrap_or("").trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("不明な場所 ({:.5}, {:.5})", cand.lat, cand.lon));
        Ok(Resolution { name, method: Method::Fallback })
    }
}
```

- [ ] **Step 7: テストが通ることを確認**

Run: `cargo test -p areitu-core`
Expected: PASS（全テスト）

- [ ] **Step 8: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): add Ollama LLM fallback and 3-stage resolver"
```

---

### Task 9: 訪問の保存・場所名の修正・検索

**Files:**
- Modify: `crates/areitu-core/src/store.rs`（場所・訪問関数を追加）
- Create: `crates/areitu-core/src/pipeline.rs`, `crates/areitu-core/src/query.rs`
- Modify: `crates/areitu-core/src/lib.rs`（`pub mod pipeline; pub mod query;`）

**Interfaces:**
- Consumes: `cluster::{cluster, VisitCandidate}`, `resolve::{Resolver, Method}`, `resolve::dictionary::record_correction`, `store::unassigned_raw_logs`
- Produces:
  - `store::PLACE_MERGE_M = 200.0`
  - `store::find_or_create_place(&Connection, name: &str, lat: f64, lon: f64) -> Result<i64>`
  - `store::insert_visit(&Connection, place_id: i64, &VisitCandidate, method: &str) -> Result<i64>`
  - `store::assign_logs(&Connection, visit_id: i64, log_ids: &[i64]) -> Result<()>`
  - `store::rename_place(&mut Connection, place_id: i64, new_name: &str) -> Result<i64>`（戻り値は名前変更後の place id。同名の近い場所があれば統合してその id）
  - `pipeline::BuildReport { visits: usize, failed: usize, errors: Vec<String> }`
  - `pipeline::build_visits(&mut Connection, &Resolver) -> Result<BuildReport>`
  - `query::SortBy { Count, Recent }`
  - `query::PlaceSummary { id: i64, name: String, visit_count: i64, last_visit: NaiveDateTime }`
  - `query::list_places(&Connection, SortBy, keyword: Option<&str>) -> Result<Vec<PlaceSummary>>`
  - `query::visits_of(&Connection, place_id: i64) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>>`（新しい順）

- [ ] **Step 1: pipeline の失敗するテストを書く**

`crates/areitu-core/src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{RawLog, Source};
    use crate::resolve::geocode::PoiGuess;
    use crate::store::{unassigned_raw_logs, upsert_raw_log};
    use crate::testutil::{FailingGeocoder, FakeGeocoder};
    use chrono::NaiveDateTime;

    fn photo(id: &str, at: &str, lat: f64, lon: f64) -> RawLog {
        RawLog {
            source: Source::Photo,
            source_id: id.into(),
            occurred_at: NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap(),
            ended_at: None,
            lat: Some(lat),
            lon: Some(lon),
            text: None,
        }
    }

    fn geo(name: &str) -> FakeGeocoder {
        FakeGeocoder(Some(PoiGuess { name: Some(name.into()), display_name: name.into(), category: None }))
    }

    fn count(c: &rusqlite::Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn builds_visits_and_assigns_logs() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        upsert_raw_log(&c, &photo("b", "2026-09-08 12:00", 35.6812, 139.7671)).unwrap();
        let g = geo("カフェ丸の内");
        let r = build_visits(&mut c, &Resolver { geocoder: &g, llm: None, min_confidence: 0.6 }).unwrap();
        assert_eq!((r.visits, r.failed), (2, 0));
        assert_eq!(count(&c, "places"), 1);
        assert_eq!(count(&c, "visits"), 2);
        assert!(unassigned_raw_logs(&c).unwrap().is_empty());
    }

    #[test]
    fn rerun_creates_nothing_new() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        let g = geo("カフェ丸の内");
        let res = Resolver { geocoder: &g, llm: None, min_confidence: 0.6 };
        build_visits(&mut c, &res).unwrap();
        let r = build_visits(&mut c, &res).unwrap();
        assert_eq!(r.visits, 0);
        assert_eq!(count(&c, "visits"), 1);
    }

    #[test]
    fn offline_geocoder_reports_failure_and_keeps_logs_for_retry() {
        let mut c = open_in_memory().unwrap();
        upsert_raw_log(&c, &photo("a", "2026-09-01 12:00", 35.6812, 139.7671)).unwrap();
        let r = build_visits(&mut c, &Resolver { geocoder: &FailingGeocoder, llm: None, min_confidence: 0.6 }).unwrap();
        assert_eq!((r.visits, r.failed), (0, 1));
        assert_eq!(r.errors, vec!["http: offline"]);
        assert_eq!(unassigned_raw_logs(&c).unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: query と rename の失敗するテストを書く**

`crates/areitu-core/src/query.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::store::{find_or_create_place, insert_visit, rename_place};
    use crate::testutil::candidate;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn seed(c: &Connection, name: &str, lat: f64, starts: &[&str]) -> i64 {
        let id = find_or_create_place(c, name, lat, 139.0).unwrap();
        for s in starts {
            let mut cand = candidate(&[]);
            cand.started_at = at(s);
            cand.ended_at = at(s);
            insert_visit(c, id, &cand, "nominatim").unwrap();
        }
        id
    }

    fn names(v: &[PlaceSummary]) -> Vec<&str> {
        v.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn sorts_by_count_and_by_recent() {
        let c = open_in_memory().unwrap();
        seed(&c, "よく行く店", 35.0, &["2026-01-01 12:00", "2026-02-01 12:00", "2026-03-01 12:00"]);
        seed(&c, "最近行った店", 35.1, &["2026-09-01 12:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, None).unwrap()), vec!["よく行く店", "最近行った店"]);
        let recent = list_places(&c, SortBy::Recent, None).unwrap();
        assert_eq!(names(&recent), vec!["最近行った店", "よく行く店"]);
        assert_eq!(recent[1].visit_count, 3);
        assert_eq!(recent[1].last_visit, at("2026-03-01 12:00"));
    }

    #[test]
    fn search_matches_substring() {
        let c = open_in_memory().unwrap();
        seed(&c, "ブルーボトルコーヒー", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "スターバックス", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, Some("ボトル")).unwrap()), vec!["ブルーボトルコーヒー"]);
    }

    #[test]
    fn search_treats_percent_and_underscore_literally() {
        let c = open_in_memory().unwrap();
        seed(&c, "100%果汁スタンド", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "普通の店", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(names(&list_places(&c, SortBy::Count, Some("%")).unwrap()), vec!["100%果汁スタンド"]);
        assert!(list_places(&c, SortBy::Count, Some("_")).unwrap().is_empty());
    }

    #[test]
    fn blank_search_returns_all() {
        let c = open_in_memory().unwrap();
        seed(&c, "A", 35.0, &["2026-09-01 12:00"]);
        seed(&c, "B", 35.1, &["2026-09-01 15:00"]);
        assert_eq!(list_places(&c, SortBy::Count, Some("   ")).unwrap().len(), 2);
    }

    #[test]
    fn visits_of_is_newest_first() {
        let c = open_in_memory().unwrap();
        let id = seed(&c, "A", 35.0, &["2026-01-01 12:00", "2026-09-01 12:00"]);
        let v = visits_of(&c, id).unwrap();
        assert_eq!(v[0].0, at("2026-09-01 12:00"));
    }

    #[test]
    fn rename_updates_name_and_records_dictionary() {
        let mut c = open_in_memory().unwrap();
        let id = seed(&c, "丸の内ビルディング", 35.0, &["2026-09-01 12:00"]);
        assert_eq!(rename_place(&mut c, id, " カフェ丸の内 ").unwrap(), id);
        assert_eq!(names(&list_places(&c, SortBy::Count, None).unwrap()), vec!["カフェ丸の内"]);
        assert_eq!(
            crate::resolve::dictionary::lookup(&c, 35.0, 139.0).unwrap().as_deref(),
            Some("カフェ丸の内")
        );
    }

    #[test]
    fn rename_to_existing_nearby_name_merges_places() {
        let mut c = open_in_memory().unwrap();
        let keep = seed(&c, "カフェ丸の内", 35.0, &["2026-01-01 12:00"]);
        let dup = seed(&c, "丸の内ビルディング", 35.0005, &["2026-02-01 12:00"]);
        assert_eq!(rename_place(&mut c, dup, "カフェ丸の内").unwrap(), keep);
        let all = list_places(&c, SortBy::Count, None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].visit_count, 2);
    }

    #[test]
    fn rename_to_blank_is_rejected() {
        let mut c = open_in_memory().unwrap();
        let id = seed(&c, "A", 35.0, &["2026-09-01 12:00"]);
        assert!(rename_place(&mut c, id, "  ").is_err());
    }

    #[test]
    fn rename_unknown_place_is_error() {
        let mut c = open_in_memory().unwrap();
        assert!(rename_place(&mut c, 999, "X").is_err());
    }
}
```

`lib.rs` に `pub mod pipeline;` と `pub mod query;` を追加。

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p areitu-core pipeline:: query::`
Expected: FAIL（`build_visits` / `list_places` / `find_or_create_place` 等が未定義）

- [ ] **Step 4: store に場所・訪問関数を実装**

`store.rs` の import を次に置き換え、関数を追加する:

```rust
use rusqlite::{params, Connection, OptionalExtension};

use crate::cluster::VisitCandidate;
use crate::geo::haversine_m;
use crate::model::{RawLog, Source};
use crate::resolve::dictionary::record_correction;
use crate::{Error, Result};

pub const PLACE_MERGE_M: f64 = 200.0;

fn find_place(conn: &Connection, name: &str, lat: f64, lon: f64, exclude: Option<i64>) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id, lat, lon FROM places WHERE name = ?1 ORDER BY id")?;
    let rows = stmt.query_map([name], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?)))?;
    for row in rows {
        let (id, la, lo) = row?;
        if Some(id) != exclude && haversine_m((lat, lon), (la, lo)) <= PLACE_MERGE_M {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub fn find_or_create_place(conn: &Connection, name: &str, lat: f64, lon: f64) -> Result<i64> {
    if let Some(id) = find_place(conn, name, lat, lon, None)? {
        return Ok(id);
    }
    conn.execute("INSERT INTO places (name, lat, lon) VALUES (?1, ?2, ?3)", params![name, lat, lon])?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_visit(conn: &Connection, place_id: i64, cand: &VisitCandidate, method: &str) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO visits (place_id, started_at, ended_at, method) VALUES (?1, ?2, ?3, ?4)",
        params![place_id, cand.started_at, cand.ended_at, method],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM visits WHERE place_id = ?1 AND started_at = ?2",
        params![place_id, cand.started_at],
        |r| r.get(0),
    )?)
}

pub fn assign_logs(conn: &Connection, visit_id: i64, log_ids: &[i64]) -> Result<()> {
    let mut stmt = conn.prepare("UPDATE raw_logs SET visit_id = ?1 WHERE id = ?2")?;
    for id in log_ids {
        stmt.execute(params![visit_id, id])?;
    }
    Ok(())
}

pub fn rename_place(conn: &mut Connection, place_id: i64, new_name: &str) -> Result<i64> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(Error::Invalid("place name must not be empty".into()));
    }
    let tx = conn.transaction()?;
    let (old, lat, lon): (String, f64, f64) = tx
        .query_row("SELECT name, lat, lon FROM places WHERE id = ?1", [place_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or_else(|| Error::Invalid(format!("place {place_id} not found")))?;
    record_correction(&tx, lat, lon, &old, new_name)?;
    let result = match find_place(&tx, new_name, lat, lon, Some(place_id))? {
        Some(target) => {
            tx.execute("UPDATE OR IGNORE visits SET place_id = ?1 WHERE place_id = ?2", params![target, place_id])?;
            tx.execute("DELETE FROM places WHERE id = ?1", [place_id])?;
            target
        }
        None => {
            tx.execute("UPDATE places SET name = ?1 WHERE id = ?2", params![new_name, place_id])?;
            place_id
        }
    };
    tx.commit()?;
    Ok(result)
}
```

- [ ] **Step 5: pipeline と query を実装**

`pipeline.rs` の先頭:

```rust
use rusqlite::Connection;

use crate::cluster::cluster;
use crate::resolve::Resolver;
use crate::store::{assign_logs, find_or_create_place, insert_visit, unassigned_raw_logs};
use crate::Result;

#[derive(Debug, Default, PartialEq)]
pub struct BuildReport {
    pub visits: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

pub fn build_visits(conn: &mut Connection, resolver: &Resolver) -> Result<BuildReport> {
    let logs = unassigned_raw_logs(conn)?;
    let mut report = BuildReport::default();
    for cand in cluster(&logs) {
        let res = match resolver.resolve(conn, &cand) {
            Ok(r) => r,
            Err(e) => {
                report.failed += 1;
                report.errors.push(e.to_string());
                continue;
            }
        };
        let tx = conn.transaction()?;
        let place = find_or_create_place(&tx, &res.name, cand.lat, cand.lon)?;
        let visit = insert_visit(&tx, place, &cand, res.method.as_str())?;
        assign_logs(&tx, visit, &cand.log_ids)?;
        tx.commit()?;
        report.visits += 1;
    }
    Ok(report)
}
```

`query.rs` の先頭:

```rust
use chrono::NaiveDateTime;
use rusqlite::{params, Connection};

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Count,
    Recent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceSummary {
    pub id: i64,
    pub name: String,
    pub visit_count: i64,
    pub last_visit: NaiveDateTime,
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub fn list_places(conn: &Connection, sort: SortBy, keyword: Option<&str>) -> Result<Vec<PlaceSummary>> {
    let order = match sort {
        SortBy::Count => "visit_count DESC, last_visit DESC",
        SortBy::Recent => "last_visit DESC, visit_count DESC",
    };
    let sql = format!(
        "SELECT p.id, p.name, COUNT(v.id) AS visit_count, MAX(v.started_at) AS last_visit
         FROM places p JOIN visits v ON v.place_id = p.id
         WHERE ?1 IS NULL OR p.name LIKE ?1 ESCAPE '\\'
         GROUP BY p.id
         ORDER BY {order}, p.id"
    );
    let pattern = keyword
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| format!("%{}%", escape_like(k)));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![pattern], |r| {
        Ok(PlaceSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            visit_count: r.get(2)?,
            last_visit: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn visits_of(conn: &Connection, place_id: i64) -> Result<Vec<(NaiveDateTime, NaiveDateTime)>> {
    let mut stmt = conn.prepare(
        "SELECT started_at, ended_at FROM visits WHERE place_id = ?1 ORDER BY started_at DESC",
    )?;
    let rows = stmt.query_map([place_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
```

- [ ] **Step 6: テストが通ることを確認**

Run: `cargo test -p areitu-core && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS、警告なし

- [ ] **Step 7: Commit**

```bash
git add crates/areitu-core
git commit -m "feat(core): persist visits, rename places with dictionary feedback, search"
```

---

### Task 10: 検証用 CLI

Nominatim とフォールバック LLM プロンプトを実データで検証するための CLI。Phase 2 の GUI ができたら開発者向けツールとして残す。

**Files:**
- Create: `crates/areitu-cli/Cargo.toml`, `crates/areitu-cli/src/main.rs`, `crates/areitu-cli/tests/cli.rs`
- Modify: `Cargo.toml`（workspace members に追加）, `README.md`（使い方を追記）

**Interfaces:**
- Consumes: `db::open`, `scan::scan_photos`, `calendar::ingest_calendar_file`, `resolve::{Resolver, geocode::Nominatim, llm::Ollama}`, `pipeline::build_visits`, `query::{list_places, visits_of, SortBy}`, `store::rename_place`
- Produces: バイナリ `areitu`。サブコマンド `ingest-photos <DIR>` / `ingest-calendar <FILE>` / `build [--ollama-model M] [--ollama-url U] [--min-confidence F]` / `list [--sort count|recent] [--search KW]` / `show <PLACE_ID>` / `rename <PLACE_ID> <NAME>`。全サブコマンド共通 `--db <PATH>`（既定 `areitu.db`）

- [ ] **Step 1: crate を作る**

```bash
cargo new --bin crates/areitu-cli
cd crates/areitu-cli
cargo add areitu-core --path ../areitu-core
cargo add clap --features derive
cargo add anyhow
cargo add --dev tempfile
cd ../..
```

`Cargo.toml`（直下）の members を `["crates/areitu-core", "crates/areitu-cli"]` にする。

`crates/areitu-cli/Cargo.toml` に追記:

```toml
[[bin]]
name = "areitu"
path = "src/main.rs"
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/areitu-cli/tests/cli.rs`:

```rust
use std::process::{Command, Output};

fn areitu(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_areitu"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn ingest_calendar_then_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("areitu.db");
    let cal = dir.path().join("events.json");
    std::fs::write(
        &cal,
        r#"{"items":[{"id":"a1","summary":"ランチ",
            "start":{"dateTime":"2026-09-01T12:00:00+09:00"},
            "end":{"dateTime":"2026-09-01T13:00:00+09:00"}}]}"#,
    )
    .unwrap();

    let out = areitu(&db, &["ingest-calendar", cal.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 件"));

    let out = areitu(&db, &["list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("場所はまだありません"));
}

#[test]
fn rename_unknown_place_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = areitu(&dir.path().join("areitu.db"), &["rename", "999", "X"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("place 999 not found"));
}

#[test]
fn ingest_missing_folder_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = areitu(&dir.path().join("areitu.db"), &["ingest-photos", "/no/such/dir/areitu"]);
    assert!(!out.status.success());
}
```

- [ ] **Step 3: テストが失敗することを確認**

Run: `cargo test -p areitu-cli`
Expected: FAIL（`cargo new` の Hello world はサブコマンドを解釈しない）

- [ ] **Step 4: 実装**

`crates/areitu-cli/src/main.rs`:

```rust
use std::path::PathBuf;

use anyhow::Result;
use areitu_core::{
    calendar::ingest_calendar_file,
    db,
    pipeline::build_visits,
    query::{list_places, visits_of, SortBy},
    resolve::{geocode::Nominatim, llm::{LlmClient, Ollama}, Resolver},
    scan::scan_photos,
    store::rename_place,
};
use clap::{Parser, Subcommand, ValueEnum};

const USER_AGENT: &str = concat!(
    "AREITU/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ikeikeikeda66/AREITU)"
);

#[derive(Parser)]
#[command(name = "areitu", version, about = "あれ、いつ行ったっけ？ 訪問ログ検証用 CLI")]
struct Cli {
    #[arg(long, global = true, default_value = "areitu.db")]
    db: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 写真フォルダを走査して Exif を取り込む
    IngestPhotos { dir: PathBuf },
    /// Google Calendar events.list の JSON を取り込む
    IngestCalendar { file: PathBuf },
    /// 未処理ログから訪問を作り、店舗名を推論する
    Build {
        #[arg(long)]
        ollama_model: Option<String>,
        #[arg(long, default_value = "http://localhost:11434")]
        ollama_url: String,
        #[arg(long, default_value_t = 0.6)]
        min_confidence: f64,
    },
    /// 場所の一覧（検索・並び替え）
    List {
        #[arg(long, value_enum, default_value_t = Sort::Count)]
        sort: Sort,
        #[arg(long)]
        search: Option<String>,
    },
    /// 場所の訪問日時一覧
    Show { place_id: i64 },
    /// 場所名を修正し、ユーザー辞書に記録する
    Rename { place_id: i64, name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    Count,
    Recent,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut conn = db::open(&cli.db)?;
    match cli.cmd {
        Cmd::IngestPhotos { dir } => {
            let r = scan_photos(&conn, &dir)?;
            println!("写真 {} 枚を確認、{} 件取り込み、{} 件スキップ", r.seen, r.inserted, r.skipped);
        }
        Cmd::IngestCalendar { file } => {
            println!("予定 {} 件を取り込みました", ingest_calendar_file(&conn, &file)?);
        }
        Cmd::Build { ollama_model, ollama_url, min_confidence } => {
            let geocoder = Nominatim::new(USER_AGENT)?;
            let ollama = ollama_model.map(|m| Ollama::new(&ollama_url, &m)).transpose()?;
            let resolver = Resolver {
                geocoder: &geocoder,
                llm: ollama.as_ref().map(|o| o as &dyn LlmClient),
                min_confidence,
            };
            let r = build_visits(&mut conn, &resolver)?;
            println!("訪問 {} 件を作成、{} 件失敗（次回再試行）", r.visits, r.failed);
            for e in r.errors {
                eprintln!("  {e}");
            }
        }
        Cmd::List { sort, search } => {
            let sort = match sort {
                Sort::Count => SortBy::Count,
                Sort::Recent => SortBy::Recent,
            };
            let places = list_places(&conn, sort, search.as_deref())?;
            if places.is_empty() {
                println!("場所はまだありません");
            }
            for p in places {
                println!("{}\t{}\t{} 回\t最終 {}", p.id, p.name, p.visit_count, p.last_visit.format("%Y-%m-%d %H:%M"));
            }
        }
        Cmd::Show { place_id } => {
            for (start, end) in visits_of(&conn, place_id)? {
                println!("{} 〜 {}", start.format("%Y-%m-%d %H:%M"), end.format("%H:%M"));
            }
        }
        Cmd::Rename { place_id, name } => {
            let id = rename_place(&mut conn, place_id, &name)?;
            println!("場所 {id} の名前を「{}」にしました", name.trim());
        }
    }
    Ok(())
}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS、警告なし

- [ ] **Step 6: 実データで手動検証（Nominatim + LLM プロンプト）**

```bash
cargo run -p areitu-cli -- --db /tmp/areitu-check.db ingest-photos ~/Pictures/sample
cargo run -p areitu-cli -- --db /tmp/areitu-check.db build
cargo run -p areitu-cli -- --db /tmp/areitu-check.db list --sort recent
```

Ollama がある環境では `build --ollama-model <モデル名>` も試す。推論結果の当たり外れ（method 別件数、誤判定例）を Phase 1 完了 issue にコメントとして残す。

- [ ] **Step 7: README に使い方を追記して Commit**

`README.md` の `## Development` の `TBD` を次に置き換える:

````markdown
Rust stable が必要です。

```bash
cargo test --workspace
cargo run -p areitu-cli -- ingest-photos <写真フォルダ>
cargo run -p areitu-cli -- ingest-calendar <events.json>
cargo run -p areitu-cli -- build [--ollama-model <モデル名>]
cargo run -p areitu-cli -- list --sort recent --search <キーワード>
```
````

```bash
git add Cargo.toml Cargo.lock README.md crates/areitu-cli
git commit -m "feat(cli): add verification CLI for ingest, build, list and rename"
```

---

## 既知の制限（Phase 1 では対応しない）

- `build` は未処理ログだけをクラスタリングする。既存の訪問の直後に撮った写真が後から取り込まれると、別の訪問になる。
- カレンダーの `location` 文字列はジオコーディングしない（座標のないカレンダー予定だけでは訪問を作らない）。
- クラウド LLM（OpenAI / Gemini）、Google Places API は Phase 2 の設定画面で追加する。
