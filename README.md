# AREITU

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

## プライバシーポリシー・利用規約

- プライバシーポリシー: https://ikeikeikeda66.github.io/AREITU/privacy.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/privacy.en.html)）
- 利用規約: https://ikeikeikeda66.github.io/AREITU/terms.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/terms.en.html)）

## 配布パッケージ（未署名ビルド）

インストーラは GitHub Releases に添付されます。`v*` にマッチするタグを push すると `.github/workflows/release.yml` が macOS（universal）と Windows のビルドを行い、リリースを**下書き**として作成します。macOS 用は `.dmg`、Windows 用は `.msi` と NSIS の `.exe` です（`tauri.conf.json` の `bundle.targets` は `all`）。

これらのビルドは Apple の公証も Windows のコード署名も行っていない未署名ビルドです。署名には Apple Developer Program の年会費や Windows 用コード署名証明書の費用がかかるため、現時点では導入しないとオーナーが決めています。そのため OS が警告を表示します。以下の手順で開けます。

### macOS（Gatekeeper）

`.dmg` を開き、`AREITU.app` を `Applications` フォルダにコピーします。

1. `AREITU.app` をダブルクリックする。「開発元を確認できないため開けません」という趣旨のダイアログが出るので閉じる
2. 「システム設定」→「プライバシーとセキュリティ」を開き、下の方にある「セキュリティ」欄の「このまま開く」（Open Anyway）を押す
3. 確認ダイアログでもう一度「開く」を押す（パスワードや Touch ID を求められる場合がある）
4. 2回目以降は通常どおり起動できる

macOS 14 以前では、`AREITU.app` を右クリック（または Control を押しながらクリック）して「開く」を選ぶ方法も使えます。macOS 15 以降ではこの方法が使えないため、上記の「このまま開く」を使ってください。

ターミナルから隔離属性を外すこともできます。

```bash
xattr -dr com.apple.quarantine /Applications/AREITU.app
```

### Windows（SmartScreen）

インストーラ（`.msi` または `.exe`）を実行すると「Windows によって PC が保護されました」と表示されることがあります。

1. 「詳細情報」をクリックする
2. 表示される「実行」ボタンをクリックする

### 自動更新

アプリは起動時に Tauri updater で更新を確認します。確認先は `tauri.conf.json` の `plugins.updater.endpoints` にある GitHub Releases の `latest.json` です。新しいバージョンがあると画面にバナーが表示され、そこからインストールして再起動できます。更新パッケージは minisign の鍵で署名されています。これは OS のコード署名とは別のものです。

`plugins.updater.pubkey` には、`TAURI_SIGNING_PRIVATE_KEY` と対になる公開鍵を設定する必要があります（現時点のリポジトリではプレースホルダのままです）。

`latest.json` は下書きを公開（Publish release）した後に、最新リリースとして配信されます。

## バージョンを上げる手順

バージョン番号は3箇所（`apps/desktop/package.json`、`apps/desktop/src-tauri/Cargo.toml`、`apps/desktop/src-tauri/tauri.conf.json`）にあり、常に一致している必要があります。

1. 3ファイルすべての `version` を同じ値に変更する
2. `cd apps/desktop && npm run check:versions` を実行し、`Versions match: X.Y.Z` と表示されることを確認する（`release.yml` も同じチェックを最初のジョブで実行し、不一致ならビルドしない）
3. 変更をコミットする
4. `git tag vX.Y.Z && git push origin vX.Y.Z` でタグを push する（`v` プレフィックスが必須）
5. GitHub Actions の `release` ワークフローが完了すると、GitHub Releases に下書きのリリースができる。内容を確認してから手動で公開する

### リリースに必要な GitHub Secrets

リポジトリの Secrets に次の4つを登録します。

| 名前 | 用途 |
|---|---|
| `AREITU_GOOGLE_CLIENT_ID` | Google OAuth クライアント ID（ビルド時に埋め込む） |
| `AREITU_GOOGLE_CLIENT_SECRET` | Google OAuth クライアントシークレット（同上） |
| `TAURI_SIGNING_PRIVATE_KEY` | 更新パッケージの minisign 秘密鍵 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 上記秘密鍵のパスワード |

更新パッケージ（`.sig` と `latest.json`）の生成は、リリース専用の設定ファイル `apps/desktop/src-tauri/tauri.release.conf.json`（`bundle.createUpdaterArtifacts: true`）を `release.yml` が `--config` で渡すことで有効になります。通常の `npm run tauri dev` や `tauri build` では署名鍵は不要です。
