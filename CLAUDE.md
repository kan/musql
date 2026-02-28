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
- `.github/workflows/ci.yml` — CI（cargo check）。
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

### Phase 3: CI/テスト強化

#### 3-1. CI ワークフロー強化 + セキュリティ設定
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` 追加。
- `cargo audit` ジョブ追加（`rustsec/audit-check` または `cargo install cargo-audit && cargo audit`）。
- `.github/dependabot.yml` 作成（cargo + github-actions の自動更新）。
- `permissions: contents: read` 明示。
- GitHub Actions SHA pinning（`release.yml` / `ci.yml` の actions を commit SHA に固定）:
  - `actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5` (v4.3.1)
  - `dtolnay/rust-toolchain@efa25f7f19611383d5b0ccf2d1c8914531636bf9` (stable, Rust 1.93.1)
  - `swatinem/rust-cache@779680da715d629ac1d338a641029a2f4372abb5` (v2.8.2)
  - `tauri-apps/tauri-action@73fb865345c54760d875b94642314f8c0c894afa` (v0.6.1)
  - `actions-rust-lang/audit@72c09e02f132669d52284a3323acdb503cfc1a24` (v1.2.7)
- その他推奨セキュリティ設定: `SECURITY.md` 作成（脆弱性報告窓口）、CodeQL 検討。

#### 3-2. ユニットテスト導入
- **純ロジックテスト** (`cargo test` で常時実行):
  - `resolve_ssh_config_host` — テスト用文字列からのパース検証。
  - `ml()` — 全キーの日英翻訳が key そのままにフォールバックしないことを検証。
  - SQL 識別子エスケープ関数（Phase 1-2 で新規作成）— バッククォート・空文字・Unicode。
  - `connection_fingerprint` — 同一設定で一致、異なる設定で不一致。
- **MySQL 結合テスト** (`#[ignore]` 付き、CI で `cargo test -- --ignored` 実行):
  - ローカルに `root:root@127.0.0.1:3306` の mysqld がある前提。
  - CI では `ubuntu-latest` + MySQL サービスコンテナで実行。
  - `build_opts` → `Pool::new` → 接続・クエリ実行・DB 切替・`row_to_json` 検証。
- **テスト対象外**: SSH トンネル、keyring 操作、Tauri ウィンドウ操作。
- **方針**: `#[cfg(test)] mod tests` を `main.rs` 末尾に追加。テスト対象の関数は必要に応じてヘルパーとして切り出し。

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
