# UI 実装ルール（`ui/`）

静的 UI（HTML/JS/CSS、Node.js 不要、ビルド工程なし）を触るときに従う。lint は `just lint-ui`（Biome、`ui/lib/**` の vendor は除外）。

## 画面構成
- **main** (`ui/index.html`, `ui/app.js`): 接続プロファイル一覧。
- **settings** (`ui/settings.html`, `ui/settings.js`): プロファイル編集・接続テスト。
- **query** (`ui/query.html`, `ui/query.js`): DB エクスプローラ（テーブル一覧・Data/Schema/SQL タブ・AI アシスト）。タブはドラッグで並び替え可。全タブ状態（SQL 内容・テーブルタブ・表示順・アクティブタブ）を localStorage に永続化。
- **redis** (`ui/redis.html`, `ui/redis.js`、#98): Redis のプロファイルを開く画面。左にキーの一覧（`redis_scan`、パターンで絞り込み）、右に CLI タブとキーごとのタブ。接続は Rust が 1 本だけ持つ（`redis_connect`）。CLI の危険なコマンドの判定と現在の DB 番号は Rust が持ち（`rust.md` の「Redis」）、画面は `redis_command` が返す `confirm` を見て確認し、`db` を表示に反映するだけにする（`applyDb`。左上の DB の切り替え（`openDbMenu`）は `redis_databases` で一覧を出し、選んだ DB を `redis_select` で切り替える）。キーの一覧はパターン欄の入力に合わせて読み直し、ワイルドカードの無い文字列は部分一致（`patternFor`）にする。CLI の補完とヘルプは、サーバーの `COMMAND DOCS` から作る（`redis_command_docs`。取れないサーバーでは補完が出ないだけで、CLI は動く）。`menu:action` は `getCurrentWindow().listen` で受ける（素の `listen` は他のウィンドウ宛ても受けるため）。
- ウィンドウは `tauri.conf.json` で事前定義、show/hide パターンで管理。
- **`style.css` / `app.js` / `query.js` / `settings.js` / `redis.js` / `result-table.js` / `credential-prompt.js` / `export.js` / `context-menu.js` を変えたら、それを読み込む HTML の `?v=N` を 1 つ上げる**（`<link href="./style.css?v=N">` など。`style.css` は 4 つの HTML すべて）。キャッシュされた古いファイルが使われ続けるのを避けるための番号で、上げ忘れても lint もテストも落ちない。`?v=` の付いていない共有スクリプト（`i18n.js` / `icons.js` / `theme.js` / `manual.js` / `about.js` / `op-picker.js` / `error-log.js`）は対象外。
- **結果テーブル**（`renderTable` / `renderPagedTable` / `showRowDetailModal`）は `ui/result-table.js`、**資格情報の入力**（`promptForCredentials`）は `ui/credential-prompt.js` にあり、query 画面と Redis 画面が共有する。どちらも `i18n.js`（と `icons.js`）の後に読み込む。資格情報の入力のモーダルの HTML（`#pw-prompt-modal` ほか）は、使う画面の側に置く。ウィンドウを閉じるときは `cancelCredentialPrompt` で入力を取り消す（残すと、次に開いたときにその入力が前のプロファイルの接続を始めてしまう）。**エクスポート**（`generateCsv` / `openExportDialog` / `exportRows` / `currentRowsExportItems`）は `ui/export.js` にあり、query 画面と Redis 画面が共有する。画面は `currentRowsExportItems` で表示中の行の項目（CSV / TSV）を得て、自分の項目を後ろに足す。テーブル全体や SQL のエクスポートは MySQL に固有なので `query.js` に残す。**コンテキストメニュー**（`showContextMenu` / `hideContextMenu`）は `ui/context-menu.js` にあり、main / query / Redis の 3 画面が共有する。要素（`#context-menu`）は各画面が持つ。クリックと Escape で隠すリスナーは `context-menu.js` が登録するので、画面側で足さないこと。メニューを開くボタンは、そのクリックが document へ届かないように止める。
- **ウィンドウタイトルを変えるときは `safeInvoke("set_window_title", { title })` を呼ぶ。** JS の `setTitle` は使わない（dev 版の `[DEBUG]` の目印を Rust 側で付けるため。capabilities から `core:window:allow-set-title` を外してあるので、呼ぶと権限エラーになる。#120）。

## 機能ごとの実装メモ
- **AI アシスト**: チャット形式モーダル。ユーザーが自然言語でプロンプト → `ai_assist` コマンドで SQL 生成。スキーマは `SCHEMA_CACHE` でキャッシュ。Claude / OpenAI / Gemini 対応。チャット履歴は DB 毎に localStorage で保持（最大 50 件）。生成 SQL はコピー / エディタ挿入可。
- **QuickOpen（Ctrl+P）**: query 画面のコマンドパレット（`query.js` の `openQuickOpen`）。先頭文字でモード切替: 無印=テーブル名 fuzzy → Data タブを開く / `@`=開いているタブ切替 / `>`=SQL 履歴（`musql:history`）検索 → アクティブ SQL エディタに挿入（SQL タブが無ければ新規タブ）/ `?`=ヘルプ。テーブル一覧は `currentTables` キャッシュを使用（DB 切替で更新）。fuzzy は subsequence スコア（`fuzzyScore`）。Ctrl+P は capture リスナーで横取り（ブラウザ印刷抑止）。
- **クエリ完了通知**: 5秒超のクエリが**非フォーカス時**に完了（成功/失敗問わず）で通知（`query.js` `maybeNotifyQueryDone`）。ON/OFF は query View メニュー `query:toggle-notify`、`musql:notify-query`（既定 ON）。通知は 3 段で試す: (1) Rust の `toast_notify` コマンド（`src-tauri/src/toast.rs`、#113。**クリックで戻れる**）→ (2) `tauri-plugin-notification`（`window.__TAURI__.notification`、capabilities `notification:default`）→ (3) Web Notification。(2) と (3) は表示のみで、クリックは受け取れない（WebView2 は Web の `onclick` を配送せず、notify_rust にも desktop のクリックコールバックが無い）。(1) が `false` を返すのは Store 版（パッケージ化されたアプリはショートカットの AUMID を使えない）で、一度返ったら以後は呼ばない。reject は「今回は出せなかった」場合（ショートカットが無い、通知用スレッドが止まっている、など）で、`console.warn` を出して (2) へ進む。`toast_notify` は実際に通知を出し終えてから結果を返すので、出せなかった通知が黙って消えることは無い。**クリックの流れ**: `toast_notify` にトークン（`セッション番号:タブ ID`）を渡す → バナーのクリックで Rust が query ウィンドウを前面に出し、`toast:activated` イベントでトークンをそのまま返す → セッション番号が今と同じなら `tabManager.activate`。**セッション番号（`notifySession`）を外さないこと**: タブ ID（`sql-1` など）は使い回されるので、番号が無いと、前の接続や前の DB の通知が、今の同名タブを選んでしまう。番号は `tabManager.removeAll` で進む（ウィンドウを閉じたときと DB を切り替えたときの両方を拾うため。`resetExplorer` に置くと DB 切替が漏れる）。トークンに入れるのは**クエリを始めた時点の番号**で、完了時に番号が変わっていたら通知自体を出さない（実行中に閉じられたクエリは、失敗として完了する）。query ウィンドウが閉じられて（hidden）いたら、Rust 側が何もしない。タブが閉じられていても何もしない。**通知センターへ移った後のクリックは対象外**（理由は `toast.rs` の冒頭と `rust.md`）。
- **アプリ内マニュアル（#46）**: `ui/manual.js` が `docs/manual/*.md` を raw.githubusercontent.com から fetch し、自前の簡易 Markdown レンダラ（DOM 構築・innerHTML 不使用）でモーダル表示。バンドルしない（main ブランチが正）。入口は F1（全ウィンドウ）/ main ヘルプメニュー「マニュアル」（`main:manual` → `menu:action` "manual"）/ 各画面の `?` ボタン（`data-manual="page.md#anchor"`、動的モーダルは `window.createHelpButton()`）。見出し ID は GitHub slug 互換（`makeSlugger`）。外部リンクは Rust `open_external`（https 限定）。CSP で raw.githubusercontent.com を connect-src / img-src に許可。マニュアルの見出しを変えたら `data-manual` のアンカーも追従すること。

- **「muSQL について」とライセンス一覧（#119）**: `ui/about.js`（main 画面だけが読み込む）。入口は main ヘルプメニュー「muSQL について」（`main:about` → `menu:action` "about" → `window.openAbout()`）。版は `window.__TAURI__.app.getVersion()`、一覧は `ui/credits.json` を初めて開いたときに `fetch` する（約 0.9MB で、起動時には読まない。CSP の `connect-src` に `'self'` があるのはこのため）。各項目の本文は開いたときに DOM を作る。**`ui/credits.json` を手で編集しないこと**（`just credits` が生成する。仕組みは `testing.md`）。**`ui/lib/` のライブラリを追加、更新したとき、または `ui/icons.js` に Lucide のアイコンを足したときは、`src-tauri/src/credits.rs` の `VENDORED` と `ui/lib/` の `LICENSE` ファイルも直し、`just credits` を回す**（vendor の分は手で管理している。テストが検出するのは、項目やライセンスファイルの欠け、CodeMirror の版の食い違い、作り直し忘れまで。sql-formatter と Lucide の版が古いことは検出しない）。
- **JS エラーのログ（#110）**: `ui/error-log.js` が未捕捉のエラー（`error` / `unhandledrejection`）を `log_frontend` コマンドでログファイルへ送る。すべての画面の HTML で**最初の `<script>`** として読み込む（後続スクリプトのエラーを拾うため）。同じエラーは 60 秒に 1 回へ間引く。**書くのは `Error` オブジェクトだけ**で、それ以外の値（`invoke` が reject する Rust の `Err` 文字列など）は型名しか書かない。`catch` を 1 つ書き忘れても SQL がログへ出ないようにするためなので、この制限を外さないこと。**`catch` 済みの失敗を `log_frontend` へ流さないこと**（クエリのエラー文には SQL の一部が入る。ログに書いてよいものの基準は `rust.md`「秘密情報」）。

## localStorage keys
- `musql:collapsed`, `musql:drafts:<profileId>`, `musql:history:<profileId>`, `musql:theme`, `musql:lang`
- `musql:ai:provider`, `musql:ai:model`, `musql:ai:op-ref:<provider>`
- `musql:ai-chat:<profileId>:<database>` — AI アシストのチャット履歴
- `musql:docker-creds` — Docker コンテナ毎の資格情報（user/ssl_mode のみ。パスワードは keyring `docker:{containerId}` に保存）
- `musql:docker-last-cred` — 最後に使った Docker 資格情報（user/ssl_mode のみ。パスワードは keyring `docker:__last__` に保存）。MySQL のコンテナの分で、Redis のコンテナの分は `musql:docker-last-cred:redis` に分けて持つ（MySQL の root のパスワードを Redis の初期値にしないため）。**Redis はパスワードを引き継がない**（多くのコンテナはパスワード無しで動き、そこへパスワードを送ると接続が失敗するため。コンテナごとのパスワードは保存する）
- `musql:export:encoding`, `musql:export:newline` — エクスポートの文字コード / 改行コード設定（既定 utf-8 / lf）
- `musql:notify-query` — 長時間クエリ完了のデスクトップ通知 ON/OFF（既定 ON、`"0"` で OFF）
- `musql:redis-history:<profileId>` — Redis の CLI タブのコマンド履歴（最大 100 件。パスワードを含みうる行は `SECRET_WORDS` で除く）

## Limits
- 結果行 500 件、ページング 100 件/ページ、SSH タイムアウト 8 秒。
