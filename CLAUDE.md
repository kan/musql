# muSQL project notes (for agents)

## Overview
- Windows向けの MySQL クライアント。Tauri v2 + Rust backend + 静的 UI。
- SSH 踏み台経由の MySQL 接続に対応。
- UI は `ui/` の素朴な HTML/JS/CSS。Node.js 不要。

## How to run (dev)
- 前提: Rust toolchain, Visual Studio Build Tools (C++), Windows OpenSSH Client (`C:\Windows\System32\OpenSSH\ssh.exe`)。
- 起動:
  - `cd src-tauri`
  - `cargo tauri dev`
- `cargo check` でコンパイル確認。

## 新規環境セットアップ (PowerShell)
1. **Rust toolchain のインストール**:
   ```powershell
   winget install Rustlang.Rustup
   ```
2. **Visual Studio Build Tools のインストール** (MSVC リンカー・Windows SDK):
   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```
   ※ ディスク空き容量 8GB 以上を推奨。
3. **PATH の設定** (rustup が自動で PATH に追加されない場合):
   ```powershell
   [Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";C:\Users\kanfu\.cargo\bin", "User")
   ```
   設定後、ターミナルを再起動して `rustc --version` で確認。
4. **Tauri CLI のインストール**:
   ```powershell
   cargo install tauri-cli
   ```
   ※ 初回は 10 分程度かかる。
5. **ビルド & 起動**:
   ```powershell
   cd src-tauri
   cargo tauri dev
   ```
   ※ 初回ビルドはクレートのダウンロード・コンパイルに時間がかかる。

## Architecture — マルチウィンドウ構成
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト・接続。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・データ/スキーマ表示・SQL 実行）。
- ウィンドウは `tauri.conf.json` で `visible: false` として事前定義。
- Rust 側の `open_*_window` で `show()` + `set_focus()`、閉じるときは `hide_window` で非表示にする（`close()` で破棄しない）。
- `on_window_event` で非 main ウィンドウの X ボタンも hide に変換。query ウィンドウ close 時に main を再表示。
- `open_query_window` で main ウィンドウを自動 hide、query ウィンドウ閉じで main を再表示 + focus。
- query ウィンドウのタイトルは `プロファイル名 / DB名 — muSQL` 形式で動的更新（Tauri `setTitle` API、`core:window:allow-set-title` パーミッション）。

## Main flow
- `ui/app.js` → Tauri `invoke`:
  - `open_settings_window` / `open_query_window` でサブウィンドウを表示。
  - `list_profiles` / `save_profile` / `delete_profile` でプロファイル CRUD。
- `ui/settings.js` → `test_connection` で接続テスト、`open_query_window` で接続（クエリウィンドウを開く）、`hide_window` でウィンドウを隠す。
- `ui/query.js` → DB エクスプローラ:
  - DB 選択モーダル → サイドバーにテーブル一覧 → タブで Data/Schema/SQL 表示。
  - SQL タブは CodeMirror 5 エディタ（MySQL シンタックスハイライト、キーワード＋テーブル名補完）。
  - 実行時エラーは `near '...' at line N` をパースしてエディタ上に赤波線でマーク。
  - `run_query` でクエリ実行（async + `spawn_blocking`、接続は Pool キャッシュ、DB 切替は `USE` で実行）。
  - クエリキャンセル: 実行中に Cancel ボタン表示。`cancel_query`（async）が `KILL QUERY <connection_id>` を別コネクションで送信。キャンセル時は「Query cancelled.」表示。
  - SQL 整形: Format ボタンで `sql-formatter`（vendor UMD）を使い選択範囲またはエディタ全体を MySQL 方言で整形。
  - 多重クリック抑止: 実行中は Run ボタンを disabled、Cancel ボタンもクリック後に disabled で二度押し防止。
  - `export_file` でネイティブ保存ダイアログ経由のファイル書き出し（CSV/TSV/SQL）。
  - エディタ下書き保存: SQL タブの内容を localStorage にプロファイル単位で自動保存（デバウンス 1 秒）。DB 切替・再接続時に復元。
  - 実行履歴: 成功した SQL を `musql:history:<profileId>` に最大 100 件保存。History ボタンから呼び出し・エディタに挿入。
- SSH 有効時:
  - Rust 側で SSH トンネルを起動、ローカルポートへ接続できるまで待機 → MySQL 接続 → 終了時に SSH プロセスを kill。

## Key files
- `src-tauri/src/main.rs` — MySQL 接続/クエリ、SSH トンネル管理、接続プール、ウィンドウ管理、プロファイル CRUD、ファイルエクスポート。
- `src-tauri/tauri.conf.json` — Tauri 設定・ウィンドウ定義。
- `src-tauri/capabilities/default.json` — パーミッション。
- `ui/index.html` + `ui/app.js` — メイン画面（接続一覧）。
- `ui/settings.html` + `ui/settings.js` — 設定画面。
- `ui/query.html` + `ui/query.js` — DB エクスプローラ画面。
- `ui/style.css` — 共通スタイル。
- `ui/icon.svg` — アプリアイコン（鼠モチーフ SVG）。favicon + main ヘッダーロゴ。
- `ui/icon.png` — アイコン変換元 PNG。`cargo tauri icon` で `src-tauri/icons/` を生成。
- `src-tauri/icons/` — Tauri アプリアイコン（各サイズ PNG/ICO/ICNS）。`cargo tauri icon ui/icon.png` で再生成。
- `ui/icons.js` — Lucide アイコン SVG パスデータ（28 個）+ `icon(name, size)` ヘルパー関数。
- `ui/theme.js` — ダークモード管理（テーマ検出・適用・トグルボタン生成・cross-window 同期）。
- `ui/i18n.js` — i18n 管理（日英翻訳データインライン埋め込み、`t(key, params)` ヘルパー、言語トグルボタン生成・cross-window 同期）。
- `ui/lib/codemirror/` — CodeMirror 5 vendored ファイル（コア、SQL モード、補完アドオン）。
- `ui/lib/sql-formatter/` — sql-formatter vendored UMD ビルド（SQL 整形）。

## Behavior notes
- パスワード安全保存: `keyring` クレート（Windows Credential Manager）で profile ID をキーにパスワードを保存。`connections.json` にはパスワードを含めない（`#[serde(skip_serializing)]`）。`load_profiles()` で旧 JSON 内パスワードを keyring へ自動マイグレーション。`test_connection` / `run_query` は `profile_id` パラメータで keyring からパスワードを解決。`has_password` コマンドで保存状態を確認可能。Settings UI はプレースホルダー `"(saved - leave blank to keep)"` で保存済みを表示。
- `test_connection` は `SELECT 1` を実行（per-request 接続、プールは使わない）。
- `run_query` は `async fn` + `spawn_blocking` で UI スレッドをブロックせず任意 SQL を実行。列名＋行＋ `affected_rows` を返却。`max_rows` パラメータで行数制限（デフォルト 500、0 で無制限）。
- クエリキャンセル: `RUNNING_QUERY`（グローバル `LazyLock<RunningQuery>`）に実行中の connection_id と Pool クローンを保持。`cancel_query`（async）が `KILL QUERY` を `spawn_blocking` で別コネクション経由で送信。
- 接続プール: `ConnectionCache` を `Arc<Mutex<...>>` で管理し `spawn_blocking` に移動可能。Pool + SshTunnel をキャッシュ。fingerprint（ホスト/ポート/ユーザー/SSL/SSH 設定）が同じなら再利用。DB は Pool opts に含めず `USE` で切替。
- SSL モード: `MySqlConfig.ssl_mode` で DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY を選択。VERIFY_CA/VERIFY_IDENTITY 時は `tls_ca_cert_path` で CA 証明書を指定可能。`native-tls` (SChannel) バックエンドで PEM/DER 対応。旧 `tls_enabled`/`tls_skip_verify` フィールドは `skip_serializing` で読み込み互換のみ残し、`load_profiles()` で自動マイグレーション。
- `pick_file` コマンド: `rfd` でネイティブファイル選択ダイアログを表示しパスを返す。CA 証明書・SSH IdentityFile の Browse に使用。
- `export_file` は `rfd` クレートでネイティブ保存ダイアログを表示しファイル書き出し。
- `ssh.private_key_path` は SSH の `IdentityFile` として `-o IdentityFile=<path> -o IdentitiesOnly=yes` で渡す。`.pub` ファイル指定で 1Password SSH agent と連携可能。
- SSH バイナリは `C:\Windows\System32\OpenSSH\ssh.exe` を優先（Git 付属の ssh.exe は 1Password SSH agent 非対応のため）。
- SSH トンネル失敗時は stderr の内容をエラーメッセージに含めて返す。
- Settings UI: SSH Bastion の Enable チェックはパネルヘッダー横に配置。off 時はフィールド群を disabled 化。Profile name 未入力時は保存ボタン disabled。「接続」ボタン（success）で既存プロファイルのクエリウィンドウを直接開ける（新規作成時は disabled）。「接続テスト」ボタンは info スタイル。
- Profile の色とタグ: `ConnectionProfile` に `color: Option<String>` と `tags: Vec<String>`（`#[serde(default)]` で後方互換）。Settings UI に 8 色カラーパレット（red/orange/yellow/green/teal/blue/purple/pink + None）とタグ入力（プリセット + 既存タグ補完、Enter/カンマ確定、Backspace 削除）。メイン画面ツリーに左端カラーバー（4px）とタグバッジを表示。
- タグフィルター: メイン画面の検索欄下にタグチップバーを表示。クリックでタグ絞り込み（トグル）。テキストフィルターとの AND 条件。タグ未使用時は非表示。
- `open_settings_window` は `id` と `group_id` をペイロード `{ id, group_id }` として emit。グループ右クリック「設定を追加」で group_id 付きで新規作成可能。
- カラムソート: `renderTable()` で全テーブル（Data/Schema/SQL 結果）の `<th>` クリックで ASC→DESC→なし の 3 ステートトグル。ソートインジケータ（▲▼）表示。`rows.slice()` で作業コピーをソートし原本不変。NULL は常に最後、空文字は NULL の手前。外部 `sortState` オブジェクトを渡すとページ切替でもソート維持。
- 行詳細モーダル: テーブル行クリックで `showRowDetailModal(columns, row)` を表示。`.row-detail-box` で grid(160px 1fr) のカラム名・値ペア。JSON 風文字列は自動整形・monospace 表示。×ボタン/overlay クリック/Escape で閉じる。Data タブは PK ベースで `SELECT`（BLOB 除外）し全文データを取得して表示。PK なし時は一覧データをそのまま表示。
- BLOB/TEXT 切り詰め（Data タブ）: `INFORMATION_SCHEMA.COLUMNS` で BLOB 系・TEXT 系・PK カラムを検出。Truncate モード ON（デフォルト）で BLOB→`'(BLOB)'` プレースホルダー、TEXT→SQL レベルで 200 文字に切り詰め。フッターの Truncate ボタンで ON/OFF トグル。BLOB/TEXT カラムがないテーブルではボタン非表示。
- ダークモード: `ui/theme.js`（IIFE）が全ページで読み込まれ、`html.dark` クラスでテーマ切替。`:root` に Light、`html.dark` に Dark の CSS 変数パレットを定義。`prefers-color-scheme` フォールバック + 手動トグル（fixed 右下の丸ボタン、sun/moon アイコン）。トグルボタンは main ウィンドウのみ表示（`body[data-theme-toggle]`）。settings/query は `storage` イベントで自動追従。`musql:theme` を localStorage に保存。CodeMirror は CSS セレクタ `html.dark .CodeMirror*` で Material 風ダークシンタックスハイライト上書き（JS 側変更なし）。
- i18n: `ui/i18n.js`（IIFE）が全ページで読み込まれ、日本語 (ja) / 英語 (en) を切替。翻訳データはインライン埋め込み（非同期読み込み不要・FOUC 防止）。`t(key, params)` で文字列取得（`{param}` テンプレート変数対応）。HTML 属性 `data-i18n`/`data-i18n-placeholder`/`data-i18n-title` で静的テキストを一括適用。JS 内の動的文字列は `t()` で直接置換。言語検出順: localStorage → `navigator.language` → デフォルト `ja`。言語トグルボタンは main ウィンドウのみ表示（テーマボタン左隣の 36px 円形、EN/JA 表示）。settings/query は `storage` イベント + `musql:langchange` カスタムイベントで自動追従。翻訳しない文字列: SQL/CSV/TSV（フォーマット名）、NULL/EMPTY（DB 概念）、SSL モード値、MySQL エラーメッセージ、タグプリセット名。

