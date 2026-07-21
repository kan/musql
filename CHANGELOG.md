# Changelog

## [0.7.1] - 2026-07-21

### Fixed
- 接続の同期で、古いバージョンの muSQL が新しいバージョンの書いた同期ファイルを上書きし、新バージョンが増やした設定項目を削ってしまう問題（#81）。同期ファイルに書き込んだアプリのバージョンを記録し、自分より新しいバージョンで書かれていた場合は書き込みをスキップする（この保護が働くのは双方が本機能を含むバージョンのときなので、同期するマシンは揃えて更新することを推奨）

## [0.7.0] - 2026-07-20

### Added
- 1Password 連携（#80）。DB パスワード / SSH パスフレーズ / SSH パスワード / AI API キーを 1Password のシークレット参照（`op://vault/item/field`）から取得できる。取得した値は Windows 資格情報マネージャーに保存され、以降はそこから読まれるため 1Password CLI が呼ばれるのはマシン毎に 1 回だけ
- 1Password のアイテム / フィールドを一覧から選ぶ参照ピッカー（#80）。参照を手で貼り付ける必要がなく、フィールドを選ぶと値の取得まで自動で実行される。設定ウィンドウと AI 設定モーダルの両方から利用可能

### Changed
- 接続の同期で、SSH 秘密鍵と SSL の CA 証明書の**ファイルパスを同期対象から除外**した（#80）。マシンごとに置き場所が異なるため。同期ファイル経由で他マシンのパスがローカル設定を上書きすることはなくなる
- 依存関係を更新（tokio 1.53.0 / russh 0.62.2 / bollard 0.21.0 / reqwest 0.13.4 ほか）

### Fixed
- 接続の同期で、別マシンの SSH 秘密鍵 / CA 証明書のパスがローカルの設定を上書きしてしまう問題（#80）
- 同期ファイルを手で編集することで、保存済みパスワードの削除フラグ（`clear_*`）を注入できた問題（#80）
- `tauri-action` v1.0.0 の破壊的変更により `latest.json` がリリースに添付されず、セルフアップデータが動作しなくなる問題

### Security
- Dependabot alert の解消（serde_with 3.21.0 / GHSA-7gcf-g7xr-8hxj、cmov 0.5.4 / GHSA-3rjw-m598-pq24）

## [0.6.0] - 2026-07-03

