# Changelog

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
