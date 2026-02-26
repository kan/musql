# muSQL project notes (for agents)

## Overview
- Windows向けの MySQL クライアント。Tauri v2 + Rust backend + 静的 UI。
- SSH 踏み台経由の MySQL 接続に対応。
- UI は `ui/` の素朴な HTML/JS/CSS。Node.js 不要。

## How to run (dev)
- 前提: Rust toolchain, Visual Studio Build Tools (C++)。
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
  - `russh` クレート（純 Rust SSH）でトンネルを確立。`tokio::net::TcpListener` でローカルポートをバインドし、`channel_open_direct_tcpip` + `tokio::io::copy_bidirectional` で中継。MySQL は `127.0.0.1:<local_port>` に接続。

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
- `ui/icons.js` — Lucide アイコン SVG パスデータ（29 個）+ `icon(name, size)` ヘルパー関数。
- `ui/theme.js` — ダークモード管理（テーマ検出・適用・トグルボタン生成・cross-window 同期）。
- `ui/i18n.js` — i18n 管理（日英翻訳データインライン埋め込み、`t(key, params)` ヘルパー、言語トグルボタン生成・cross-window 同期）。
- `ui/lib/codemirror/` — CodeMirror 5 vendored ファイル（コア、SQL モード、補完アドオン）。
- `ui/lib/sql-formatter/` — sql-formatter vendored UMD ビルド（SQL 整形）。
- `.github/workflows/release.yml` — リリースワークフロー（`v*` タグ push → NSIS ビルド → GitHub Release）。
- `.github/workflows/ci.yml` — CI ワークフロー（main push/PR → `cargo check`）。

