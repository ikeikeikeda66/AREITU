# Phase 3B: 配布パッケージ・自動更新・OAuth 審査対応 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** タグ push（`v*`）で macOS / Windows 向けインストーラをビルドして GitHub Release の下書きに添付し、minisign 署名による自動更新を提供する。あわせて Tauri の CSP を有効化し、GitHub Pages でプライバシーポリシー・利用規約（日英）を公開して Google OAuth 審査に必要な材料を揃える。コード署名・公証は今回のスコープ外（オーナー判断、2026-09-28）だが、後から秘密情報を追加するだけで有効化できる構造にする。

**Architecture:** GitHub Actions に2つの新しいワークフローを追加する。`release.yml` はタグ push をトリガーに `tauri-apps/tauri-action` で macOS（universal）と Windows（NSIS/MSI）をビルドし、GitHub Release の下書きに添付、`--updater-json` で `latest.json` を同時生成する。`pages.yml` は `docs/pages/` 配下の変更を GitHub Pages（Actions 経由のデプロイ）に公開する。既存の `ci.yml`（push / PR で3 OS の `cargo test` と `npm run test`）はこの変更の影響を受けず、新しい GitHub Secrets が未設定でも green を保つ——Google OAuth クライアント情報とアップデータ署名鍵は `release.yml` だけが参照し、`ci.yml` は参照しない。バージョン番号は `apps/desktop/package.json` / `apps/desktop/src-tauri/Cargo.toml` / `apps/desktop/src-tauri/tauri.conf.json` の3箇所に重複しているため、Node の小さなチェックスクリプトで一致を検証し、`ci.yml` と `release.yml` の両方に組み込む。

**Tech Stack:** GitHub Actions（`tauri-apps/tauri-action`, `actions/configure-pages`, `actions/upload-pages-artifact`, `actions/deploy-pages`）。Tauri 2 の `tauri-plugin-updater`（Rust）と `@tauri-apps/plugin-updater`（JS）、`tauri-plugin-process` / `@tauri-apps/plugin-process`（更新後の再起動用）。署名は minisign（Tauri updater plugin 組み込み、OS 証明書とは別物、無料）。バージョンチェックは Node.js 標準機能のみ（追加 npm 依存なし）。プライバシーポリシー・利用規約は素の HTML（Jekyll 等のビルドステップなし）。

**Spec:** GitHub issue #21（配布パッケージとリリース）と #22（Google OAuth 審査対応）。全体ロードマップ: `docs/superpowers/plans/2026-09-26-roadmap.md`（Phase 3: 一般配布・オンボーディング対応）。参照した既存実装: `crates/areitu-google/src/auth.rs`（`AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` を `option_env!` でビルド時に読む、要求スコープは現状 `drive.appdata` のみ）、`apps/desktop/src-tauri/tauri.conf.json`（`csp: null`）、`.github/workflows/ci.yml`。

## Global Constraints

- 独自サーバーを持たない。ユーザーのデータは常にユーザー自身の Google Drive の appDataFolder に留まる。この事実をプライバシーポリシーに正確に書く
- 今回のスコープではコード署名・公証を行わない（オーナー判断、2026-09-28）。ただし `release.yml` は Apple / Windows の証明書用シークレットを追加するだけで有効化できるよう、該当箇所をコメントアウトしたプレースホルダとして残す
- `ci.yml`（push / PR で動く通常 CI）は `AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` / `TAURI_SIGNING_PRIVATE_KEY` などの新しいシークレットに一切依存せず、フォークからの PR でも green を保つ
- Google Client ID / Secret はビルド時の環境変数から `option_env!` で読む既存の仕組み（`crates/areitu-google/src/auth.rs`）を変更しない。`release.yml` でのみ GitHub Secrets `AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET` として注入する
- シークレットの値をログに出力しない。ワークフロー中で `echo` や `run:` に直接値を書かない（`env:` 経由でのみ渡す）
- コミットメッセージは本文の後に空行を1つ置き、`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` で終える
- 依存クレート・npm パッケージのバージョンは実行時に `cargo add` / `npm install` で解決する（本プランではメジャーバージョンのみ固定し、パッチバージョンは固定しない）
- `apps/desktop/src-tauri/Cargo.toml` は現状 `edition = "2021"` であり、本プランではこれを変更しない（`areitu-core` などが `edition = "2024"` であるのとの不一致は既存の状態であり、本プランのスコープ外）

## Review Focus

1. バージョン番号が3ファイルで食い違ったままタグを push した場合 → `release.yml` の `check-versions` ジョブが最初に失敗し、食い違ったバイナリが Release に公開されない（Task 1, Task 4 でテスト）
2. アップデータ署名鍵（`TAURI_SIGNING_PRIVATE_KEY`）が未設定のままタグを push した場合 → `tauri-action` の署名ステップが明確なエラーで停止し、署名なしの「自動更新可能」と称するビルドが誤って公開されない（Task 3, Task 4 で確認）
3. CSP を `null` から具体的なポリシーに変えると、Tauri アプリではよくある失敗として画面が真っ白になる → CSP 変更後に必ずビルド・起動確認を行う手順を明記する（Task 2）
4. macOS の Gatekeeper（「壊れているため開けません」）や Windows の SmartScreen（「WindowsによってPCが保護されました」）を見て、未署名アプリの起動を諦めてしまうユーザー → README に具体的な回避手順（右クリック→開く、詳細情報→実行）を明記する（Task 5）
5. プライバシーポリシーが実装と食い違っている（例: 逆ジオコーディングや任意の LLM 送信の記載漏れ）と Google の審査で reject される → Task 6 で `areitu-core` の実際の外部通信先（Nominatim、任意設定時の OpenAI/Gemini）を正確に記述し、Task 7 のチェックリストで審査提出前に見直す項目として明記する

## File Structure

```
.github/workflows/release.yml                 タグ push (v*) で起動するリリースビルド・GitHub Release 下書き作成
.github/workflows/pages.yml                    docs/pages/ を GitHub Pages にデプロイ
apps/desktop/scripts/version-check.mjs         3ファイルからバージョンを抽出・比較する純粋関数（テスト対象）
apps/desktop/scripts/version-check.test.mjs    version-check.mjs の単体テスト
apps/desktop/scripts/check-versions.mjs        CLI 本体（fs から読み、不一致なら exit 1）
apps/desktop/package.json                      Modify: "check:versions" スクリプト、updater/process の npm 依存を追加
apps/desktop/src-tauri/tauri.conf.json         Modify: csp を具体的なポリシーに変更、plugins.updater を追加
apps/desktop/src-tauri/Cargo.toml              Modify: tauri-plugin-updater, tauri-plugin-process を追加
apps/desktop/src-tauri/src/lib.rs              Modify: updater/process プラグインを登録
apps/desktop/src-tauri/capabilities/default.json  Modify: updater:default, process:default 権限を追加
apps/desktop/src/api/updater.ts                checkForUpdate() / installUpdateAndRestart() のラッパー
apps/desktop/src/api/updater.test.ts           updater.ts の単体テスト（Tauri プラグインをモック）
apps/desktop/src/App.tsx                       Modify: 起動時に checkForUpdate() を呼び、更新可能ならバナー表示
README.md                                      Modify: 未署名アプリの開き方、バージョンアップ手順
docs/pages/index.html                          同意画面用ランディングページ（日英リンク）
docs/pages/privacy.ja.html                     プライバシーポリシー（日本語）
docs/pages/privacy.en.html                     Privacy Policy (English)
docs/pages/terms.ja.html                       利用規約（日本語）
docs/pages/terms.en.html                       Terms of Service (English)
docs/google-oauth-verification-checklist.md    同意画面設定とOAuth審査提出のチェックリスト（オーナー用）
```

