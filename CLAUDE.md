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
- **アップデート**: `tauri-plugin-updater` + Ed25519 署名。手動チェック。Cargo feature `self-updater`（デフォルト有効）で分離。`--no-default-features` で Store ビルド（アップデータ無効）。
- **SQL 識別子**: JS `quoteId()` / Rust `escape_identifier()` でバッククォートエスケープ。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`, `musql:ai:enabled`

## Limits
- 結果行 500 件、ページング 100 件/ページ、SSH タイムアウト 8 秒。

## Build variants
- **Standalone（デフォルト）**: `cargo tauri build` — NSIS インストーラ + セルフアップデータ付き。
- **Store**: `cargo tauri build --config tauri.store.conf.json -- --no-default-features` — アップデータ無効。Store 用 EXE を生成。
- **Store dev 確認**: `cargo tauri dev --config src-tauri/tauri.store.conf.json -- --no-default-features`（要 Developer Command Prompt / RC.EXE in PATH）。
- Store 用アイコン: `src-tauri/icons/Square44x44Logo.png`, `Square150x150Logo.png`, `StoreLogo.png`。
- CI: `release.yml` の `build-store` ジョブが Store EXE をリリースにアップロード。

## Roadmap

### Phase 5: Windows Store 配布
- MSIX パッケージング: `store/AppxManifest.xml` + `store/build-msix.ps1` で未署名 MSIX を生成。CI (`release.yml` の `build-store`) がタグ push 時に EXE + MSIX をリリースにアップロード。
- Partner Center Identity: `CommunitylinksInc.muSQL` / `CN=38A0E012-12F7-45AE-8FAA-50937924F823`。
- 残タスク: Partner Center へ MSIX を提出 → Microsoft 自動署名 → Store 公開。

### Phase 6: 機能拡充
- Docker MySQL 簡単アクセス（`docker ps` でコンテナ検出、ポートマッピング自動入力）。
- TablePlusの設定引き継ぎ
- クロスプラットフォーム対応（macOS/Linux ビルド・CI マトリクス化）。

### Phase X: 優先改修事項
- mainウインドウ
  - タイトルの上下のマージンが大きすぎる
  - 言語設定は初回起動時に環境情報から取得して決定
  - モードと言語のUIは消す(メニューから変更のみ)
  - リストの下にもマージンを設ける
  - インポート/エクスポートのUIボタンは消す
  - ハンバーガーメニューボタンのサイズを他と揃える
  - connectionsが1つも無い時はナビゲーション表示
- settingウインドウ
  - タイトル自体なくす
  - 縮めるとカードが重なるUIが気持ち悪いので修正
  - カードの高さをfixにして、はみ出す時はスクロール
  - 3つのカードを1つにまとめる
  - フッターにある操作ボタンは上の右にもってくる
  - タグの補完のz-indexを上げる
  - MySQLカードは「DB設定」にする
  - DBエンジンのプルダウンを追加(とりあえずMySQL固定)
  - 「SSH踏み台」カードを「SSH経由」に変えて、これ自体を有効化のチェックボックスにする
  - ssh configの読み込みがバグっているので確認
  - configを読み込んだ内容を変更不可にする
- queryウインドウ
  - ソートは全体のソートにする
  - テーブルはタブいっぱいに表示する
  - テーブルは交互に行の色を変える
  - 「カラム切詰」→ 「短縮表示」に
  - 構造タブでは下にインデックス情報を表示
  - データと構造は同一タブ内で切り替え(ボタンは右端に置く)
  - SQLタブの操作ボタンを小さくする
  - カーソルの居る行をハイライトする
  - SQL実行結果もタブ一杯に表示
  - 結果件数は左寄せ。右寄せで実行時間を表示
  - SQL入力欄をリサイズ可能に
  - SQL1,2,3の単位でSQLを保存して、開き直した時に復元する
  - SQLタブの追加ボタンは左側へ
  - 履歴プルダウンを選んだらSQL全体をポップアップ
  - 履歴を選択したら新しいSQLタブを開く
  - AI補完のAPI呼び出しをプログレス表示 & 結果サマリーも表示