## Behavior notes
- パスワード安全保存: `keyring` クレート（Windows Credential Manager）で profile ID をキーにパスワードを保存。`connections.json` にはパスワードを含めない（`#[serde(skip_serializing)]`）。`load_profiles()` で旧 JSON 内パスワードを keyring へ自動マイグレーション。`test_connection` / `run_query` は `profile_id` パラメータで keyring からパスワードを解決。`has_password` コマンドで保存状態を確認可能。Settings UI はプレースホルダー `"(saved - leave blank to keep)"` で保存済みを表示。
- `test_connection` は `SELECT 1` を実行（per-request 接続、プールは使わない）。
- `run_query` は `async fn` + `spawn_blocking` で UI スレッドをブロックせず任意 SQL を実行。列名＋行＋ `affected_rows` を返却。`max_rows` パラメータで行数制限（デフォルト 500、0 で無制限）。
- クエリキャンセル: `RUNNING_QUERY`（グローバル `LazyLock<RunningQuery>`）に実行中の connection_id と Pool クローンを保持。`cancel_query`（async）が `KILL QUERY` を `spawn_blocking` で別コネクション経由で送信。
- 接続プール: `ConnectionCache` を `Arc<Mutex<...>>` で管理し `spawn_blocking` に移動可能。Pool + SshTunnel をキャッシュ。fingerprint（ホスト/ポート/ユーザー/SSL/SSH 設定）が同じなら再利用。DB は Pool opts に含めず `USE` で切替。
- SSL モード: `MySqlConfig.ssl_mode` で DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY を選択。VERIFY_CA/VERIFY_IDENTITY 時は `tls_ca_cert_path` で CA 証明書を指定可能。`native-tls` (SChannel) バックエンドで PEM/DER 対応。旧 `tls_enabled`/`tls_skip_verify` フィールドは `skip_serializing` で読み込み互換のみ残し、`load_profiles()` で自動マイグレーション。
- `pick_file` コマンド: `rfd` でネイティブファイル選択ダイアログを表示しパスを返す。CA 証明書・SSH IdentityFile の Browse に使用。
- `export_file` は `rfd` クレートでネイティブ保存ダイアログを表示しファイル書き出し。
- インポート/エクスポート: `export_profiles` で全プロファイル＋グループを JSON ファイルにエクスポート。パスワード含めるかは `include_passwords` フラグで選択（keyring から取得して `passwords` マップに格納）。`import_profiles` で JSON ファイルを読み込みインポート。重複検出付き 2 段階コール方式: 初回（`mode=None`）でファイル読み込み＋重複チェック。重複なしならそのままインポート。重複あり（同名グループ or 同名＋同グループのプロファイル）なら `conflicts` + `file_path` を返し、UI が `confirm()` で上書き/新規追加を選択して 2 回目（`mode="overwrite"|"add"`）を呼ぶ。overwrite 時は同名グループの ID を再利用し、同名＋同グループのプロファイルは既存を上書き（request/color/tags 更新、ID 維持）。add 時は常に新 ID 生成。パスワードは新/既存 ID に対応付けて keyring に保存。メイン画面ヘッダーに upload/download アイコンボタンを配置。
- SSH トンネル: `russh` クレート（v0.48、純 Rust、Tokio async）で実装。外部 ssh.exe バイナリ不要。`SshTunnel` は `tokio::task::JoinHandle` を保持し、Drop で abort。
- SSH 認証優先順位: (1) `private_key_path` 指定あり → `russh_keys::load_secret_key(path, passphrase)` でファイルから鍵を読み込み `authenticate_publickey`。(2) SSH agent に問い合わせ（Windows: `\\.\pipe\openssh-ssh-agent`、1Password 対応。Unix: `SSH_AUTH_SOCK`）→ `authenticate_publickey_with` で各 identity を試行。(3) デフォルト鍵ファイル（`~/.ssh/id_ed25519`, `id_rsa`, `id_ecdsa`）を順に試行。
- SSH 秘密鍵パスフレーズ: MySQL パスワードと同パターンで keyring（`{profile_id}:ssh_passphrase` キー）に保存。`SshConfig.passphrase` フィールド（`skip_serializing`）で受け渡し。`resolve_ssh_passphrase` で keyring から解決。Settings UI に Passphrase 入力欄（IdentityFile の下）、保存済み時はプレースホルダー表示。プロファイル複製・エクスポート/インポート（`ssh_passphrases` マップ）にも対応。デフォルト鍵ファイル（~/.ssh/id_*）はパスフレーズなし（`None`）で試行。
- SSH config: `config_host` 指定時、`resolve_ssh_config_host` で `~/.ssh/config` をパースし `HostName`/`Port`/`User`/`IdentityFile` を解決。
- SSH ホスト鍵検証: 初期実装では全受け入れ（`check_server_key` で常に `Ok(true)`）。
- SSH 接続タイムアウト: `tokio::time::timeout` で 8 秒。
- Settings UI: SSH Bastion の Enable チェックはパネルヘッダー横に配置。off 時はフィールド群を disabled 化。Profile name 未入力時は保存ボタン disabled。「接続」ボタン（success）で既存プロファイルのクエリウィンドウを直接開ける（新規作成時は disabled）。「接続テスト」ボタンは info スタイル。
- Profile の色とタグ: `ConnectionProfile` に `color: Option<String>` と `tags: Vec<String>`（`#[serde(default)]` で後方互換）。Settings UI に 8 色カラーパレット（red/orange/yellow/green/teal/blue/purple/pink + None）とタグ入力（プリセット + 既存タグ補完、Enter/カンマ確定、Backspace 削除）。メイン画面ツリーに左端カラーバー（4px）とタグバッジを表示。
- タグフィルター: メイン画面の検索欄下にタグチップバーを表示。クリックでタグ絞り込み（トグル）。テキストフィルターとの AND 条件。タグ未使用時は非表示。
- `open_settings_window` は `id` と `group_id` をペイロード `{ id, group_id }` として emit。グループ右クリック「設定を追加」で group_id 付きで新規作成可能。
- カラムソート: `renderTable()` で全テーブル（Data/Schema/SQL 結果）の `<th>` クリックで ASC→DESC→なし の 3 ステートトグル。ソートインジケータ（▲▼）表示。`rows.slice()` で作業コピーをソートし原本不変。NULL は常に最後、空文字は NULL の手前。外部 `sortState` オブジェクトを渡すとページ切替でもソート維持。
- 行詳細モーダル: テーブル行クリックで `showRowDetailModal(columns, row)` を表示。`.row-detail-box` で grid(160px 1fr) のカラム名・値ペア。JSON 風文字列は自動整形・monospace 表示。×ボタン/overlay クリック/Escape で閉じる。Data タブは PK ベースで `SELECT`（BLOB 除外）し全文データを取得して表示。PK なし時は一覧データをそのまま表示。
- BLOB/TEXT 切り詰め（Data タブ）: `INFORMATION_SCHEMA.COLUMNS` で BLOB 系・TEXT 系・PK カラムを検出。Truncate モード ON（デフォルト）で BLOB→`'(BLOB)'` プレースホルダー、TEXT→SQL レベルで 200 文字に切り詰め。フッターの Truncate ボタンで ON/OFF トグル。BLOB/TEXT カラムがないテーブルではボタン非表示。
- ダークモード: `ui/theme.js`（IIFE）が全ページで読み込まれ、`html.dark` クラスでテーマ切替。`:root` に Light、`html.dark` に Dark の CSS 変数パレットを定義。`prefers-color-scheme` フォールバック + 手動トグル（fixed 右下の丸ボタン、sun/moon アイコン）。トグルボタンは main ウィンドウのみ表示（`body[data-theme-toggle]`）。settings/query は `storage` イベントで自動追従。`musql:theme` を localStorage に保存。CodeMirror は CSS セレクタ `html.dark .CodeMirror*` で Material 風ダークシンタックスハイライト上書き（JS 側変更なし）。
- i18n: `ui/i18n.js`（IIFE）が全ページで読み込まれ、日本語 (ja) / 英語 (en) を切替。翻訳データはインライン埋め込み（非同期読み込み不要・FOUC 防止）。`t(key, params)` で文字列取得（`{param}` テンプレート変数対応）。HTML 属性 `data-i18n`/`data-i18n-placeholder`/`data-i18n-title` で静的テキストを一括適用。JS 内の動的文字列は `t()` で直接置換。言語検出順: localStorage → `navigator.language` → デフォルト `ja`。言語トグルボタンは main ウィンドウのみ表示（テーマボタン左隣の 36px 円形、EN/JA 表示）。settings/query は `storage` イベント + `musql:langchange` カスタムイベントで自動追従。翻訳しない文字列: SQL/CSV/TSV（フォーマット名）、NULL/EMPTY（DB 概念）、SSL モード値、MySQL エラーメッセージ、タグプリセット名。