### Added
- ユーザーマニュアル（`docs/manual/`、機能別・日本語）とアプリ内マニュアルビューア（`F1` / ヘルプメニュー / 各画面の `?` ボタンから該当ページへ直接ジャンプ）(#46)
- 接続プロファイルの外部 JSON 同期（Dropbox 等のパス指定で複数マシン共有）(#47)
- QuickOpen コマンドパレット（`Ctrl+P`: テーブル名 fuzzy / `@` タブ切替 / `>` SQL 履歴）(#44)
- 長時間クエリ（5 秒超）完了のデスクトップ通知（非フォーカス時、View メニューで ON/OFF）(#43)
- エクスポートの文字コード（UTF-8 / BOM 付き / Shift_JIS / EUC-JP）・改行コード選択とクリップボードコピー (#40)
- SQL タブのクエリ結果ページングと全行エクスポート (#41)
- query タブのコンテキストメニュー（閉じる / 他を閉じる / 右側を閉じる / すべて閉じる）(#42)
- ウィンドウ状態（サイズ・位置・最大化）の保存 / 復元（dev とインストール版は identifier で分離）(#42)
- SECURITY.md（脆弱性報告ポリシー）(#28)

### Changed
- AI アシストのモデルリストを現行モデルに更新（Claude Sonnet 5 / Haiku 4.5 / Opus 4.8、GPT-5.4 / 5.4 Mini / 5.4 Nano / 5.5、Gemini 3.5 Flash / 3.1 Flash-Lite / 3.1 Pro）(#71)
- 削除確認などのダイアログをカスタム UI 化（Enter / Escape 操作対応）(#42)
- `F5` / `Ctrl+R` による WebView リロードを抑止 (#42)
- CI: Biome による UI JavaScript の lint を導入 (#45)、cargo audit を週次実行化

### Fixed
- AI 設定モーダルが AI アシストモーダルの背後に開いてしまう問題 (#71)
- URL オープンを `cmd /c start` 経由から `tauri-plugin-opener`（ShellExecuteW）に変更（コマンドインジェクション対策）
- セキュリティ: 依存関係を更新（russh 0.61.2 / openssl 0.10.81 / tauri 2.11.5 ほか GHSA / RUSTSEC 対応）

## [0.5.5] - 2026-04-25

### Changed
- tokio 1.50.0 → 1.51.1
- russh 0.58.1 → 0.60.0（aws-lc-rs 脆弱性修正・rekey 後のチャネル EOF/CLOSE replay 修正含む）
- tauri-plugin-updater 2.10.0 → 2.10.1（updater パッケージのファイル拡張子保持）
- CI: softprops/action-gh-release 2.6.1 → 3.0.0（Node 24 対応）

### Fixed
- セキュリティ: openssl 0.10.75 → 0.10.78（`Deriver::derive` バッファオーバーフロー / `digest_final` OOB write / AES key wrap 境界チェック / PSK・cookie トランポリンの未チェックコールバック長 / PEM パスワードコールバックの OOB read）
- セキュリティ: rustls-webpki 0.103.10 → 0.103.13（不正な CRL BIT STRING での DoS panic / name constraint 検証）
- セキュリティ: rand 0.8.5 → 0.8.6、0.10.0 → 0.10.1（カスタムロガー使用時の unsoundness）
- clippy `collapsible_match` 警告を解消（Rust 1.95 で強化された lint）

## [0.5.4] - 2026-03-30

### Added
- テーブルタブにリロードボタンを追加（ページング・ソートリセット付き）
- プライバシーポリシーを追加

### Changed
- 接続一覧のホスト表示を改善（SSH config alias 対応・localhost 省略）
- Partner Center の新アカウントに合わせて MSIX Identity を更新
- CI: dtolnay/rust-toolchain を更新（Rust 1.94.1 パッチリリース対応）

## [0.5.3] - 2026-03-27

### Changed
- 接続一覧のタグ表示をプロファイル名の右に移動し、3行→2行のコンパクト表示に変更
- mysql 27 → 28（MariaDB Parsec プラグイン認証サポート追加）
- russh 0.57.1 → 0.58.1（スループット約21%改善、Windows メモリロック修正、libcrux-sha3 脆弱性修正）
- bollard 0.20.1 → 0.20.2
- CI: tauri-action 0.6.1 → 0.6.2、rust-cache 2.8.2 → 2.9.1、action-gh-release 2.5.0 → 2.6.1

## [0.5.2] - 2026-03-09

### Added
- 起動時にバックグラウンドでアップデートチェックを実行（3秒遅延、更新がある場合はバナーで通知） (#12)
- SQL エディタで `;` 区切りのステートメント単位ハイライト（カーソル位置のステートメント全行を薄青背景で表示） (#11)

### Changed
- bollard 0.18 → 0.20（Docker API クレート、`query_parameters` / `ContainerCreateBody` への移行含む）
- tauri 2.10.2 → 2.10.3
- tokio 1.49.0 → 1.50.0

## [0.5.1] - 2026-03-06

### Changed
- 設定画面の色選択をカラードットからカスタムドロップダウンに変更（色付き■スウォッチ付き、プロファイル名と横並び配置）

## [0.5.0] - 2026-03-04

### Added
- Docker MySQL コンテナ自動検出・ワンクリック接続
  - Docker 上の running MySQL コンテナを自動検出（exposed port 3306 or `musql.enable=true` ラベル）
  - ports バインドなしのコンテナに `alpine/socat` 一時コンテナで TCP トンネルを自動作成
  - Docker Desktop / WSL2 dockerd 対応（名前付きパイプ + TCP フォールバック）
  - 接続時の SSL モード選択（DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY）
  - 資格情報（user / password / SSL モード）をコンテナ毎に保持
  - `musql.*` ラベルによるカスタマイズ（表示名・user・password・port）
  - アプリ起動時・終了時・切断時にトンネルコンテナを自動クリーンアップ
- Cargo feature `docker`（デフォルト有効）で Docker 関連依存を分離

## [0.4.5] - 2026-03-04

### Added
- SSH パスワード認証サポート（公開鍵認証に加え、パスワード認証を選択可能に）
- パスワード都度入力オプション（MySQL パスワード・SSH パスフレーズ・SSH パスワードそれぞれに「キーリングに保存」チェックボックスを追加）
- 接続時のクレデンシャル入力モーダル（keyring 未保存のパスワードを接続前にプロンプト）

### Changed
- SSH 設定画面に認証方式プルダウン（公開鍵 / パスワード）を追加
- ssh config 参照時は認証方式を公開鍵に固定
- ラベル変更: IdentityFile → 秘密鍵、鍵 → 公開鍵

## [0.4.1] - 2026-03-04

### Changed
- russh を 0.57.1 に更新（デッドロック修正・keepalive 修正）
- reqwest を 0.13 に更新

### Other
- README にバッジ追加（CI・Rust・Tauri・Windows）

## [0.4.0] - 2026-03-04

### Added
- AI アシスト（チャット形式モーダル）
  - 自然言語でプロンプト → AI が SQL を生成
  - チャット履歴を DB 毎に保持（最大 50 件）
  - 生成 SQL のコピー / エディタ挿入ボタン
  - Claude / ChatGPT / Gemini 対応（旧ゴーストテキスト補完を置換）
- タブのドラッグ並び替え（マウスイベントベース）
- テーブルタブ（Data/Structure）の永続化・復元
- SQL タブ番号の再利用（閉じた番号を再割当）と SQL 内容の復元
- CodeMirror アクティブ行ハイライト
- 接続ボタンでプロファイルを自動保存してから接続（新規プロファイルでも即接続可能）

### Changed
- 設定画面を 1 カード化、操作ボタンを上部に配置
- メイン画面の空状態ナビ・マージン調整
- アプリ identifier を `jp.co.communitylinks.musql` に変更

## [0.3.1] - 2026-03-02

### Added
- Windows Store ビルドバリアント（`self-updater` feature gate で分離）
- MSIX パッケージングパイプライン（Microsoft Store 提出用）

## [0.3.0] - 2026-02-28

### Added
- AI クエリ補完（Claude / ChatGPT / Gemini 対応）
  - 入力停止後にインラインでSQL候補をゴースト表示（Tab で確定、Esc で破棄）
  - スキーマ情報を自動取得しプロンプトに含めて精度向上
  - API キーは Windows Credential Manager で安全保存
  - チェックボックスで機能の ON/OFF 切替
  - AI 設定モーダル（プロバイダ・モデル・API キー）

### Fixed
- 接続一覧が多い場合にウィンドウ全体がスクロールする問題を修正（リスト内スクロールに変更）
- スクロールバーをスリムなデザインに統一

## [0.2.0] - 2026-02-26

### Changed
- アップデートチェックを起動時の自動実行からメニュー手動実行に変更
  - Help メニューに「アップデートを確認...」を追加
  - 更新がない場合は「最新バージョンです。」と表示

## [0.1.0] - 2025-06-15

### Added
- MySQL 接続（ホスト・ポート・ユーザー・パスワード・データベース選択）
- SSH 踏み台接続（russh 純 Rust 実装、SSH Agent / ~/.ssh/config 対応）
- SSH 秘密鍵パスフレーズの keyring 保存
- SSL 接続（DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY）
- 接続プール（同一設定の自動再利用）
- SQL エディタ（CodeMirror 5、シンタックスハイライト、補完、エラー箇所表示）
- マルチ SQL タブ
- SQL 整形（sql-formatter）
- クエリキャンセル（KILL QUERY）
- 行詳細モーダル（JSON 自動整形）
- カラムソート（ASC / DESC / なし 3 ステート）
- BLOB/TEXT 切り詰め表示（トグル可能）
- データエクスポート（CSV / TSV / SQL）
- パスワード安全保存（Windows Credential Manager）
- プロファイル管理（グループ・色・タグ・ドラッグ＆ドロップ）
- プロファイルのインポート/エクスポート（JSON、重複検出付き）
- ダークモード（システム追従 + 手動切替）
- 日英切替（i18n）
- ネイティブメニュー（ハンバーガーメニュー + キーボードショートカット）
- アプリ内自動更新（NSIS インストーラー + Ed25519 署名）
- GitHub Actions CI/CD（cargo check + リリースビルド）

[0.7.1]: https://github.com/kan/musql/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/kan/musql/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/kan/musql/compare/v0.5.5...v0.6.0
[0.5.5]: https://github.com/kan/musql/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/kan/musql/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/kan/musql/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/kan/musql/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/kan/musql/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/kan/musql/compare/v0.4.5...v0.5.0
[0.4.5]: https://github.com/kan/musql/compare/v0.4.1...v0.4.5
[0.4.1]: https://github.com/kan/musql/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/kan/musql/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/kan/musql/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/kan/musql/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kan/musql/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kan/musql/releases/tag/v0.1.0
