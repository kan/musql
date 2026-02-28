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

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`

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

### Phase 4: AI クエリ補完

SQL エディタ上でコメント（`-- ユーザーごとの売上合計` 等）を書くと、Copilot のようにインラインでクエリ候補をゴースト表示し、Tab で確定する機能。

#### 仕組み
- **スキーマ収集**: 接続中 DB の `INFORMATION_SCHEMA`（テーブル名・カラム名・型・PK/FK）を Rust 側で取得しキャッシュ。DB 切替時にリフレッシュ。
- **プロンプト構築**: スキーマ情報 + エディタの前後コンテキスト（カーソル前 N 行）を組み合わせて LLM に送信。
- **AI バックエンド**: ユーザーが API キーを設定画面で入力。対応モデル: OpenAI API / Anthropic API（将来的にローカル LLM も検討）。API キーは `keyring` で安全保存。
- **UI**: CodeMirror 上でゴーストテキスト（薄いグレー）として候補を表示。Tab で挿入、Esc で破棄。入力停止後のデバウンス（500ms 程度）でリクエスト。
- **Tauri コマンド**: `ai_complete(context, schema)` → Rust 側で API 呼び出し → 補完テキストを返却。
- **設定**: プロファイル単位ではなくアプリグローバル設定（API プロバイダ・モデル・API キー）。Settings 画面または専用の AI 設定画面。

### Phase 5: 配布拡充
- Windows Store (MSIX) での配布。Tauri の MSIX バンドル対応、署名、ストア申請。
- Docker MySQL 簡単アクセス（`docker ps` でコンテナ検出、ポートマッピング自動入力）。
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）。
