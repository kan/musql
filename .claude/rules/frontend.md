# UI 実装ルール（`ui/`）

静的 UI（HTML/JS/CSS、Node.js 不要、ビルド工程なし）を触るときに読む。lint は `just lint-ui`（Biome、`ui/lib/**` の vendor は除外）。

## 画面構成
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・Data/Schema/SQL タブ・AI アシスト）。タブはドラッグで並び替え可。全タブ状態（SQL 内容・テーブルタブ・表示順・アクティブタブ）を localStorage に永続化。
- ウィンドウは `tauri.conf.json` で事前定義、show/hide パターンで管理。

## 機能ごとの実装メモ
- **AI アシスト**: チャット形式モーダル。ユーザーが自然言語でプロンプト → `ai_assist` コマンドで SQL 生成。スキーマは `SCHEMA_CACHE` でキャッシュ。Claude / OpenAI / Gemini 対応。チャット履歴は DB 毎に localStorage で保持（最大 50 件）。生成 SQL はコピー / エディタ挿入可。
- **QuickOpen（Ctrl+P）**: query 画面のコマンドパレット（`query.js` の `openQuickOpen`）。先頭文字でモード切替: 無印=テーブル名 fuzzy → Data タブを開く / `@`=開いているタブ切替 / `>`=SQL 履歴（`musql:history`）検索 → アクティブ SQL エディタに挿入（SQL タブが無ければ新規タブ）/ `?`=ヘルプ。テーブル一覧は `currentTables` キャッシュを使用（DB 切替で更新）。fuzzy は subsequence スコア（`fuzzyScore`）。Ctrl+P は capture リスナーで横取り（ブラウザ印刷抑止）。
- **クエリ完了通知**: 5秒超のクエリが**非フォーカス時**に完了（成功/失敗問わず）で通知（`query.js` `maybeNotifyQueryDone`）。`tauri-plugin-notification` 優先（`window.__TAURI__.notification`）→ Web Notification フォールバック。ON/OFF は query View メニュー `query:toggle-notify`、`musql:notify-query`（既定 ON）、capabilities `notification:default`。**Windows 制約**: dev は AUMID 未登録でタイトルが起動元プロセス名（インストール版は muSQL 表示）／WebView2 は Web の `onclick` 非配送・notify_rust も desktop クリックコールバック無しのため**クリックでのフォーカス/タブ切替は非対応**（表示のみ）。
- **アプリ内マニュアル（#46）**: `ui/manual.js` が `docs/manual/*.md` を raw.githubusercontent.com から fetch し、自前の簡易 Markdown レンダラ（DOM 構築・innerHTML 不使用）でモーダル表示。バンドルしない（main ブランチが正）。入口は F1（全ウィンドウ）/ main ヘルプメニュー「マニュアル」（`main:manual` → `menu:action` "manual"）/ 各画面の `?` ボタン（`data-manual="page.md#anchor"`、動的モーダルは `window.createHelpButton()`）。見出し ID は GitHub slug 互換（`makeSlugger`）。外部リンクは Rust `open_external`（https 限定）。CSP で raw.githubusercontent.com を connect-src / img-src に許可。マニュアルの見出しを変えたら `data-manual` のアンカーも追従すること。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`, `musql:ai:op-ref:<provider>`
- `musql:ai-chat:<profileId>:<database>` — AI アシストのチャット履歴
- `musql:docker-creds` — Docker コンテナ毎の資格情報（user/ssl_mode のみ。パスワードは keyring `docker:{containerId}` に保存）
- `musql:docker-last-cred` — 最後に使った Docker 資格情報（user/ssl_mode のみ。パスワードは keyring `docker:_last` に保存）
- `musql:export:encoding`, `musql:export:newline` — エクスポートの文字コード / 改行コード設定（既定 utf-8 / lf）
- `musql:notify-query` — 長時間クエリ完了のデスクトップ通知 ON/OFF（既定 ON、`"0"` で OFF）

## Limits
- 結果行 500 件、ページング 100 件/ページ、SSH タイムアウト 8 秒。
