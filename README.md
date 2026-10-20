# AREITU

> **お知らせ: デスクトップ版は開発・配布を終了しました。**
> AREITU は iPhone アプリに方針転換しました。デスクトップ版（Tauri）のコードは [`feat/phase3` ブランチ](https://github.com/ikeikeikeda66/AREITU/tree/feat/phase3) に残していますが、メンテナンスとリリースは行いません。GitHub Releases にインストーラはありません。CI も停止しています。
>
> **Notice: the desktop version is discontinued.** AREITU is moving to an iPhone app. The desktop (Tauri) code is kept on the `feat/phase3` branch but is no longer maintained or released. No installers are published.

## プライバシーポリシー・利用規約

- プライバシーポリシー: https://ikeikeikeda66.github.io/AREITU/privacy.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/privacy.en.html)）
- 利用規約: https://ikeikeikeda66.github.io/AREITU/terms.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/terms.en.html)）

## Overview
TBD

## Setup
TBD

## Development
Rust stable が必要です。

```bash
cargo test --workspace
cargo run -p areitu-cli -- ingest-photos <写真フォルダ>
cargo run -p areitu-cli -- ingest-calendar <events.json>
cargo run -p areitu-cli -- build [--ollama-model <モデル名>]
cargo run -p areitu-cli -- list --sort recent --search <キーワード>
```

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