- バージョン表示: メイン画面ヘッダーに `window.__TAURI__.app.getVersion()` で取得したバージョン（`v0.1.0` 形式）を `#app-version` span で表示。
- 自動更新: 起動 3 秒後に `tauri-plugin-updater` で更新チェック。更新あれば `update-available` イベントで UI に通知 → info 色バナーを `.app-header` の下に表示。「更新して再起動」ボタンで `install_update` コマンド実行 → NSIS passive インストール → アプリ再起動。

- ネイティブメニュー: `tauri::menu` API でウィンドウごとにメニューを構築。メニューバーは `setup_menus()` で起動時に一度だけ `set_menu()` + `hide_menu()` し、アクセラレータ登録用に保持（非表示）。各ウィンドウにハンバーガーボタン（Lucide `menu` アイコン、右上配置）を設置し、クリック時に `show_popup_menu` コマンドで `lang`/`theme` を渡してオンデマンドにメニューを構築 → `window.popup_menu()` でポップアップ表示。毎回新規構築のため `set_menu()` の再呼び出しが不要で画面のちらつきが発生しない。メニュー構成: main（File/Edit/View/Help）、query（File/Edit/View/Query）、settings（Edit/View/Settings）。Edit メニューは `PredefinedMenuItem`（OS ネイティブ Cut/Copy/Paste/Undo/Redo/Select All）。View メニューは全ウィンドウ共通で `CheckMenuItemBuilder` によるテーマ（Light/Dark）・言語（English/日本語）選択を含む（query の View にはさらに「DB 切替」）。チェック状態は `show_popup_menu` 呼び出し時の `lang`/`theme` パラメータから設定。Settings メニューは「接続テスト」「接続」「保存」「削除」。メニューイベントは `on_menu_event` で処理: Rust 完結アクション（exit/github/close）以外は `emit_to(window, "menu:action", action)` でフロントエンドへ転送。JS 側は `eventApi.listen("menu:action", ...)` でルーティング。theme-light/theme-dark は `setTheme()` を、lang-en/lang-ja は `setLang()` を呼び出し。query ウィンドウの SQL タブアクション（run/runAll/format/cancel）は `sqlTabActions` グローバルマップ経由で active tab のボタンに委譲。アクセラレータはメニュー非表示でもウィンドウに紐付いたまま動作: Ctrl+N（New Profile）、Ctrl+Shift+N（New Group）、Ctrl+T（New SQL Tab）、Ctrl+W（Close）、Ctrl+Enter（Run）、Ctrl+Shift+Enter（Run All）、Ctrl+Shift+F（Format）。メニューラベル i18n: Rust 側 `ml()` 関数で日英翻訳（サブメニュー名・メニュー項目名）。ハンバーガーボタン配置: main は `.app-header` 右端、query は `.tab-bar` 内タブ追加ボタンの右、settings は `.settings-header` 右端。

