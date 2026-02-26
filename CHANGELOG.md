# Changelog

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

[0.2.0]: https://github.com/kan/musql/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kan/musql/releases/tag/v0.1.0
