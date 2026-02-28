# muSQL project notes (for agents)

## Overview
- Windows向け MySQL クライアント。Tauri v2 + Rust backend + 静的 UI（`ui/` の素朴な HTML/JS/CSS、Node.js 不要）。
- SSH 踏み台経由の MySQL 接続に対応（`russh` 純 Rust 実装）。

## How to run
- `cd src-tauri && cargo tauri dev` で起動。
- `cargo check` でコンパイル確認、`cargo test` でユニットテスト。
- 新規環境セットアップは `README.md` 参照。

## Architecture
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・Data/Schema/SQL タブ）。
- ウィンドウは `tauri.conf.json` で事前定義、show/hide パターンで管理（close しない）。
- `src-tauri/src/main.rs` に全 Rust ロジック集約（接続・クエリ・SSH・プール・プロファイル CRUD・メニュー・エクスポート）。

## Key files
- `src-tauri/src/main.rs` — Rust バックエンド全体。
- `src-tauri/tauri.conf.json` — Tauri 設定・ウィンドウ定義。
- `src-tauri/capabilities/default.json` — Tauri パーミッション。
- `ui/{index,settings,query}.html` + `ui/{app,settings,query}.js` — 各画面。
- `ui/style.css` — 共通スタイル。
- `ui/icons.js` — Lucide SVG アイコン + `icon()` ヘルパー。
- `ui/theme.js` — ダークモード（cross-window 同期）。
- `ui/i18n.js` — 日英 i18n（インライン翻訳データ、`t(key, params)`）。
- `ui/lib/codemirror/` — CodeMirror 5 vendored。
- `ui/lib/sql-formatter/` — sql-formatter vendored UMD。
- `.github/workflows/release.yml` — リリース（`v*` タグ → NSIS → GitHub Release）。
- `.github/workflows/ci.yml` — CI（fmt / clippy / test / audit）。
- `CHANGELOG.md` — リリースノート。

## Key behaviors
- **パスワード**: `keyring`（Windows Credential Manager）で保存。`connections.json` には非保存。
- **接続プール**: `ConnectionCache`（`Arc<Mutex>`）で Pool + SshTunnel をキャッシュ。fingerprint 一致で再利用。DB は `USE` で切替。
- **クエリ実行**: `run_query`（async + `spawn_blocking`）。`max_rows` デフォルト 500。
- **クエリキャンセル**: `RUNNING_QUERIES`（`HashMap<String, RunningQueryEntry>`）でタブ単位に connection_id + Pool 保持 → `KILL QUERY`。`tab_id` で対象タブを特定。
- **パスワード削除**: `save_profile` で `clear_password` / `clear_ssh_passphrase` フラグにより keyring から明示削除。Settings UI に × ボタン。
- **SSH**: `russh` v0.50。`channel_open_direct_tcpip` + `copy_bidirectional`。認証: 指定鍵 → agent → デフォルト鍵。タイムアウト 8 秒。ホスト鍵検証: `~/.ssh/known_hosts` ベース TOFU。
- **SQL 識別子エスケープ**: JS `quoteId()` / Rust `escape_identifier()` でバッククォートをエスケープ。
- **メニュー**: ハンバーガーボタン → `show_popup_menu` で毎回構築 → `popup_menu()`。アクセラレータは非表示メニューバーで保持。
- **アップデート**: Help メニューから手動チェック。`tauri-plugin-updater` + Ed25519 署名。
- **インポート/エクスポート**: 重複検出付き 2 段階コール。パスワードはオプション。
- **AI 補完**: 入力停止後 500ms デバウンスで `ai_complete` → ゴーストテキスト表示（Tab 確定 / Esc 破棄）。スキーマは `SCHEMA_CACHE` でキャッシュ。API キーは `keyring` 保存。Claude / OpenAI / Gemini 対応。チェックボックスで ON/OFF 切替。補完確定後の再問い合わせ抑制（`aiJustAccepted`）、空結果・エラー時のリトライ抑制（`aiSuppressed`、次の入力でリセット）。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`, `musql:ai:enabled` — AI 補完設定（グローバル）

## Limits
- 結果行: デフォルト 500 件。ページング: 100 件/ページ。SSH タイムアウト: 8 秒。

## Roadmap (優先度順)

### Phase 2: 品質改善 ✔ 完了
- 2-1. タブ単位クエリキャンセル（`RUNNING_QUERIES` HashMap）
- 2-2. import_profiles の sleep 除去（atomic counter による ID 生成）
- 2-3. パスワード明示削除（Settings UI × ボタン + `clear_password` フラグ）

### Phase 3: CI/テスト強化 ✔ 完了
- 3-1. CI ワークフロー強化（`cargo fmt --check` / `cargo clippy -- -D warnings` / `cargo test` + `cargo audit` ジョブ）
- 3-2. GitHub Actions SHA ピニング（`ci.yml` / `release.yml`）
- 3-3. `.github/dependabot.yml`（cargo + github-actions 週次更新）
- 3-4. ユニットテスト導入（`escape_identifier` / `parse_ssh_config_host` / `connection_fingerprint` / `generate_profile_id`）
- 3-5. clippy 全警告修正 + `rust-version` 1.77→1.80

### Phase 4: AI クエリ補完 ✔ 完了
- 4-1. スキーマキャッシュ（`SCHEMA_CACHE` / `INFORMATION_SCHEMA` 取得 / テーブル数 100 上限）
- 4-2. AI API 呼び出し（Claude / OpenAI / Gemini 3 プロバイダ対応 / `reqwest`）
- 4-3. `ai_complete` コマンド（プロンプト構築 + API 呼び出し + スキーマキャッシュ統合）
- 4-4. AI API キー管理（`keyring` 保存 / `save_ai_api_key` / `has_ai_api_key`）
- 4-5. AI 設定モーダル（プロバイダ・モデル・API キー / View メニューから開く）
- 4-6. ゴーストテキスト UI（CodeMirror `setBookmark` + widget / Tab 確定 / Esc 破棄 / 500ms デバウンス）
- 4-7. ユニットテスト（`build_ai_prompt` / `ai_keyring_key` / `AiProvider` serde）

### Phase 5: 配布拡充
- Windows Store (MSIX) での配布。Tauri の MSIX バンドル対応、署名、ストア申請。
- Docker MySQL 簡単アクセス（`docker ps` でコンテナ検出、ポートマッピング自動入力）。
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）。