## Limits / defaults
- 結果行はデフォルト最大 500 件（`max_rows` で変更可能、エクスポート時は無制限）。
- データタブのページングはデフォルト 100 件/ページ（50/100/200/500 から選択可能）。
- SSH トンネル確立待ちタイムアウトは 8 秒。

## TODO
(上から優先度順)

- SSH ホスト鍵検証（known_hosts / TOFU）
- docker上のmysqlへの簡単アクセス
- Windows以外での動作

## localStorage keys
- `musql:collapsed`: グループの開閉状態（`app.js`）。
- `musql:drafts:<profileId>`: SQL タブのエディタ内容（`string[]`、タブ順）。
- `musql:history:<profileId>`: 実行済み SQL（`{ sql, ts }[]`、新しい順、最大 100 件）。
- `musql:theme`: テーマ設定（`"light"` | `"dark"` | 未設定=システム準拠）。
- `musql:lang`: 言語設定（`"ja"` | `"en"` | 未設定=`navigator.language` フォールバック→デフォルト `ja`）。

## Strategy
- 接続設定インポート/エクスポート: 実装済み。`export_profiles` / `import_profiles` コマンド。JSON 形式（`ExportData` 構造体）。パスワードはオプションで `passwords` マップに profile ID → パスワードで格納。インポート時は全 ID を再生成して重複回避。
- SSH 接続方法: 実装済み（russh）。`russh` v0.48（純 Rust、Tokio async）で SSH トンネルを実装。ssh.exe 外部バイナリ依存を排除。`channel_open_direct_tcpip` + `tokio::io::copy_bidirectional` でローカル TCP リスナー方式のトンネル。SSH agent（Windows named pipe / Unix socket）対応。`~/.ssh/config` パース対応。ホスト鍵検証は全受け入れ（将来 known_hosts / TOFU 対応予定）。
- Docker MySQL: `docker ps` でコンテナ一覧を取得し、MySQL コンテナを検出。ポートマッピングから接続先を自動入力。
- ビルド・配布: 実装済み。`cargo tauri build` で NSIS インストーラー生成。`.github/workflows/release.yml` で `v*` タグ push 時に `tauri-apps/tauri-action@v0` で自動ビルド → GitHub Releases に NSIS インストーラー + `latest.json` をアップロード。`.github/workflows/ci.yml` で main push/PR 時に `cargo check`。`tauri.conf.json` で `bundle.active: true` + `bundle.createUpdaterArtifacts: true`。Secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- アップデートチェック・セルフアップデート: 実装済み。`tauri-plugin-updater` v2 を使用。`tauri.conf.json` に `plugins.updater`（pubkey + endpoint）を設定。Rust 側: `.plugin(tauri_plugin_updater::Builder::new().build())` で登録、`setup()` 内で 3 秒遅延後に非同期更新チェック → 更新あれば `update-available` イベントを emit。`install_update` コマンドで `download_and_install()` → `app.restart()`。UI 側: main ウィンドウで `update-available` イベントを listen → info 色の更新バナー（バージョン表示 + 「更新して再起動」ボタン）を表示。i18n 対応（`update_available`/`update_install`/`update_installing`）。`capabilities/default.json` に `updater:default` パーミッション追加。Ed25519 署名鍵ペアで検証。初回リリース手順: `cargo tauri signer generate` で鍵生成 → 公開鍵を `tauri.conf.json` に設定 → 秘密鍵を GitHub Secrets に設定。
- クロスプラットフォーム: macOS/Linux 動作確認。russh は純 Rust のため OS 固有の SSH バイナリ依存なし。CI でマルチプラットフォームビルドを追加。
