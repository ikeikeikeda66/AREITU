# Google OAuth 審査対応チェックリスト

このチェックリストの作業はすべてオーナー（Google Cloud Console と GitHub リポジトリの管理者）が行う。エージェントは代行しない。
「OWNER ACTION」と付いていない項目は、コードと公開ページがすでに満たしている内容の確認用である。

## 前提（コードと公開ページの内容）

- OAuth クライアントの種類: **Desktop app**（デスクトップアプリ）
- ホームページ: `https://ikeikeikeda66.github.io/AREITU/`
- プライバシーポリシー: `https://ikeikeikeda66.github.io/AREITU/privacy.en.html`（英語）/ `https://ikeikeikeda66.github.io/AREITU/privacy.ja.html`（日本語）
- 利用規約: `https://ikeikeikeda66.github.io/AREITU/terms.en.html`（英語）/ `https://ikeikeikeda66.github.io/AREITU/terms.ja.html`（日本語）
- 上記 URL は、GitHub Pages を有効化し `pages.yml` が `main` で成功するまで 404 になる。
- スコープ定義: `crates/areitu-google/src/lib.rs`

| スコープ | 定数 | いつ要求するか | 分類 |
|---|---|---|---|
| `https://www.googleapis.com/auth/drive.appdata` | `SCOPE_DRIVE_APPDATA` | サインイン時に常に | 非機微（non-sensitive） |
| `https://www.googleapis.com/auth/calendar.readonly` | `SCOPE_CALENDAR_READONLY` | 設定でカレンダー取り込みを有効にした場合のみ（incremental authorization） | **機微（sensitive）** |

`calendar.readonly` は機微スコープであり、テストユーザー以外に公開する前に Google の OAuth 審査（verification）が必須。
`drive.appdata` は非機微だが、同じアプリで機微スコープを申請するため、審査は同意画面全体に対して行われる。
最終的な分類は Cloud Console のスコープ追加画面の表示を正とする。

## データの流れ（審査の説明用）

- 開発者のサーバーは存在しない。Google ユーザーデータが開発者に送られることはない。
- `drive.appdata`: `areitu.db`（SQLite）をユーザー自身の Google Drive の appDataFolder にアップロード・ダウンロードする。競合時は同フォルダに `areitu-conflict-<timestamp>.db` を作る。通常の Drive 画面には表示されない。
- `calendar.readonly`: 予定のタイトルと時間帯を読み取り、同じ時間帯の訪問の場所名を推定する手がかりにする。カレンダーは書き換えない。
- 例外（プライバシーポリシーに明記済み）: ユーザーが設定で LLM（Ollama / OpenAI / Gemini）を有効にした場合に限り、同じ時間帯のカレンダー予定タイトル・場所と、ユーザーが取り込んだ Google Timeline ファイル由来の場所名（Google ユーザーデータではないが同じプロンプトに入る）が、ユーザーが選んだ LLM の送信先へプロンプトの一部として送られる。既定は無効。
- 認証情報の保存: Google のリフレッシュトークンは OS のキーチェーンのみ。`config.json` にはキーを含まない。
- `areitu.db` を AREITU は暗号化しない（Drive 上は Google の保存時暗号化のみ）。

## 各スコープの必要性の説明（同意画面の記入用下書き）

- `drive.appdata`: "AREITU keeps a visit-log database file (areitu.db) on the user's device. When the user signs in, this file is uploaded to and downloaded from the appDataFolder of the user's own Google Drive so the same data can be synced between the user's own devices. The folder is hidden from the normal Drive UI and accessible only to this app. If a sync conflict occurs, a backup copy is saved in the same folder."
- `calendar.readonly`: "Only if the user enables calendar import in Settings, AREITU reads the titles and time ranges of the user's calendar events. They are used as hints to infer the place name of visits in the same time range. The calendar is never modified. The data is not sent to the developer or used for advertising. If the user separately enables an LLM for place-name inference, event titles and locations for the same time range, and place names from any Google Timeline file the user imported, are included in the prompt sent to the destination the user selected."

## Limited Use の声明

`privacy.en.html` / `privacy.ja.html` の "Google user data (Limited Use)" に次の内容を記載済み。Cloud Console の提出フォームの回答はこれと矛盾させない。