---

### Task 1: バージョン整合性チェック

**Files:**
- Create: `apps/desktop/scripts/version-check.mjs`
- Create: `apps/desktop/scripts/version-check.test.mjs`
- Create: `apps/desktop/scripts/check-versions.mjs`
- Modify: `apps/desktop/package.json`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `extractPackageJsonVersion(jsonText: string): string`、`extractCargoTomlVersion(tomlText: string): string`、`extractTauriConfVersion(jsonText: string): string`、`checkVersionsMatch(input: { packageJson: string; cargoToml: string; tauriConf: string }): { ok: boolean; versions: Record<string, string> }` — いずれも `apps/desktop/scripts/version-check.mjs` からの named export。Task 4 の `release.yml` はこのタスクが追加する npm script `check:versions` を `check-versions` ジョブから呼ぶ

- [ ] **Step 1: 失敗するテストを書く**

`apps/desktop/scripts/version-check.test.mjs`:

```javascript
import { describe, expect, it } from "vitest";
import {
  checkVersionsMatch,
  extractCargoTomlVersion,
  extractPackageJsonVersion,
  extractTauriConfVersion,
} from "./version-check.mjs";

describe("extractPackageJsonVersion", () => {
  it("reads the version field", () => {
    expect(extractPackageJsonVersion('{"name":"desktop","version":"1.2.3"}')).toBe("1.2.3");
  });

  it("throws a clear error when version is missing", () => {
    expect(() => extractPackageJsonVersion('{"name":"desktop"}')).toThrow(
      /does not have a string "version" field/,
    );
  });
});

describe("extractCargoTomlVersion", () => {
  it("reads the version from the [package] section", () => {
    const toml = `[package]\nname = "areitu-desktop"\nversion = "1.2.3"\nedition = "2021"\n`;
    expect(extractCargoTomlVersion(toml)).toBe("1.2.3");
  });

  it("ignores version fields in other sections", () => {
    const toml = `[package]\nname = "areitu-desktop"\nversion = "1.2.3"\n\n[dependencies]\nserde = { version = "1" }\n`;
    expect(extractCargoTomlVersion(toml)).toBe("1.2.3");
  });

  it("throws a clear error when [package] has no version", () => {
    const toml = `[dependencies]\nserde = { version = "1" }\n`;
    expect(() => extractCargoTomlVersion(toml)).toThrow(/no \[package\] version field/);
  });
});

describe("extractTauriConfVersion", () => {
  it("reads the version field", () => {
    expect(extractTauriConfVersion('{"productName":"AREITU","version":"1.2.3"}')).toBe("1.2.3");
  });
});

describe("checkVersionsMatch", () => {
  it("reports ok when all three versions are equal", () => {
    const result = checkVersionsMatch({ packageJson: "1.2.3", cargoToml: "1.2.3", tauriConf: "1.2.3" });
    expect(result.ok).toBe(true);
  });

  it("reports not ok and lists the mismatched files when versions differ", () => {
    const result = checkVersionsMatch({ packageJson: "1.2.3", cargoToml: "1.2.4", tauriConf: "1.2.3" });
    expect(result.ok).toBe(false);
    expect(result.versions["apps/desktop/src-tauri/Cargo.toml"]).toBe("1.2.4");
  });
});
```

- [ ] **Step 2: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npx vitest run scripts/version-check.test.mjs`
Expected: FAIL — `Cannot find module './version-check.mjs'` またはそれに類するモジュール解決エラー

- [ ] **Step 3: 最小実装を書く**

`apps/desktop/scripts/version-check.mjs`:

```javascript
export function extractPackageJsonVersion(jsonText) {
  const data = JSON.parse(jsonText);
  if (typeof data.version !== "string") {
    throw new Error('package.json does not have a string "version" field');
  }
  return data.version;
}

export function extractCargoTomlVersion(tomlText) {
  const lines = tomlText.split("\n");
  let inPackageSection = false;
  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (line.startsWith("[")) {
      inPackageSection = line === "[package]";
      continue;
    }
    if (inPackageSection) {
      const match = line.match(/^version\s*=\s*"([^"]+)"/);
      if (match) {
        return match[1];
      }
    }
  }
  throw new Error("Cargo.toml has no [package] version field");
}

export function extractTauriConfVersion(jsonText) {
  const data = JSON.parse(jsonText);
  if (typeof data.version !== "string") {
    throw new Error('tauri.conf.json does not have a string "version" field');
  }
  return data.version;
}

export function checkVersionsMatch({ packageJson, cargoToml, tauriConf }) {
  const versions = {
    "apps/desktop/package.json": packageJson,
    "apps/desktop/src-tauri/Cargo.toml": cargoToml,
    "apps/desktop/src-tauri/tauri.conf.json": tauriConf,
  };
  const unique = new Set(Object.values(versions));
  return { ok: unique.size === 1, versions };
}
```

- [ ] **Step 4: テストを実行して成功を確認する**

Run: `cd apps/desktop && npx vitest run scripts/version-check.test.mjs`
Expected: PASS — 7 テストすべて成功

- [ ] **Step 5: CLI 本体を書く**

`apps/desktop/scripts/check-versions.mjs`:

```javascript
#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkVersionsMatch,
  extractCargoTomlVersion,
  extractPackageJsonVersion,
  extractTauriConfVersion,
} from "./version-check.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const desktopRoot = join(scriptDir, "..");

const packageJsonText = readFileSync(join(desktopRoot, "package.json"), "utf8");
const cargoTomlText = readFileSync(join(desktopRoot, "src-tauri", "Cargo.toml"), "utf8");
const tauriConfText = readFileSync(join(desktopRoot, "src-tauri", "tauri.conf.json"), "utf8");

const result = checkVersionsMatch({
  packageJson: extractPackageJsonVersion(packageJsonText),
  cargoToml: extractCargoTomlVersion(cargoTomlText),
  tauriConf: extractTauriConfVersion(tauriConfText),
});

