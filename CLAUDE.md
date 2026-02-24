# muSQL project notes (for agents)

## Overview
- Windows向けの MySQL クライアント。Tauri v2 + Rust backend + 静的 UI。
- SSH 踏み台経由の MySQL 接続に対応。
- UI は `ui/` の素朴な HTML/JS/CSS。Node.js 不要。

## How to run (dev)
- 前提: Rust toolchain, Windows OpenSSH Client (`C:\Windows\System32\OpenSSH\ssh.exe`)。
- 起動:
  - `cd src-tauri`
  - `cargo tauri dev`
- cargo は PATH に入っていないため、シェルから実行する際は `export PATH="$PATH:/c/Users/kanfu/.cargo/bin"` を先に実行すること。
- `cargo check` でコンパイル確認。

## Architecture — マルチウィンドウ構成
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト・接続。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・データ/スキーマ表示・SQL 実行）。
- ウィンドウは `tauri.conf.json` で `visible: false` として事前定義。
- Rust 側の `open_*_window` で `show()` + `set_focus()`、閉じるときは `hide_window` で非表示にする（`close()` で破棄しない）。
- `on_window_event` で非 main ウィンドウの X ボタンも hide に変換。

## Main flow
- `ui/app.js` → Tauri `invoke`:
  - `open_settings_window` / `open_query_window` でサブウィンドウを表示。
  - `list_profiles` / `save_profile` / `delete_profile` でプロファイル CRUD。
- `ui/settings.js` → `test_connection` で接続テスト、`open_query_window` で接続（クエリウィンドウを開く）、`hide_window` でウィンドウを隠す。
- `ui/query.js` → DB エクスプローラ:
  - DB 選択モーダル → サイドバーにテーブル一覧 → タブで Data/Schema/SQL 表示。
  - SQL タブは CodeMirror 5 エディタ（MySQL シンタックスハイライト、キーワード＋テーブル名補完）。
  - 実行時エラーは `near '...' at line N` をパースしてエディタ上に赤波線でマーク。
  - `run_query` でクエリ実行（接続は Pool キャッシュ、DB 切替は `USE` で実行）。
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
- `ui/lib/codemirror/` — CodeMirror 5 vendored ファイル（コア、SQL モード、補完アドオン）。

## Behavior notes
- `test_connection` は `SELECT 1` を実行（per-request 接続、プールは使わない）。
- `run_query` は任意 SQL を実行して列名＋行＋ `affected_rows` を返却。`max_rows` パラメータで行数制限（デフォルト 500、0 で無制限）。
- 接続プール: `ConnectionCache` で Pool + SshTunnel をキャッシュ。fingerprint（ホスト/ポート/ユーザー/SSL/SSH 設定）が同じなら再利用。DB は Pool opts に含めず `USE` で切替。
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

## Limits / defaults
- 結果行はデフォルト最大 500 件（`max_rows` で変更可能、エクスポート時は無制限）。
- データタブのページングはデフォルト 100 件/ページ（50/100/200/500 から選択可能）。
- SSH トンネル確立待ちタイムアウトは 8 秒。

## TODO
(上から優先度順)

- 設定のパスワードを安全に保存
- SQL実行の多重クリック抑止
- 見た目の強化
  - ダークモード対応
  - i18n対応
  - アイコンを適宜使用
  - faviconをちゃんと作る
- 接続設定のインポートとエクスポート
- docker上のmysqlへの簡単アクセス
- ビルド・配布手段の検討
- Windows以外での動作

## localStorage keys
- `musql:collapsed`: グループの開閉状態（`app.js`）。
- `musql:drafts:<profileId>`: SQL タブのエディタ内容（`string[]`、タブ順）。
- `musql:history:<profileId>`: 実行済み SQL（`{ sql, ts }[]`、新しい順、最大 100 件）。

## Strategy
- パスワード安全保存: OS のキーチェーン（Windows Credential Manager）を利用。Tauri プラグインまたは `keyring` クレートで profile ID をキーにしてパスワードを保存。`connections.json` にはパスワードを含めない。
- SQL 多重クリック抑止: 実行ボタンを実行中 disabled にし、完了/エラー後に復帰。
- 接続設定インポート/エクスポート: JSON 形式でファイル書き出し/読み込み。`pick_file` / `export_file` を再利用。パスワードを含めるかはオプション。
- Docker MySQL: `docker ps` でコンテナ一覧を取得し、MySQL コンテナを検出。ポートマッピングから接続先を自動入力。
- ダークモード: CSS 変数を light/dark で切り替え。`prefers-color-scheme` メディアクエリ + 手動切替トグル。
