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
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・データ/スキーマ表示・SQL 実行）。
- ウィンドウは `tauri.conf.json` で `visible: false` として事前定義。
- Rust 側の `open_*_window` で `show()` + `set_focus()`、閉じるときは `hide_window` で非表示にする（`close()` で破棄しない）。
- `on_window_event` で非 main ウィンドウの X ボタンも hide に変換。

## Main flow
- `ui/app.js` → Tauri `invoke`:
  - `open_settings_window` / `open_query_window` でサブウィンドウを表示。
  - `list_profiles` / `save_profile` / `delete_profile` でプロファイル CRUD。
- `ui/settings.js` → `test_connection` で接続テスト、`hide_window` でウィンドウを隠す。
- `ui/query.js` → DB エクスプローラ:
  - DB 選択モーダル → サイドバーにテーブル一覧 → タブで Data/Schema/SQL 表示。
  - `run_query` でクエリ実行（接続は Pool キャッシュ、DB 切替は `USE` で実行）。
  - `export_file` でネイティブ保存ダイアログ経由のファイル書き出し（CSV/TSV/SQL）。
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

## Behavior notes
- `test_connection` は `SELECT 1` を実行（per-request 接続、プールは使わない）。
- `run_query` は任意 SQL を実行して列名＋行＋ `affected_rows` を返却。`max_rows` パラメータで行数制限（デフォルト 500、0 で無制限）。
- 接続プール: `ConnectionCache` で Pool + SshTunnel をキャッシュ。fingerprint（ホスト/ポート/ユーザー/SSH 設定）が同じなら再利用。DB は Pool opts に含めず `USE` で切替。
- `export_file` は `rfd` クレートでネイティブ保存ダイアログを表示しファイル書き出し。
- `ssh.private_key_path` は SSH の `IdentityFile` として `-o IdentityFile=<path> -o IdentitiesOnly=yes` で渡す。`.pub` ファイル指定で 1Password SSH agent と連携可能。
- SSH バイナリは `C:\Windows\System32\OpenSSH\ssh.exe` を優先（Git 付属の ssh.exe は 1Password SSH agent 非対応のため）。
- SSH トンネル失敗時は stderr の内容をエラーメッセージに含めて返す。

## Limits / defaults
- 結果行はデフォルト最大 500 件（`max_rows` で変更可能、エクスポート時は無制限）。
- データタブのページングはデフォルト 100 件/ページ（50/100/200/500 から選択可能）。
- SSH トンネル確立待ちタイムアウトは 8 秒。

## TODO
(上から優先度順)

- SQL を簡易的な補完と色付きで入力したい。クエリ履歴の記録・呼び出し機能も欲しい
- ssh_config の alias を使えるようにしたい
- TLS 接続時の CA 証明書を指定して検証もできるようにする

## Strategy
- SQL 補完/色付け: 軽量エディタを導入して色付け。補完はキーワードのみ → 必要なら接続メタデータから拡張。履歴はローカルストレージまたはファイルに保存。
- ssh_config: まずは `ssh -F <config> <alias>` を許可する UI/実装。必要なら config 解析へ拡張。
- TLS CA: UI に CA パス入力（ファイルピッカー）。`mysql::SslOpts::with_root_cert_path` で検証対応。
