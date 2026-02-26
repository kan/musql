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
- **クエリキャンセル**: `RUNNING_QUERY` グローバルに connection_id + Pool 保持 → `KILL QUERY`。
- **SSH**: `russh` v0.48。`channel_open_direct_tcpip` + `copy_bidirectional`。認証: 指定鍵 → agent → デフォルト鍵。タイムアウト 8 秒。
- **メニュー**: ハンバーガーボタン → `show_popup_menu` で毎回構築 → `popup_menu()`。アクセラレータは非表示メニューバーで保持。
- **アップデート**: Help メニューから手動チェック。`tauri-plugin-updater` + Ed25519 署名。
- **インポート/エクスポート**: 重複検出付き 2 段階コール。パスワードはオプション。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`

## Limits
- 結果行: デフォルト 500 件。ページング: 100 件/ページ。SSH タイムアウト: 8 秒。

## Roadmap (優先度順)

### Phase 1: セキュリティ修正 (Critical/High)

#### 1-1. innerHTML XSS 修正
- **箇所**: `ui/query.js` (DB名一覧 L567, テーブル名一覧 L612), `ui/app.js` (タグチップ L254)
- **問題**: DB由来/ユーザー入力文字列を `innerHTML` で直接差し込み。`withGlobalTauri: true` のため Tauri API 呼び出し可能な XSS に繋がる。
- **対応**: アイコンは別 `span` ノードで追加し、テキスト部分は `textContent` のみ使用。

#### 1-2. SQL 識別子エスケープ統一
- **箇所**: `ui/query.js` (L166, L170, L844, L891), `src-tauri/src/main.rs` (`USE \`{}\`` L886)
- **問題**: バッククォートを含む DB/テーブル名で構文が壊れる。意図しないクエリ実行の余地。
- **対応**: 識別子エスケープ関数を JS/Rust 双方に用意（`` ` → `` ``）し全箇所に適用。

#### 1-3. SSH ホスト鍵検証（known_hosts / TOFU）
- **箇所**: `src-tauri/src/main.rs` `check_server_key` (L149) が常に `Ok(true)`。
- **問題**: MITM 攻撃に弱く SSH トンネルの安全性が実質ゼロ。
- **対応**: `~/.ssh/known_hosts` 読み込み + TOFU（初回接続時に保存、次回以降照合）。不一致時は UI にダイアログ表示。

### Phase 2: 品質改善 (Medium)

#### 2-1. クエリキャンセル状態のタブ単位管理
- **箇所**: `RUNNING_QUERY` グローバル 1 本（`main.rs` L163）。
- **問題**: 複数 SQL タブ同時実行時に別クエリを KILL QUERY する可能性。
- **対応**: タブ ID を `run_query` / `cancel_query` に渡し、`HashMap<String, RunningQuery>` で管理。

#### 2-2. import_profiles 内の `std::thread::sleep` 除去
- **箇所**: `main.rs` L1169, L1210（インポートループ内で 2ms sleep）。
- **問題**: async コンテキストをブロックし UI 応答性に悪影響。
- **対応**: ID 生成を `uuid` クレートに置換し sleep 依存を削除。

#### 2-3. パスワード明示削除機能
- **箇所**: `main.rs` L1270 付近（空文字は「既存維持」扱い）。
- **問題**: 保存済みパスワードをユーザーが消せない。
- **対応**: Settings UI に「保存済みパスワードを削除」チェックボックスまたはボタンを追加。`save_profile` に `clear_password` フラグを新設。

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

### Phase 4: 機能追加
- Docker MySQL 簡単アクセス（`docker ps` でコンテナ検出、ポートマッピング自動入力）。
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）。