## Limits / defaults
- 結果行はデフォルト最大 500 件（`max_rows` で変更可能、エクスポート時は無制限）。
- データタブのページングはデフォルト 100 件/ページ（50/100/200/500 から選択可能）。
- SSH トンネル確立待ちタイムアウトは 8 秒。

## TODO
(上から優先度順)

- 見た目の強化
- 接続設定のインポートとエクスポート
- docker上のmysqlへの簡単アクセス
- ビルド・配布手段の検討
- Windows以外での動作
- SSH秘密鍵パスフレーズ対応
  - 設定画面でパスフレーズを保存（keyring）
  - 接続時に SSH_ASKPASS 経由で自動入力（一時 .cmd スクリプト生成→削除）
  - パスフレーズ未保存時は都度入力ダイアログ（Tauri dialog）

## localStorage keys
- `musql:collapsed`: グループの開閉状態（`app.js`）。
- `musql:drafts:<profileId>`: SQL タブのエディタ内容（`string[]`、タブ順）。
- `musql:history:<profileId>`: 実行済み SQL（`{ sql, ts }[]`、新しい順、最大 100 件）。
- `musql:theme`: テーマ設定（`"light"` | `"dark"` | 未設定=システム準拠）。
- `musql:lang`: 言語設定（`"ja"` | `"en"` | 未設定=`navigator.language` フォールバック→デフォルト `ja`）。