- AREITU による Google API から受け取った情報の使用と他アプリへの転送は、Google API Services User Data Policy（Limited Use の要件を含む）に準拠する。
- Drive データは自分のデータベースの同期にのみ使い、カレンダーデータは訪問の場所名推定にのみ使う。
- Google ユーザーデータを開発者や第三者に転送しない（ユーザーが有効にした LLM への予定タイトル送信を除く）。
- 広告に使わない。開発者はデータを閲覧しない（アクセスできない）。

## OWNER ACTIONS: GitHub

- [ ] リポジトリ Settings → Pages → Build and deployment → Source を **GitHub Actions** に設定する（「Deploy from a branch」ではない）
- [ ] `main` に `docs/pages/**` の変更を含む push を行うか、Actions タブから `pages` ワークフローを手動実行（workflow_dispatch）して、デプロイ成功を確認する
- [ ] 公開後にブラウザで次を開き、すべて 200 で表示されることを確認する: ホームページ、privacy.ja / privacy.en、terms.ja / terms.en。ホームページから各ページへのリンクも確認する
- [ ] `docs/superpowers/plans/` などが公開 URL 配下に出ていないことを確認する（`docs/pages/` のみを公開する設定）

## OWNER ACTIONS: Google Cloud Console

- [ ] プロジェクトで Google Drive API と Google Calendar API を有効化する
- [ ] OAuth クライアントを作成する。アプリケーションの種類は **Desktop app**。クライアント ID を、リリースビルドの `AREITU_GOOGLE_CLIENT_ID` として GitHub の Secrets / 設定に登録する（値をリポジトリにコミットしない）
- [ ] OAuth 同意画面のユーザータイプを **External** にする
- [ ] アプリ名を `AREITU` に設定する
- [ ] ユーザーサポートメールとデベロッパー連絡先メールを設定する
- [ ] アプリのホームページに `https://ikeikeikeda66.github.io/AREITU/` を設定する
- [ ] プライバシーポリシー URL に `https://ikeikeikeda66.github.io/AREITU/privacy.en.html` を設定する
- [ ] 利用規約 URL に `https://ikeikeikeda66.github.io/AREITU/terms.en.html` を設定する
- [ ] 承認済みドメインの登録・所有権確認を行う。`github.io` は公開サフィックスのため、Cloud Console や Search Console で所有権を確認できない可能性がある。確認できない場合の対応（独自ドメインの取得と Pages への割り当てなど）を審査前に決める
- [ ] スコープを追加する: `.../auth/drive.appdata`、`.../auth/calendar.readonly`
- [ ] 各スコープの必要性の説明に、上記の下書きを記入する
- [ ] 公開前に、自分の Google アカウントを「テストユーザー」に追加し、配布ビルドでサインイン → 同期 → カレンダー取り込み → サインアウトを一通り確認する（テストモード中はリフレッシュトークンが 7 日で失効する点に注意）

## デモ動画の構成案（YouTube に限定公開などでアップロードし、URL を提出）

`calendar.readonly` の審査には、同意画面と各スコープの実際の使用場面を示す動画が必要。

1. アプリを起動し、画面で使用中の OAuth クライアント ID（Cloud Console のプロジェクト）を示す
2. 「Google でサインイン」を押し、ブラウザに同意画面が表示されるところを映す。URL バーの `client_id` と、要求スコープ（`drive.appdata` のみ）が見える状態にする
3. サインイン後、`areitu.db` が appDataFolder に同期される様子を示す（アプリの同期表示）。Drive の通常画面にファイルが出ないこと、Drive 設定の「アプリの管理」に AREITU の非表示データがあることを示す
4. 設定でカレンダー取り込みを有効にする。増分認可の同意画面に `calendar.readonly` が追加で表示されるところを映す
5. 取り込んだカレンダー予定が、同じ時間帯の訪問の場所名推定に使われる画面を示す
6. LLM が既定で無効であることを設定画面で示す
7. サインアウトし、Google アカウントのサードパーティアクセスのページでアクセス権を削除できることを示す

## OWNER ACTIONS: 提出

- [ ] Cloud Console の OAuth 同意画面から「Submit for verification」を実行する
- [ ] Google からの追加質問・修正依頼にオーナーが対応する
- [ ] 承認後、`crates/areitu-google/src/lib.rs` のスコープ定数と、承認されたスコープが一致していることを確認する
- [ ] 承認後、公開ステータスを「Testing」から「In production」に切り替える
