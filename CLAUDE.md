# muSQL project notes (for agents)

## Overview
Windows向け MySQL クライアント。Tauri v2 + Rust backend + 静的 UI（`ui/` の HTML/JS/CSS、Node.js 不要）。SSH 踏み台対応（`russh`）。

## How to run
- `cd src-tauri && cargo tauri dev` で起動。
- `cargo check` / `cargo test` / `cargo fmt --check` / `cargo clippy -- -D warnings`。

## Architecture
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・Data/Schema/SQL タブ・AI アシスト）。タブはドラッグで並び替え可。全タブ状態（SQL 内容・テーブルタブ・表示順・アクティブタブ）を localStorage に永続化。
- `src-tauri/src/main.rs` に全 Rust ロジック集約。
- `src-tauri/src/docker/` に Docker 連携モジュール（`discovery.rs`, `tunnel.rs`）。
- ウィンドウは `tauri.conf.json` で事前定義、show/hide パターンで管理。

## Key behaviors
- **パスワード / API キー**: `keyring`（Windows Credential Manager）で保存。各パスワード（MySQL / SSH パスフレーズ / SSH パスワード）は `save_*` フラグで keyring 保存 or 都度入力を選択可能。都度入力の場合、query 画面で接続前にモーダルプロンプトを表示。
- **接続プール**: `ConnectionCache`（`Arc<Mutex>`）で Pool + SshTunnel をキャッシュ。fingerprint 一致で再利用。
- **クエリキャンセル**: `RUNNING_QUERIES` でタブ単位に `KILL QUERY`。
- **SSH**: `russh` v0.57。認証方式は公開鍵（`key`）またはパスワード（`password`）を選択可能。公開鍵: 指定鍵 → agent → デフォルト鍵。パスワード: `session.authenticate_password()`。タイムアウト 8 秒。ssh config 参照時は認証方式を公開鍵に固定。
- **AI アシスト**: チャット形式モーダル。ユーザーが自然言語でプロンプト → `ai_assist` コマンドで SQL 生成。スキーマは `SCHEMA_CACHE` でキャッシュ。Claude / OpenAI / Gemini 対応。チャット履歴は DB 毎に localStorage で保持（最大 50 件）。生成 SQL はコピー / エディタ挿入可。
- **メニュー**: ハンバーガーボタン → `popup_menu()`。アクセラレータは非表示メニューバーで保持。
- **アップデート**: `tauri-plugin-updater` + Ed25519 署名。手動チェック。Cargo feature `self-updater`（デフォルト有効）で分離。`--no-default-features` で Store ビルド（アップデータ無効）。
- **Docker 連携**: `bollard` クレートで Docker API に接続（名前付きパイプ → TCP `127.0.0.1:2375/2376` フォールバック、WSL2 dockerd 対応）。running コンテナから MySQL コンテナを自動検出（exposed port 3306 or `musql.enable=true` ラベル）。ports バインドなしのコンテナには `alpine/socat` 一時コンテナで TCP トンネルを作成（`auto_remove: true`、ラベル `musql.tunnel=true`）。トンネルコンテナは検出一覧から除外。アプリ起動時・終了時・query ウィンドウ close 時にトンネルをクリーンアップ。資格情報（user/password/ssl_mode）はコンテナ毎に localStorage で保持。Cargo feature `docker`（デフォルト有効）で `bollard`/`futures-util` 依存を分離。
- **SQL 識別子**: JS `quoteId()` / Rust `escape_identifier()` でバッククォートエスケープ。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`
- `musql:ai-chat:<profileId>:<database>` — AI アシストのチャット履歴
- `musql:docker-creds` — Docker コンテナ毎の資格情報（user/password/ssl_mode）
- `musql:docker-last-cred` — 最後に使った Docker 資格情報（新規コンテナのデフォルト値）

## Limits
- 結果行 500 件、ページング 100 件/ページ、SSH タイムアウト 8 秒。

## Build variants
- **Standalone（デフォルト）**: `cargo tauri build` — NSIS インストーラ + セルフアップデータ付き。
- **Store**: `cargo tauri build --config tauri.store.conf.json -- --no-default-features --features docker` — アップデータ無効・Docker有効。Store 用 EXE を生成。
- **Store dev 確認**: `cargo tauri dev --config src-tauri/tauri.store.conf.json -- --no-default-features --features docker`（要 Developer Command Prompt / RC.EXE in PATH）。
- Store 用アイコン: `src-tauri/icons/Square44x44Logo.png`, `Square150x150Logo.png`, `StoreLogo.png`。
- CI: `release.yml` の `build-store` ジョブが Store EXE をリリースにアップロード。

## Roadmap

### Phase 4.5: Store リリース前の優先改修（完了）
- ✅ SSHサーバーへのパスワード認証サポート（`SshConfig.auth_method`: `key` | `password`）
- ✅ パスワード・パスフレーズの接続時都度入力サポート（`save_password` / `save_ssh_passphrase` / `save_ssh_password` フラグ + query 画面のプロンプトモーダル）

### Phase 4.6: Docker MySQL コンテナ自動検出・接続（完了）
- ✅ `bollard` クレートで Docker API に接続（名前付きパイプ + TCP フォールバックで WSL2 dockerd 対応）
- ✅ running コンテナから MySQL コンテナを自動検出（exposed port 3306 or `musql.enable=true` ラベル）
- ✅ `alpine/socat` 一時コンテナによる TCP トンネル（ports バインドなしのコンテナ対応）
- ✅ メイン画面に Docker ボタン + コンテナ一覧モーダル + 資格情報入力モーダル（SSL モード選択付き）
- ✅ 資格情報のコンテナ毎 localStorage 保持（最後に使った設定を新規コンテナに適用）
- ✅ トンネルクリーンアップ（起動時・終了時・query close 時）
- ✅ `musql.*` ラベルカスタマイズ（`musql.name`, `musql.user`, `musql.password`, `musql.port`）
- ✅ Cargo feature `docker`（デフォルト有効）で `bollard`/`futures-util` 依存を分離

### Phase 5: Windows Store 配布
- MSIX パッケージング: `store/AppxManifest.xml` + `store/build-msix.ps1` で未署名 MSIX を生成。CI (`release.yml` の `build-store`) がタグ push 時に EXE + MSIX をリリースにアップロード。
- Partner Center Identity: `CommunitylinksInc.muSQL` / `CN=38A0E012-12F7-45AE-8FAA-50937924F823`。
- 残タスク: Partner Center へ MSIX を提出 → Microsoft 自動署名 → Store 公開。

### Phase 6: マルチDB 抽象化 + PostgreSQL / MariaDB
最初のマルチ DB 対応。抽象化レイヤーを構築し、PostgreSQL を追加する。MariaDB は MySQL 互換のためラベル追加のみ。

**6a. DB 抽象化レイヤー構築**
- Rust: DB 方言 trait を導入（接続・クエリ実行・スキーマ取得・識別子エスケープ・キャンセル・型変換）
- JS: 方言差の吸収（識別子引用符、ページング構文、文字列関数、スキーマ問い合わせ）
- 設定画面: DB Engine ドロップダウンを有効化、エンジン別のデフォルト値（ポート等）切替
- AI プロンプトのエンジン名動的化（「MySQL query assistant」→ エンジン名に応じて変更）
- MySQL 固有の主な箇所（約40箇所）:
  - Rust: `mysql` クレート / `OptsBuilder` / `Value` 型変換 / `KILL QUERY` / `escape_identifier`（バッククォート）/ INFORMATION_SCHEMA クエリ / `USE db`
  - JS: `SHOW DATABASES` / `SHOW TABLES` / `DESCRIBE` / `SHOW INDEX` / `quoteId`（バッククォート）/ `CHAR_LENGTH`・`CONCAT`・`LEFT` / `LIMIT offset, n` / BLOB/TEXT 型名ハードコード

**6b. PostgreSQL 対応**
- クレート: `tokio-postgres` または `sqlx`
- 方言差: 識別子は `""`、ページングは `LIMIT n OFFSET m`、スキーマは `information_schema`（カラム名差異あり）、`SHOW DATABASES` → `SELECT datname FROM pg_database`、`SHOW TABLES` → `pg_tables`、`DESCRIBE` → `information_schema.columns`、`SHOW INDEX` → `pg_indexes`、キャンセルは `pg_cancel_backend(pid)`、型名は `bytea` / `text` / `varchar` 等
- デフォルトポート: 5432、デフォルトユーザー: `postgres`

**6c. MariaDB 対応**
- MySQL ワイヤープロトコル互換のため、既存の `mysql` クレートでそのまま接続可能
- 設定画面の DB Engine ドロップダウンに選択肢を追加するのみ（コード変更は実質なし）

### Phase 7: SQLite / Redshift
Phase 6 の抽象化レイヤーの上に追加。

**7a. SQLite**
- クレート: `rusqlite`（ファイルベース、ネットワーク不要）
- 接続設定: ホスト/ポートではなくファイルパス指定（設定画面の動的フィールド切替が必要）
- SSH 不要、SSL 不要
- 方言差: `SHOW DATABASES` → 概念なし（単一ファイル＝単一DB）、`SHOW TABLES` → `sqlite_master`、`DESCRIBE` → `PRAGMA table_info()`、`SHOW INDEX` → `PRAGMA index_list()` + `PRAGMA index_info()`、型は動的型付け
- DB 選択モーダルをスキップ、テーブル一覧を直接表示

**7b. Redshift**
- PostgreSQL ワイヤープロトコル互換のため、Phase 6 の PostgreSQL 対応がベース
- `tokio-postgres` でほぼ接続可能
- 追加考慮: `DISTKEY` / `SORTKEY` 等の Redshift 固有スキーマ情報表示、`STL_QUERY` 等のシステムテーブル
- デフォルトポート: 5439

### Phase 8: Redis
SQL ではない KVS だが、既存 UI 部品の流用で実用的な GUI を構築可能。

**クレート**: `redis`（Rust、非同期対応）

**UI 設計:**
- サイドバー: Key Browser（パターンフィルタで `SCAN`、型別グルーピング: string / hash / list / set / zset / stream）
- メインペイン（キータブ）: 型別に既存の結果テーブルを流用
  - string → 値表示（JSON なら自動整形）
  - hash → フィールド / 値 の2カラムテーブル
  - list → インデックス / 値 の2カラムテーブル
  - set → 値の1カラムテーブル
  - zset → スコア / 値 の2カラムテーブル
  - stream → ID / フィールド群のテーブル
- CLI タブ: CodeMirror エディタを流用し Redis コマンドを入力・実行（SQL タブと同じ位置づけ）
- 各キータブのヘッダーに型・TTL・メモリ使用量を表示
- SSH トンネル・AI アシスト（コマンド生成）・コマンド履歴は既存資産を流用

**既存資産の流用:**
- 結果テーブル（ページング・ソート・行詳細）→ hash/list/set/zset 値表示
- JSON 自動整形 → string 値表示
- CodeMirror エディタ → CLI タブ
- 履歴機能 → コマンド履歴
- SSH トンネル → Redis サーバーへの踏み台接続

**新規実装:**
- Rust: `redis` クレート接続・コマンド実行・型判定
- JS: Key Browser（SCAN ベース遅延読み込み）、型別表示の分岐

### Phase 9: その他機能拡充
- TablePlus の設定引き継ぎ
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）
- ✅ tbls 互換スキーマ Markdown 出力（Structure ビューのフッター + Data ビューのエクスポートメニューから tbls 互換 Markdown をエクスポート）

### Phase X: 優先改修事項
- 全項目完了済み