## Strategy
- パスワード安全保存: **実装済み**。`keyring` クレート（Windows Credential Manager）で profile ID をキーにパスワードを保存。`connections.json` にはパスワードを含めない。旧データは `load_profiles()` で自動マイグレーション。
- SSH パスフレーズ対応:
  - 保存: MySQL パスワードと同様に keyring に保存（キー: `musql:ssh-passphrase:<profileId>` 等）。Settings UI にパスフレーズ入力欄を追加。
  - 自動入力: SSH トンネル起動時に `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force` で一時 `.cmd` スクリプト（パスフレーズを echo）を指定。確立後に即削除。
  - 都度入力: パスフレーズ未保存時は Tauri dialog でパスフレーズ入力を求め、同様に SSH_ASKPASS 経由で渡す。
- SQL 多重クリック抑止: **実装済み**。実行中は Run ボタン disabled + Cancel ボタン表示。Cancel も一度押したら disabled。`executing` フラグで Ctrl+Enter 含む全エントリポイントをガード。
- SQL 整形: **実装済み**。`sql-formatter@15.4.10` UMD ビルドを `ui/lib/sql-formatter/` に vendor。Format ボタンで選択範囲またはエディタ全体を MySQL 方言で整形。
- クエリキャンセル: **実装済み**。`run_query` を `async fn` + `spawn_blocking` で非同期化し UI ブロックを解消。`RUNNING_QUERY` グローバルに connection_id + Pool を保持、`cancel_query`（async）が `KILL QUERY` を別コネクションで送信。JS 側は `cancelled` フラグで結果表示を「Query cancelled.」に差し替え。
- Table タブ機能強化: **実装済み**。カラムソート（`<th>` クリックで ASC/DESC/なしトグル、▲▼インジケータ、ページ切替でも維持）。行詳細モーダル（行クリックで key-value 表示、PK ベース全文取得、JSON 自動整形）。BLOB/TEXT 切り詰め（`INFORMATION_SCHEMA.COLUMNS` で検出、BLOB は `'(BLOB)'` プレースホルダー、TEXT は 200 文字切り詰め、Truncate ボタンでトグル）。
- ダークモード: **実装済み**。`:root` に Light パレット、`html.dark` に Dark パレットを CSS 変数で定義。全ハードコード色を変数化。`ui/theme.js`（IIFE）でテーマ検出（localStorage → `prefers-color-scheme` フォールバック）、即時適用（FOUC 防止）、トグルボタン（fixed 右下、sun/moon アイコン）、cross-window 同期（`storage` イベント）。CodeMirror は `html.dark .CodeMirror*` セレクタで Material 風ダークシンタックスハイライト。
- i18n: **実装済み**。`ui/i18n.js`（IIFE）に日英翻訳データをインライン埋め込み。`t(key, params)` ヘルパーで文字列取得（テンプレート変数 `{name}` 対応）。`musql:lang` を localStorage に保存。HTML 静的テキストは `data-i18n`/`data-i18n-placeholder`/`data-i18n-title` 属性で一括適用。JS 動的文字列は `t()` で直接置換。main ウィンドウにテーマボタン左隣に EN/JA 言語トグルボタン。cross-window 同期は `storage` イベント + `musql:langchange` カスタムイベント。
- アイコン: **実装済み**。Lucide SVG アイコン 28 個を `ui/icons.js` に格納（MIT ライセンス、24x24 viewBox、stroke-based）。`icon(name, size)` ヘルパーで `<svg class="icon">` を返す。`stroke="currentColor"` でボタン文字色を自動継承。全 HTML ページの `<script>` で読み込み、ボタン・コンテキストメニュー・見出し・リスト項目・タブヘッダーにインライン SVG で適用。main ヘッダーボタンはアイコンのみ（title 属性でツールチップ）。フィルター入力欄内に虫めがねアイコン配置。タグフィルターチップ内にもアイコン。
- favicon / アプリアイコン: **実装済み**。鼠モチーフの SVG アイコン (`ui/icon.svg`)。全 HTML に `<link rel="icon">` 設定。`cargo tauri icon ui/icon.png` で Tauri アプリアイコン（タスクバー・タイトルバー・ICO・ICNS）も生成済み。main ウィンドウヘッダーにロゴ + タイトル横並び表示。
- 接続設定インポート/エクスポート: JSON 形式でファイル書き出し/読み込み。`pick_file` / `export_file` を再利用。パスワードを含めるかはオプション。
- Docker MySQL: `docker ps` でコンテナ一覧を取得し、MySQL コンテナを検出。ポートマッピングから接続先を自動入力。
- ビルド・配布: `cargo tauri build` で MSI/NSIS インストーラー生成。GitHub Actions で CI 自動ビルド。GitHub Releases で配布。
- クロスプラットフォーム: macOS/Linux 動作確認。SSH バイナリパス分岐は `cfg!(target_os)` で既に対応済み。CI でマルチプラットフォームビルドを追加。
