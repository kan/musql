# muSQL project notes (for agents)

## Overview
Windows向け MySQL クライアント。Tauri v2 + Rust backend + 静的 UI（`ui/` の HTML/JS/CSS、Node.js 不要）。SSH 踏み台対応（`russh`）。

## How to run
- `cd src-tauri && cargo tauri dev` で起動。
- `cargo check` / `cargo test` / `cargo fmt --check` / `cargo clippy -- -D warnings`。

## Architecture
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・Data/Schema/SQL タブ・AI 補完）。
- `src-tauri/src/main.rs` に全 Rust ロジック集約。
- ウィンドウは `tauri.conf.json` で事前定義、show/hide パターンで管理。

## Key behaviors
- **パスワード / API キー**: `keyring`（Windows Credential Manager）で保存。
- **接続プール**: `ConnectionCache`（`Arc<Mutex>`）で Pool + SshTunnel をキャッシュ。fingerprint 一致で再利用。
- **クエリキャンセル**: `RUNNING_QUERIES` でタブ単位に `KILL QUERY`。
- **SSH**: `russh` v0.50。認証: 指定鍵 → agent → デフォルト鍵。タイムアウト 8 秒。
- **AI 補完**: 500ms デバウンス → `ai_complete` → ゴーストテキスト（Tab 確定 / Esc 破棄）。スキーマは `SCHEMA_CACHE` でキャッシュ。Claude / OpenAI / Gemini 対応。ON/OFF チェックボックス。確定後・空結果・エラー時はリトライ抑制。
- **メニュー**: ハンバーガーボタン → `popup_menu()`。アクセラレータは非表示メニューバーで保持。
- **アップデート**: `tauri-plugin-updater` + Ed25519 署名。手動チェック。
- **SQL 識別子**: JS `quoteId()` / Rust `escape_identifier()` でバッククォートエスケープ。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`, `musql:ai:enabled`

## Limits
- 結果行 500 件、ページング 100 件/ページ、SSH タイムアウト 8 秒。

## Roadmap

### Phase 5: Windows Store 配布

Tauri v2 は MSIX ネイティブ未対応。`cargo tauri build` で EXE を生成後、MakeAppx.exe で手動 MSIX パッケージング。Store 提出時に Microsoft が署名するため自前証明書不要。

#### 5-1. Cargo feature でセルフアップデータを分離
- `Cargo.toml`: `tauri-plugin-updater` を `optional = true` に変更。`[features] default = ["self-updater"]`, `self-updater = ["tauri-plugin-updater"]` 追加。
- `main.rs`: アップデータ関連コードを `#[cfg(feature = "self-updater")]` で囲む。
  - `tauri_plugin_updater::Builder` プラグイン登録 (L2417)
  - `check_update` / `install_update` コマンド (L2374–2413)
  - Help メニュー「アップデートを確認...」項目 (L2202–2208)
  - メニューイベント `"main:check-update"` ハンドラ (L2431–2458)
- Store ビルド用にスタブコマンドを `#[cfg(not(feature = "self-updater"))]` で定義（`check_update` → `Ok(false)`, `install_update` → `Ok(())`）。JS 側は変更不要。
- `capabilities/default.json` の `"updater:default"` は未ロード時に無視されるためそのまま。

#### 5-2. Store ビルド用 Tauri 設定オーバーレイ
- `src-tauri/tauri.store.conf.json` を新規作成:
  ```json
  {
    "bundle": {
      "createUpdaterArtifacts": false,
      "windows": { "webviewInstallMode": { "type": "offlineInstaller" } }
    },
    "plugins": {}
  }
  ```
- ビルド: `cargo tauri build --config tauri.store.conf.json -- --no-default-features`

#### 5-3. MSIX パッケージング
- `src-tauri/icons/` に Store 用アイコン追加: `Square44x44Logo.png`, `Square150x150Logo.png`, `StoreLogo.png`（`tauri icon` で生成 or 手動リサイズ）。
- `AppxManifest.xml` テンプレート作成（Identity/Publisher は Partner Center 登録後に確定）。
- `MakeAppx.exe pack /d msix-staging /p muSQL_{version}_x64.msix` で MSIX 生成。

#### 5-4. CI: release.yml に Store ビルドジョブ追加
- 既存の `build` ジョブ（NSIS standalone）はそのまま。
- `build-store` ジョブを追加: `--no-default-features` でビルド → MakeAppx.exe で MSIX 生成 → Release にアップロード。

#### 5-5. Partner Center 登録・申請
- 開発者アカウント登録（個人 ~$19）。
- アプリ名 "muSQL" を予約 → Identity Name / Publisher を取得 → AppxManifest.xml に反映。
- MSIX を提出 → Microsoft の自動認定テスト通過後に公開。

#### 対象ファイル
| ファイル | 変更内容 |
|---------|---------|
| `src-tauri/Cargo.toml` | `self-updater` feature 追加、`tauri-plugin-updater` を optional 化 |
| `src-tauri/src/main.rs` | `#[cfg(feature)]` でアップデータ関連コード分岐、スタブコマンド追加 |
| `src-tauri/tauri.store.conf.json` | 新規: Store ビルド用設定オーバーレイ |
| `src-tauri/icons/` | Store 用アイコン追加 |
| `.github/workflows/release.yml` | `build-store` ジョブ追加 |

### Phase 6: 機能拡充
- Docker MySQL 簡単アクセス（`docker ps` でコンテナ検出、ポートマッピング自動入力）。
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）。