if (!result.ok) {
  console.error(
    "Version mismatch across apps/desktop/package.json, apps/desktop/src-tauri/Cargo.toml, apps/desktop/src-tauri/tauri.conf.json:",
  );
  for (const [file, version] of Object.entries(result.versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

console.log(`Versions match: ${Object.values(result.versions)[0]}`);
```

- [ ] **Step 6: `package.json` に script を追加する**

`apps/desktop/package.json` の `"scripts"` に追加（既存の `"test"` の後）:

```json
    "test": "vitest run --passWithNoTests",
    "check:versions": "node scripts/check-versions.mjs"
```

- [ ] **Step 7: 現状のリポジトリで CLI を実行して成功することを確認する**

Run: `cd apps/desktop && npm run check:versions`
Expected: `Versions match: 0.1.0`（3ファイルとも現状 `0.1.0` のため）

- [ ] **Step 8: わざと不一致を作って CLI が exit 1 することを確認する**

Run: `cd apps/desktop && sed -i.bak 's/"version": "0.1.0"/"version": "0.1.1"/' package.json && npm run check:versions; echo "exit code: $?"; mv package.json.bak package.json`
Expected: 標準エラーに `Version mismatch across ...` と3ファイルのバージョンが出力され、`exit code: 1`。最後に `package.json` が元に戻っていることを確認する

- [ ] **Step 9: `ci.yml` の frontend ジョブにチェックを組み込む**

`.github/workflows/ci.yml` の `frontend` ジョブに1ステップ追加する（`npm run test` と `npm run build` の間、`npm ci` の直後）:

```yaml
      - working-directory: apps/desktop
        run: npm ci
      - working-directory: apps/desktop
        run: npm run check:versions
      - working-directory: apps/desktop
        run: npm run test
```

- [ ] **Step 10: `ci.yml` を Python で構文検証する**

Run: `python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('ci.yml: valid YAML')"`
Expected: `ci.yml: valid YAML`

- [ ] **Step 11: コミット**

```bash
git add apps/desktop/scripts/version-check.mjs apps/desktop/scripts/version-check.test.mjs apps/desktop/scripts/check-versions.mjs apps/desktop/package.json .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
feat(release): add version consistency check across package.json/Cargo.toml/tauri.conf.json

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: CSP 強化とビルド確認

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: なし（Task 1 と独立）
- Produces: `app.security.csp` に具体的なポリシー文字列。Task 3 がこの同じファイルに `plugins.updater` を追加するため、Task 3 の実装者はここで確定した JSON 構造（トップレベルの `app` / `bundle` キーの並び）を壊さないこと

- [ ] **Step 1: 変更前の状態でビルドできることを確認する（ベースライン）**

Run: `cd apps/desktop && npm run build`
Expected: PASS — `dist/` にファイルが生成され、エラーなく終了する（既存 CI で通っている手順の再確認）

- [ ] **Step 2: CSP を `null` から具体的なポリシーに変更する**

`apps/desktop/src-tauri/tauri.conf.json` の `app.security` を変更する:

```json
  "app": {
    "windows": [
      {
        "title": "AREITU",
        "width": 800,
        "height": 600
      }
    ],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: http://asset.localhost; connect-src 'self' ipc: http://ipc.localhost; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'"
    },
    "trayIcon": {
      "iconPath": "icons/icon.png"
    }
  },
```

補足（このステップで書いた理由）:
- `script-src 'self'`: フロントエンドはビルド済みの外部 JS ファイルのみを読み込み、インラインスクリプトを使わない（`apps/desktop/index.html` を確認済み）
- `style-src 'self' 'unsafe-inline'`: Tailwind の生成 CSS は外部ファイルだが、Vite の開発サーバーが HMR 用に `<style>` タグを動的挿入するため、`npm run tauri dev` を壊さないよう `'unsafe-inline'` を残す。React の `style={{...}}` 属性はコードベース中で未使用（`/usr/bin/grep -rn "style={{" apps/desktop/src` で確認済み、ゼロ件）
- `connect-src 'self' ipc: http://ipc.localhost`: このアプリはネットワーク呼び出しをすべて Rust 側（`areitu-core` / `areitu-google`）で行い、WebView から直接 HTTP は呼ばない。WebView が行うのは Tauri IPC のみ
- `img-src ... data: asset: http://asset.localhost`: 将来 Tauri の `asset:` プロトコルでローカル画像（写真サムネイル、Phase 4 予定）を表示する余地を残す。現時点では使っていないが、後で `img-src` だけ緩める手戻りを避ける

- [ ] **Step 3: フロントエンドビルドが CSP 変更後も壊れないことを確認する**

Run: `cd apps/desktop && npm run build`
Expected: PASS — Step 1 と同じ出力（CSP はランタイムの WebView 設定であり Vite のビルド結果自体には影響しないため、差分が出ないことを確認する）

- [ ] **Step 4: デバッグバンドルをビルドする**

Run: `cd apps/desktop && npm run tauri build -- --debug`
Expected: PASS。初回は Rust の依存クレートをビルドするため数分かかる。最後に生成されたバンドルのパス（macOS なら `src-tauri/target/debug/bundle/macos/AREITU.app` など）が出力される

- [ ] **Step 5: 起動確認（目視、手順を記録するだけで自動アサーションはしない）**

Run: `open apps/desktop/src-tauri/target/debug/bundle/macos/AREITU.app`（macOS の場合。Windows では `src-tauri\target\debug\bundle\nsis\` 配下の `.exe` を実行）
Expected: ウィンドウが開き、検索一覧画面が表示される（白紙のウィンドウにならない）。開発者ツールが必要な場合はデバッグビルドで右クリック→「要素を検証」が使えるので、コンソールに `Content-Security-Policy` 違反のエラーが出ていないことを確認する

**この Step 5 は目視確認であり、実行者（人間またはディスプレイにアクセスできるエージェント）がその場で判断する。CI では実行しない（CI はヘッドレスで GUI を検証できないため、Step 4 のビルド成功までを CI の保証範囲とする）。**

- [ ] **Step 6: コミット**

```bash
git add apps/desktop/src-tauri/tauri.conf.json
git commit -m "$(cat <<'EOF'
fix(desktop): replace csp: null with a restrictive Content-Security-Policy

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Tauri Updater プラグインの配線（minisign 署名）

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/package.json`
- Create: `apps/desktop/src/api/updater.ts`
- Create: `apps/desktop/src/api/updater.test.ts`
- Modify: `apps/desktop/src/App.tsx`

**Interfaces:**
- Consumes: Task 2 で確定した `tauri.conf.json` の `app` / `bundle` 構造（このタスクは同ファイルに `plugins` キーを追加するのみで、他のキーは変更しない）
- Produces: `checkForUpdate(): Promise<UpdateCheckResult>` と `installUpdateAndRestart(update: Update): Promise<void>`（`apps/desktop/src/api/updater.ts` からの named export）。`UpdateCheckResult` は `{ available: false } | { available: true; update: Update; version: string; notes: string | null }`。`Update` は `@tauri-apps/plugin-updater` がエクスポートする型をそのまま再エクスポートする。Task 4 の `release.yml` はこのタスクでオーナーが生成する GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` を消費する

- [ ] **Step 1 (OWNER ACTION): minisign 署名鍵ペアを生成する**

これは秘密鍵を生成する操作であり、オーナー（リポジトリ管理者）本人のマシンで実行すること。エージェントはこのステップを代行しない。

```bash
cd apps/desktop
npx tauri signer generate -w ~/.tauri/areitu-updater.key
```

- プロンプトでパスワードを設定する（空パスワードも技術的には可能だが、秘密鍵ファイル単体が漏れた場合のリスクがあるため、パスワードを設定することを強く推奨する）
- コマンドは公開鍵を標準出力に印字する（`untrusted comment: ...` の次の行、`base64` 文字列）。この公開鍵は Step 4 で `tauri.conf.json` に書く（秘密ではないのでコミットしてよい）
- 秘密鍵ファイルは `~/.tauri/areitu-updater.key`（テキストファイル）に保存される。**このファイルの中身を絶対にコミットしない。絶対にログや chat に貼り付けない**
- 生成した秘密鍵ファイルの中身全体と、設定したパスワードを、次の2つの GitHub Secrets として登録する（GitHub の Web UI: リポジトリの Settings → Secrets and variables → Actions → New repository secret）:
  - `TAURI_SIGNING_PRIVATE_KEY`: `~/.tauri/areitu-updater.key` の中身全体（`cat ~/.tauri/areitu-updater.key` の出力をそのまま貼り付ける）
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: Step 1 で設定したパスワード
- 公開鍵は秘密鍵と対になる1つだけを使い続ける（毎回生成し直すと、過去のビルドを使っているユーザーが新しいビルドを「署名検証エラー」で受け取れなくなる）

- [ ] **Step 2: Rust 依存を追加する**

Run: `cd apps/desktop/src-tauri && cargo add tauri-plugin-updater@2 && cargo add tauri-plugin-process@2`
Expected: `Cargo.toml` の `[dependencies]` に `tauri-plugin-updater` と `tauri-plugin-process` が追加される

- [ ] **Step 3: プラグインを登録する**

`apps/desktop/src-tauri/src/lib.rs` の `tauri::Builder::default()` チェーンに追加する（`tauri_plugin_autostart::init(...)` の後）:

```rust
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```

- [ ] **Step 4: `tauri.conf.json` に updater 設定を追加する**

`apps/desktop/src-tauri/tauri.conf.json` のトップレベルに `plugins` キーを追加する（`bundle` の後）。`pubkey` は Step 1 でオーナーが生成した公開鍵に置き換える:

```json
  "plugins": {
    "updater": {
      "pubkey": "REPLACE_WITH_OWNER_GENERATED_MINISIGN_PUBLIC_KEY_FROM_STEP_1",
      "endpoints": [
        "https://github.com/ikeikeikeda66/AREITU/releases/latest/download/latest.json"
      ]
    }
  }
```

**注意:** `pubkey` がプレースホルダ文字列のままだと、実行時に `check()` を呼んだ際は Base64 デコードに失敗して `Err` を返す（アプリはクラッシュしない — `updater.ts` の `checkForUpdate()` は `try/catch` で包む。Step 8 で確認する）。`npm run tauri build` 自体はプレースホルダ文字列のままでも成功する（ビルド時に鍵の妥当性を検証しないため）。Step 1 の公開鍵に置き換えるのはオーナーの作業であり、これが完了するまで実際のアップデート配信は機能しない

- [ ] **Step 5: capabilities に権限を追加する**

`apps/desktop/src-tauri/capabilities/default.json` の `"permissions"` 配列に追加する:

```json
  "permissions": [
    "core:default",
    "opener:default",
    "updater:default",
    "process:default"
  ]
```

- [ ] **Step 6: npm 依存を追加する**

Run: `cd apps/desktop && npm install @tauri-apps/plugin-updater@2 @tauri-apps/plugin-process@2`
Expected: `package.json` の `"dependencies"` に両パッケージが追加される

- [ ] **Step 7: 失敗するテストを書く**

`apps/desktop/src/api/updater.test.ts`:

```typescript
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}));

describe("checkForUpdate", () => {
  it("returns available: false when no update is found", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    vi.mocked(check).mockResolvedValue(null);
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: false });
  });

  it("returns available: true with version and notes when an update is found", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    const fakeUpdate = { version: "1.2.3", body: "バグ修正", downloadAndInstall: vi.fn() };
    // @ts-expect-error テスト用の最小フェイク（Update 型の全フィールドは持たない）
    vi.mocked(check).mockResolvedValue(fakeUpdate);
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: true, update: fakeUpdate, version: "1.2.3", notes: "バグ修正" });
  });

  it("returns available: false when check() throws (e.g. no network, invalid pubkey)", async () => {
    const { check } = await import("@tauri-apps/plugin-updater");
    vi.mocked(check).mockRejectedValue(new Error("boom"));
    const { checkForUpdate } = await import("./updater");

    const result = await checkForUpdate();

    expect(result).toEqual({ available: false });
  });
});

describe("installUpdateAndRestart", () => {
  it("downloads, installs, and relaunches", async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    const download = vi.fn().mockResolvedValue(undefined);
    // @ts-expect-error テスト用の最小フェイク
    const fakeUpdate = { downloadAndInstall: download };
    const { installUpdateAndRestart } = await import("./updater");

    await installUpdateAndRestart(fakeUpdate);

    expect(download).toHaveBeenCalledTimes(1);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 8: テストを実行して失敗を確認する**

Run: `cd apps/desktop && npx vitest run src/api/updater.test.ts`
Expected: FAIL — `Cannot find module './updater'` またはそれに類するモジュール解決エラー

- [ ] **Step 9: 実装を書く**

`apps/desktop/src/api/updater.ts`:

```typescript
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdateCheckResult =
  | { available: false }
  | { available: true; update: Update; version: string; notes: string | null };

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  try {
    const update = await check();
    if (update === null) {
      return { available: false };
    }
    return { available: true, update, version: update.version, notes: update.body ?? null };
  } catch {
    // ネットワークがない、pubkey が未設定/不正、署名検証失敗など。
    // 更新チェックの失敗でアプリの起動自体を妨げないよう、常に available: false として扱う。
    return { available: false };
  }
}

export async function installUpdateAndRestart(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
```

- [ ] **Step 10: テストを実行して成功を確認する**

Run: `cd apps/desktop && npx vitest run src/api/updater.test.ts`
Expected: PASS — 4 テストすべて成功

- [ ] **Step 11: `App.tsx` に起動時チェックとバナーを組み込む**

`apps/desktop/src/App.tsx` を変更する:

```typescript
import { useEffect, useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { checkForUpdate, installUpdateAndRestart, type UpdateCheckResult } from "./api/updater";
import type { Place } from "./api/types";

type View = { kind: "list" } | { kind: "detail"; place: Place } | { kind: "settings" };

export default function App() {
  const [view, setView] = useState<View>({ kind: "list" });
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult>({ available: false });
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    checkForUpdate().then(setUpdateInfo);
  }, []);

  async function handleInstallUpdate() {
    if (!updateInfo.available) return;
    setInstalling(true);
    await installUpdateAndRestart(updateInfo.update);
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
      {updateInfo.available && (
        <div className="flex items-center justify-between bg-amber-100 p-2 text-sm text-amber-900">
          <span>新しいバージョン {updateInfo.version} が利用可能です</span>
          <button
            type="button"
            onClick={handleInstallUpdate}
            disabled={installing}
            className="rounded-md border border-amber-400 px-3 py-1 hover:bg-amber-200 disabled:opacity-50"
          >
            {installing ? "更新中..." : "更新して再起動"}
          </button>
        </div>
      )}
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

- [ ] **Step 12: 既存のフロントエンドテストを含めて全体を実行する**

Run: `cd apps/desktop && npm run test`
Expected: PASS — 既存の `SearchListScreen.test.tsx` / `PlaceDetailScreen.test.tsx` を含め全テストが成功する（`App.tsx` の変更は `checkForUpdate` を呼ぶが、`useEffect` 内なので既存テストのレンダリング結果には影響しない）

- [ ] **Step 13: `cargo test` と `clippy` を実行する**

Run: `cd apps/desktop/src-tauri && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 14: プレースホルダ pubkey のままでもビルドが壊れないことを確認する**

Run: `cd apps/desktop && npm run tauri build -- --debug`
Expected: PASS。Step 4 のプレースホルダ `pubkey` はビルド自体を止めない（実行時の署名検証でのみ問題になる）ことを確認する

- [ ] **Step 15: コミット**

```bash
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/capabilities/default.json apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json apps/desktop/package-lock.json apps/desktop/src/api/updater.ts apps/desktop/src/api/updater.test.ts apps/desktop/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): wire up Tauri updater plugin with minisign signing

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: リリースワークフロー (`release.yml`)

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 1 の `npm run check:versions`、Task 3 で追加された `plugins.updater` 設定とオーナーが登録する `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`、および `crates/areitu-google/src/auth.rs` が読む `AREITU_GOOGLE_CLIENT_ID` / `AREITU_GOOGLE_CLIENT_SECRET`
- Produces: タグ `vX.Y.Z` を push すると GitHub Release の下書きに macOS (universal `.app`/`.dmg`) と Windows (`.msi`/`.exe`) のインストーラ、および `latest.json`（updater 用）が添付される

- [ ] **Step 1 (OWNER ACTION): GitHub Secrets を登録する（release.yml が参照するもの）**

リポジトリの Settings → Secrets and variables → Actions → New repository secret で、以下をすべて登録する（値は絶対に issue/PR/コミットメッセージに書かない）:

- `AREITU_GOOGLE_CLIENT_ID`: Google Cloud Console で発行した OAuth クライアント ID
- `AREITU_GOOGLE_CLIENT_SECRET`: 同クライアントシークレット
- `TAURI_SIGNING_PRIVATE_KEY`: Task 3 Step 1 で生成した minisign 秘密鍵ファイルの中身全体
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: 同秘密鍵のパスワード

これらのシークレットは `release.yml` だけが参照し、通常の `ci.yml`（push / PR）は参照しない。したがって未登録でも `ci.yml` は green のまま。`release.yml` はタグ push でのみ動くため、これらのシークレットが揃うまでは意図的にタグを push しない

- [ ] **Step 2: ワークフローファイルを書く**

`.github/workflows/release.yml`:

```yaml
name: release

on:
  push:
    tags:
      - "v*"

permissions:
  contents: write

jobs:
  check-versions:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - working-directory: apps/desktop
        run: npm ci
      - working-directory: apps/desktop
        run: npm run check:versions

  release:
    needs: check-versions
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: macos-latest
            args: --target universal-apple-darwin
          - platform: windows-latest
            args: ""
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - uses: dtolnay/rust-toolchain@stable
        if: matrix.platform == 'macos-latest'
        with:
          targets: aarch64-apple-darwin,x86_64-apple-darwin
      - uses: dtolnay/rust-toolchain@stable
        if: matrix.platform == 'windows-latest'
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          AREITU_GOOGLE_CLIENT_ID: ${{ secrets.AREITU_GOOGLE_CLIENT_ID }}
          AREITU_GOOGLE_CLIENT_SECRET: ${{ secrets.AREITU_GOOGLE_CLIENT_SECRET }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          # --- macOS code signing / notarization: NOT configured yet (owner decision, 2026-09-28). ---
          # To enable later: add these repository secrets, then uncomment the four lines below.
          # No other change to this workflow is needed.
          # APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          # APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          # APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          # APPLE_ID: ${{ secrets.APPLE_ID }}
          # APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          # APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          # --- Windows code signing: NOT configured yet. Add these secrets later and uncomment. ---
          # WINDOWS_CERTIFICATE: ${{ secrets.WINDOWS_CERTIFICATE }}
          # WINDOWS_CERTIFICATE_PASSWORD: ${{ secrets.WINDOWS_CERTIFICATE_PASSWORD }}
        with:
          projectPath: apps/desktop
          tagName: ${{ github.ref_name }}
          releaseName: "AREITU ${{ github.ref_name }}"
          releaseBody: |
            未署名ビルドです。macOS / Windows での開き方は README を参照してください。
            This is an unsigned build. See the README for how to open it on macOS / Windows.
          releaseDraft: true
          prerelease: false
          includeUpdaterJson: true
          args: ${{ matrix.args }}
```

- [ ] **Step 3: YAML の構文を検証する**

Run: `python3 -c "import yaml; d = yaml.safe_load(open('.github/workflows/release.yml')); print('valid YAML, jobs:', list(d['jobs'].keys()))"`
Expected: `valid YAML, jobs: ['check-versions', 'release']`

- [ ] **Step 4: `check-versions` ジョブと同等の手順をローカルで再現する**

Run: `cd apps/desktop && npm ci && npm run check:versions`
Expected: `Versions match: 0.1.0`（`release.yml` の `check-versions` ジョブが実際に行うのと同じコマンド）

- [ ] **Step 5: このステップで実行できない検証を記録する**

以下は本タスクの中では実行しない。タグを実際に push してワークフローを走らせないと確認できないため、初回リリース（最初の `vX.Y.Z` タグ push）でオーナーが確認する:
- `tauri-apps/tauri-action` が macOS / Windows の実行環境で実際にバンドルを生成できること
- `TAURI_SIGNING_PRIVATE_KEY` が正しく設定されている場合に署名済み `latest.json` が生成されること（未設定・不正な場合はジョブが失敗することも含む）
- GitHub Release の下書きに4種類前後の成果物（macOS `.app`/`.dmg`、Windows `.msi`/`.exe`、`latest.json` と署名ファイル）が実際に添付されること

- [ ] **Step 6: コミット**

```bash
git add .github/workflows/release.yml
git commit -m "$(cat <<'EOF'
feat(ci): add tag-triggered release workflow for macOS and Windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: README 更新（未署名アプリの開き方・バージョンアップ手順）

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: Task 1 の `npm run check:versions`、Task 4 の `release.yml`（タグ push で起動）

- [ ] **Step 1: README に「配布パッケージ」セクションを追加する**

`README.md` の末尾（デスクトップアプリのセクションの後）に追加する:

```markdown

## 配布パッケージ（未署名ビルド）

GitHub Releases の各リリースには macOS 用インストーラ（`.dmg` / `.app`）と Windows 用インストーラ（`.msi` / `.exe`、NSIS）が添付されます。現時点ではコード署名・公証を行っていない未署名ビルドのため、OS の標準的な警告が表示されます。以下の手順で開けます。

### macOS（Gatekeeper）

ダウンロードした `.dmg` を開き、`AREITU.app` を `Applications` フォルダにコピーした後:

1. `Applications` フォルダで `AREITU.app` を **右クリック（または Control キーを押しながらクリック）** し、「開く」を選択する
2. 「開発元が未確認のため開けません」という警告が出るので、もう一度「開く」を押す
3. 2回目以降は通常のダブルクリックで起動できる

ターミナルから直接許可することもできます:

```bash
xattr -dr com.apple.quarantine /Applications/AREITU.app
```

### Windows（SmartScreen）

インストーラ（`.msi` または `.exe`）を実行すると「WindowsによってPCが保護されました」と表示されることがあります:

1. 「詳細情報」をクリックする
2. 表示される「実行」ボタンをクリックする

### 今後の予定

コード署名・公証（Apple Developer Program / Windows コード署名証明書）は現時点では導入していません。導入した場合はこれらの警告は表示されなくなります。

## バージョンを上げる手順

バージョン番号は3箇所（`apps/desktop/package.json`、`apps/desktop/src-tauri/Cargo.toml`、`apps/desktop/src-tauri/tauri.conf.json`）に重複して存在し、常に一致していなければなりません。

1. 3ファイルすべての `version` フィールドを同じ値に変更する
2. `cd apps/desktop && npm run check:versions` を実行し、`Versions match: X.Y.Z` と表示されることを確認する
3. 変更をコミットする
4. `git tag vX.Y.Z && git push origin vX.Y.Z` でタグを push する（`v` プレフィックス必須。`.github/workflows/release.yml` は `v*` にマッチするタグでのみ起動する）
5. GitHub Actions の `release` ワークフローが完了すると、GitHub Releases に**下書き**として新しいリリースが作成される。内容を確認してから手動で公開（Publish release）する
```

- [ ] **Step 2: README 内の相対コマンドが実際に動くことを確認する**

Run: `cd apps/desktop && npm run check:versions`
Expected: `Versions match: 0.1.0`（README の手順2で案内しているコマンドと出力が一致することを確認する）

- [ ] **Step 3: コミット**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs: document unsigned-build install steps and version bump procedure

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: プライバシーポリシー・利用規約ページ（日英）

**Files:**
- Create: `docs/pages/index.html`
- Create: `docs/pages/privacy.ja.html`
- Create: `docs/pages/privacy.en.html`
- Create: `docs/pages/terms.ja.html`
- Create: `docs/pages/terms.en.html`

**Interfaces:**
- Produces: 静的 HTML ファイル一式。Task 7 の `pages.yml` はこのディレクトリ（`docs/pages/`）をそのまま GitHub Pages の公開ルートとしてアップロードする。ファイル名・相対リンクは Task 7 で変更しない

- [ ] **Step 1: ランディングページを書く**

`docs/pages/index.html`:

```html
<!doctype html>
<html lang="ja">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>AREITU</title>
</head>
<body>
  <h1>AREITU</h1>
  <p>
    AREITU はプライバシー重視の訪問履歴デスクトップアプリです。データはユーザー自身の端末と、ユーザー自身の Google
    Drive アカウント内にのみ保存されます。独自のサーバーは持ちません。
  </p>
  <p>
    AREITU is a privacy-first personal visit-log desktop app. Your data stays on your own machine and in your own
    Google Drive account. We do not run our own server.
  </p>
  <ul>
    <li><a href="./privacy.ja.html">プライバシーポリシー（日本語）</a> / <a href="./privacy.en.html">Privacy Policy (English)</a></li>
    <li><a href="./terms.ja.html">利用規約（日本語）</a> / <a href="./terms.en.html">Terms of Service (English)</a></li>
    <li><a href="https://github.com/ikeikeikeda66/AREITU">GitHub リポジトリ / GitHub Repository</a></li>
  </ul>
</body>
</html>
```

- [ ] **Step 2: プライバシーポリシー（日本語）を書く**

`docs/pages/privacy.ja.html`:

```html
<!doctype html>
<html lang="ja">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>プライバシーポリシー - AREITU</title>
</head>
<body>
  <h1>プライバシーポリシー</h1>
  <p>最終更新日: 2026年9月28日</p>

  <h2>基本方針</h2>
  <p>
    AREITU は独自のサーバーを持ちません。あなたが記録した訪問履歴データ（<code>areitu.db</code>、SQLite ファイル）は、
    あなたの端末のローカルディスクにのみ保存されます。Google アカウントでログインし同期を有効にした場合のみ、この
    ファイルはあなた自身の Google アカウントの Google Drive にある「アプリデータフォルダ（appDataFolder）」にバック
    アップ・同期されます。このフォルダは通常の Google Drive の画面には表示されず、AREITU 以外のアプリからはアクセス
    できません。開発者（AREITU の作者）は、あなたのデータにアクセスする手段を持ちません。
  </p>

  <h2>リクエストする Google OAuth スコープ</h2>
  <ul>
    <li>
      <code>https://www.googleapis.com/auth/drive.appdata</code>: <code>areitu.db</code> をあなたの Google Drive の
      アプリデータフォルダにアップロード・ダウンロードするためだけに使います。他のファイルへはアクセスしません。
    </li>
    <li>
      <code>https://www.googleapis.com/auth/calendar.readonly</code>: あなたの Google カレンダーの予定を読み取り、
      訪問履歴とのつき合わせ（同じ時間帯に予定があったかどうかの推定）に使います。カレンダーの内容を書き換えたり、
      外部に送信したりすることはありません。
    </li>
  </ul>

  <h2>データがあなたの端末・Google アカウントの外に出るケース</h2>
  <p>
    以下の2つの場合を除き、データが外部に送信されることはありません。
  </p>
  <ul>
    <li>
      <strong>逆ジオコーディング</strong>: 写真の GPS 座標から場所の候補名を推定するために、緯度・経度のみを
      OpenStreetMap の Nominatim（またはあなたが設定した場合は Google Places API）に送信します。氏名・メール
      アドレスなど、あなたを特定する情報は送信しません。
    </li>
    <li>
      <strong>LLM フォールバック（任意設定時のみ）</strong>: 設定画面で OpenAI または Gemini の API キーをあなた自身
      で入力し有効化した場合に限り、場所名の推定が難しいケースについて、位置情報や周辺の候補名などの断片的な情報を
      その LLM プロバイダに送信します。デフォルト（未設定）ではこの送信は一切行われません。ローカルの Ollama を
      設定した場合は、この送信すら発生しません（同一端末内で完結します）。
    </li>
  </ul>

  <h2>データの保存場所</h2>
  <ul>
    <li>訪問履歴データベース: OS のアプリデータディレクトリ配下の <code>areitu.db</code>（ローカル）</li>
    <li>設定情報: 同 config ディレクトリの <code>config.json</code>（ローカル、API キーなどの秘密情報は含まない）</li>
    <li>API キー・Google の認証情報: OS のキーチェーン（macOS Keychain / Windows Credential Manager）にのみ保存</li>
    <li>Google Drive 同期を有効にした場合のバックアップ: あなたの Google アカウントの appDataFolder</li>
  </ul>

  <h2>データの削除方法</h2>
  <ol>
    <li>
      AREITU アプリ内の設定画面からサインアウトすると、ローカルに保存された Google の認証情報（リフレッシュ
      トークン）が削除されます。
    </li>
    <li>
      <a href="https://myaccount.google.com/permissions">Google アカウントのアクセス権限ページ</a>から
      「AREITU」へのアクセスを取り消すと、Google 側に保存されたトークンが無効化されます。
    </li>
    <li>
      アプリデータフォルダに保存されたバックアップファイルは、上記のアクセス取り消しと合わせて Google 側で
      削除されます（appDataFolder はユーザーが直接ファイルブラウザで削除することはできず、アクセス権を取り消す
      ことで解決されます）。
    </li>
    <li>
      ローカルのデータベースファイル（<code>areitu.db</code>）と設定ファイル（<code>config.json</code>）を削除
      するには、AREITU をアンインストールした上で、OS のアプリデータ/コンフィグディレクトリから該当ファイルを
      手動で削除してください。
    </li>
  </ol>

  <h2>お問い合わせ</h2>
  <p>
    このプライバシーポリシーに関するお問い合わせは
    <a href="https://github.com/ikeikeikeda66/AREITU/issues">GitHub リポジトリの Issue</a>までお願いします。
  </p>
</body>
</html>
```

- [ ] **Step 3: プライバシーポリシー（英語）を書く**

`docs/pages/privacy.en.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Privacy Policy - AREITU</title>
</head>
<body>
  <h1>Privacy Policy</h1>
  <p>Last updated: September 28, 2026</p>

  <h2>Overview</h2>
  <p>
    AREITU does not run its own server. Your visit-log data (<code>areitu.db</code>, a SQLite file) is stored only on
    your device's local disk. If you sign in with your Google account and enable sync, this file is backed up to and
    synced with the "appDataFolder" in your own Google Drive account. This folder is not visible in the normal Google
    Drive UI and cannot be accessed by any app other than AREITU. The developer (AREITU's author) has no means of
    accessing your data.
  </p>

  <h2>Google OAuth scopes requested</h2>
  <ul>
    <li>
      <code>https://www.googleapis.com/auth/drive.appdata</code>: used only to upload and download
      <code>areitu.db</code> to and from the appDataFolder in your own Google Drive. No other files are accessed.
    </li>
    <li>
      <code>https://www.googleapis.com/auth/calendar.readonly</code>: used to read events from your Google Calendar
      and match them against your visit log (to estimate whether a calendar event coincided with a visit). We never
      modify your calendar or send its contents anywhere else.
    </li>
  </ul>

  <h2>When data leaves your device or Google account</h2>
  <p>Other than the two cases below, no data is ever sent externally.</p>
  <ul>
    <li>
      <strong>Reverse geocoding</strong>: to guess a place name from a photo's GPS coordinates, we send only the
      latitude/longitude to OpenStreetMap's Nominatim service (or, if you configure it, the Google Places API). We
      never send anything that identifies you personally, such as your name or email address.
    </li>
    <li>
      <strong>Optional LLM fallback</strong>: only if you enter and enable your own OpenAI or Gemini API key in
      Settings, difficult-to-resolve place names are sent, along with fragments such as location and nearby
      candidates, to that LLM provider. By default (unconfigured) this never happens. If you configure a local
      Ollama instance instead, this step never leaves your machine at all.
    </li>
  </ul>

  <h2>Where data is stored</h2>
  <ul>
    <li>Visit log database: <code>areitu.db</code> under the OS application-data directory (local)</li>
    <li>Settings: <code>config.json</code> under the same config directory (local, contains no secrets)</li>
    <li>API keys and Google credentials: stored only in the OS keychain (macOS Keychain / Windows Credential Manager)</li>
    <li>Backup when Google Drive sync is enabled: the appDataFolder of your own Google account</li>
  </ul>

  <h2>How to delete your data</h2>
  <ol>
    <li>Signing out from AREITU's Settings screen deletes the locally stored Google credential (refresh token).</li>
    <li>
      Revoking AREITU's access from your
      <a href="https://myaccount.google.com/permissions">Google Account permissions page</a> invalidates the token
      stored on Google's side.
    </li>
    <li>
      The backup file in your appDataFolder is removed as part of revoking access above (the appDataFolder cannot be
      browsed or deleted directly by the user; revoking access is how it is cleaned up).
    </li>
    <li>
      To delete the local database (<code>areitu.db</code>) and settings (<code>config.json</code>), uninstall
      AREITU and manually remove those files from the OS application-data/config directory.
    </li>
  </ol>

  <h2>Contact</h2>
  <p>
    For questions about this Privacy Policy, please use the
    <a href="https://github.com/ikeikeikeda66/AREITU/issues">Issues page of the GitHub repository</a>.
  </p>
</body>
</html>
```

- [ ] **Step 4: 利用規約（日本語）を書く**

`docs/pages/terms.ja.html`:

```html
<!doctype html>
<html lang="ja">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>利用規約 - AREITU</title>
</head>
<body>
  <h1>利用規約</h1>
  <p>最終更新日: 2026年9月28日</p>

  <h2>ライセンス</h2>
  <p>
    AREITU は MIT ライセンスで公開されているオープンソースソフトウェアです。ソースコードは
    <a href="https://github.com/ikeikeikeda66/AREITU">GitHub リポジトリ</a>で公開されています。ライセンス全文は
    リポジトリの <code>LICENSE</code> ファイルを参照してください。
  </p>

  <h2>無保証</h2>
  <p>
    AREITU は「現状のまま」提供され、明示または黙示を問わずいかなる保証もありません。開発者は、本ソフトウェアの
    使用によって生じたいかなる損害についても責任を負いません（詳細は MIT ライセンス本文を参照）。
  </p>

  <h2>Google アカウントとの連携について</h2>
  <p>
    本アプリで Google アカウントにログインし機能を利用する場合、
    <a href="https://policies.google.com/terms">Google の利用規約</a>および
    <a href="https://developers.google.com/terms/api-services-user-data-policy">Google API サービスのユーザーデータ
    ポリシー</a>（Limited Use の要件を含む）にも従います。本アプリが取得する Google データの取り扱いについては
    <a href="./privacy.ja.html">プライバシーポリシー</a>を参照してください。
  </p>

  <h2>サポート範囲</h2>
  <p>
    本アプリはボランティアベースで開発されているオープンソースプロジェクトであり、商用サポートは提供していません。
    不具合の報告や機能要望は
    <a href="https://github.com/ikeikeikeda66/AREITU/issues">GitHub の Issue</a>で受け付けています。
  </p>
</body>
</html>
```

- [ ] **Step 5: 利用規約（英語）を書く**

`docs/pages/terms.en.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Terms of Service - AREITU</title>
</head>
<body>
  <h1>Terms of Service</h1>
  <p>Last updated: September 28, 2026</p>

  <h2>License</h2>
  <p>
    AREITU is open-source software released under the MIT License. Source code is available on the
    <a href="https://github.com/ikeikeikeda66/AREITU">GitHub repository</a>. See the <code>LICENSE</code> file in the
    repository for the full license text.
  </p>

  <h2>No warranty</h2>
  <p>
    AREITU is provided "as is", without warranty of any kind, express or implied. The developer is not liable for
    any damages arising from the use of this software (see the full MIT License text for details).
  </p>

  <h2>Use of your Google Account</h2>
  <p>
    If you sign in with your Google account to use certain features, you also agree to
    <a href="https://policies.google.com/terms">Google's Terms of Service</a> and the
    <a href="https://developers.google.com/terms/api-services-user-data-policy">Google API Services User Data
    Policy</a> (including its Limited Use requirements). See the <a href="./privacy.en.html">Privacy Policy</a> for
    how this app handles Google data.
  </p>

  <h2>Support</h2>
  <p>
    This is a volunteer-run open-source project and does not offer commercial support. Bug reports and feature
    requests are welcome via <a href="https://github.com/ikeikeikeda66/AREITU/issues">GitHub Issues</a>.
  </p>
</body>
</html>
```

- [ ] **Step 6: 実装したスコープの記載がコードと一致していることを検証する**

Run: `/usr/bin/grep -o 'https://www.googleapis.com/auth/drive.appdata' docs/pages/privacy.ja.html docs/pages/privacy.en.html crates/areitu-google/src/lib.rs`
Expected: 3ファイルすべてで1件ずつヒットする（プライバシーポリシー2言語と実装のスコープ定数が一致していることの確認）

- [ ] **Step 7: HTML が5ファイルとも妥当な構造であることを軽く検証する**

Run: `for f in docs/pages/*.html; do python3 -c "import sys; from html.parser import HTMLParser; HTMLParser().feed(open(sys.argv[1]).read()); print(sys.argv[1], 'parses OK')" "$f"; done`
Expected: 5行すべて `... parses OK` と出力される（パースエラーで例外が出ないこと）

- [ ] **Step 8: コミット**

```bash
git add docs/pages/index.html docs/pages/privacy.ja.html docs/pages/privacy.en.html docs/pages/terms.ja.html docs/pages/terms.en.html
git commit -m "$(cat <<'EOF'
docs: add bilingual privacy policy and terms of service pages

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: GitHub Pages デプロイと OAuth 審査チェックリスト

**Files:**
- Create: `.github/workflows/pages.yml`
- Create: `docs/google-oauth-verification-checklist.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: Task 6 で作成した `docs/pages/` 配下のファイル一式
- Produces: `main` ブランチへの `docs/pages/**` の変更が push されるたびに、GitHub Pages に自動デプロイされる公開 URL（`https://ikeikeikeda66.github.io/AREITU/`、オーナーが Pages を有効化した後）

- [ ] **Step 1: Pages デプロイワークフローを書く**

`.github/workflows/pages.yml`:

```yaml
name: pages

on:
  push:
    branches: [main]
    paths:
      - "docs/pages/**"
      - ".github/workflows/pages.yml"
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  deploy:
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
        with:
          path: docs/pages
      - id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Step 2: YAML の構文を検証する**

Run: `python3 -c "import yaml; d = yaml.safe_load(open('.github/workflows/pages.yml')); print('valid YAML, job:', list(d['jobs'].keys())[0])"`
Expected: `valid YAML, job: deploy`

- [ ] **Step 3 (OWNER ACTION): リポジトリ設定で GitHub Pages を有効化する**

このワークフローは Pages 機能自体が有効化されていないとデプロイに失敗する。以下はオーナーがリポジトリの Web UI で一度だけ行う設定であり、シークレットは不要:

1. GitHub の `ikeikeikeda66/AREITU` リポジトリで Settings → Pages を開く
2. 「Build and deployment」の「Source」を **GitHub Actions** に設定する（「Deploy from a branch」ではない）
3. 保存後、`main` ブランチに `docs/pages/**` の変更が push されると（このタスクのコミット自体がそれに当たる）、`pages.yml` が自動実行され、数分後に `https://ikeikeikeda66.github.io/AREITU/` で公開される
4. 公開後、実際にブラウザでアクセスして `index.html` からプライバシーポリシー・利用規約の日英4ページへのリンクがすべて開けることを確認する（この確認はオーナーが Pages 有効化後に行う。ワークフローの YAML 検証や artifact のアップロード自体は Step 2 と `.github/workflows/pages.yml` の構造で保証されているが、実際に公開 URL が生きているかは Pages 有効化後でないと確認できない）

- [ ] **Step 4: OAuth 審査チェックリストを書く**

`docs/google-oauth-verification-checklist.md`:

```markdown
# Google OAuth 審査対応チェックリスト

このチェックリストはすべてオーナー（Google Cloud Console の管理者）が行う作業であり、エージェントが代行しない。

## 前提

- プライバシーポリシー: `https://ikeikeikeda66.github.io/AREITU/privacy.ja.html`（日本語）/
  `https://ikeikeikeda66.github.io/AREITU/privacy.en.html`（英語） — Task 7 Step 3 で GitHub Pages を有効化した後に
  公開される
- 利用規約: `https://ikeikeikeda66.github.io/AREITU/terms.ja.html` / `terms.en.html`
- ホームページ / アプリのドメイン: `https://ikeikeikeda66.github.io/AREITU/`
- リクエストするスコープ: `drive.appdata`（非機微〜機微スコープ）、`calendar.readonly`（機微スコープ）。
  実際にリクエストされるスコープの一覧は `crates/areitu-google/src/lib.rs` を参照。審査時点での正式な分類
  （sensitive / restricted）は Google Cloud Console の OAuth 同意画面編集画面が都度表示するので、そこで確認する

## OAuth 同意画面の設定（Google Cloud Console）

- [ ] 「External」ユーザータイプで OAuth 同意画面を作成する（個人開発者が配布する OSS アプリのため）
- [ ] アプリ名を `AREITU` に設定する
- [ ] ユーザーサポートメール、デベロッパーの連絡先メールを設定する
- [ ] アプリのホームページ URL に `https://ikeikeikeda66.github.io/AREITU/` を設定する
- [ ] プライバシーポリシー URL に `https://ikeikeikeda66.github.io/AREITU/privacy.en.html`（英語版を優先して登録。
  日本語版は同ページ内のリンクから辿れる）を設定する
- [ ] 利用規約 URL に `https://ikeikeikeda66.github.io/AREITU/terms.en.html` を設定する
- [ ] 承認済みドメインに `github.io` を追加する
- [ ] スコープを追加する: `.../auth/drive.appdata`、`.../auth/calendar.readonly`
- [ ] 各スコープについて「このスコープが必要な理由」の説明文を記入する（下記の下書きを利用してよい）:
  - `drive.appdata`: "AREITU stores its local SQLite database as a backup in the user's own Google Drive
    appDataFolder, which is invisible in the normal Drive UI and accessible only to this app. This is the only way
    the app syncs data across the user's own devices."
  - `calendar.readonly`: "AREITU reads calendar events to cross-reference them with photo-derived visit timestamps,
    helping the user confirm or correct where they were at a given time. Calendar data is never modified or sent to
    any third party."

## 審査提出前の準備

- [ ] デモ動画を用意する（審査担当者がスコープの必要性を確認するため）。構成案:
  1. アプリ起動 → 「Googleでログイン」ボタンを押す → OAuth 同意画面でスコープが表示される
  2. サインイン後、Google Drive 側でユーザーの通常のファイル一覧には何も表示されないこと（appDataFolder は
     不可視であることの説明）
  3. カレンダー同期を有効化し、カレンダーの予定と訪問履歴が突き合わされる画面
  4. 設定画面でサインアウトし、アクセス権を取り消せることを見せる
- [ ] テスターアカウントを Google Cloud Console の「Test users」に追加し、公開前に一通りログイン〜同期〜サインアウト
  までを実機（macOS または Windows の配布ビルド、Task 4 の `release.yml` で作成したもの）で確認する
- [ ] `drive.appdata` と `calendar.readonly` それぞれについて、限定利用ポリシー（Limited Use）に準拠している
  ことを確認する（取得したデータをこのアプリの機能提供以外の目的で使わない、広告に使わない、人間に読ませない、
  第三者に譲渡しない）。本プロジェクトはこの方針に合致している（`docs/pages/privacy.*.html` に明記済み）

## 提出

- [ ] Google Cloud Console の OAuth 同意画面から「Submit for verification」を行う
- [ ] 審査担当者からの追加質問・修正依頼にはオーナーが直接対応する
- [ ] 審査完了後、`crates/areitu-google/src/auth.rs` のスコープ定数（`SCOPE_DRIVE_APPDATA` など）と実際に承認
  されたスコープが一致していることを最終確認する
```

- [ ] **Step 5: チェックリストの Markdown にリンク切れがないことを軽く検証する**

Run: `/usr/bin/grep -c 'ikeikeikeda66.github.io/AREITU' docs/google-oauth-verification-checklist.md`
Expected: `5`（ホームページ・プライバシーポリシー(日英)・利用規約(日英)への言及、計5箇所）

- [ ] **Step 6: README に Pages の公開先を追記する**

`README.md` の「配布パッケージ（未署名ビルド）」セクションの前に追加する:

```markdown

## プライバシーポリシー・利用規約

- プライバシーポリシー: https://ikeikeikeda66.github.io/AREITU/privacy.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/privacy.en.html)）
- 利用規約: https://ikeikeikeda66.github.io/AREITU/terms.ja.html （[English](https://ikeikeikeda66.github.io/AREITU/terms.en.html)）
```

- [ ] **Step 7: コミット**

```bash
git add .github/workflows/pages.yml docs/google-oauth-verification-checklist.md README.md
git commit -m "$(cat <<'EOF'
docs(ci): deploy privacy/terms pages to GitHub Pages, add OAuth verification checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

## このプランの実行だけでは証明できないこと

- `release.yml` が実際のタグ push で macOS / Windows それぞれのランナー上で最後までバンドルを生成できるかどうか
  （ローカルでは `--debug` ビルドのみ検証可能。リリースビルド固有の問題、たとえば `universal-apple-darwin`
  ターゲットのクロスコンパイルや Windows の NSIS バンドラの挙動は、実際の CI 実行でしか確認できない）
- `TAURI_SIGNING_PRIVATE_KEY` / `AREITU_GOOGLE_CLIENT_ID` などのシークレットが正しく設定されているかどうか
  （ローカル環境には存在しないため、ワークフロー内の `env:` 参照が正しいことしか静的には確認できない）
- 公開された `latest.json` を実際のアプリが読みに行き、更新バナーが正しく表示され、`downloadAndInstall` から
  `relaunch` までが実機で成功するかどうか（Task 3 のユニットテストはプラグイン呼び出しをモックしており、実際の
  署名検証・ダウンロード・インストールは検証していない）
- GitHub Pages が有効化された後、公開 URL が実際に到達可能で Google の審査担当者から閲覧できるか
- Google OAuth 審査の可否そのもの（内容・体裁の妥当性は本プランで整えるが、承認は Google 側の判断）
