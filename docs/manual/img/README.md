# マニュアル用スクリーンショット 撮影リスト

各ページで参照している画像の一覧です。ここに同名の PNG を置くと、マニュアルに反映されます。撮影時のシチュエーションを添えています（各ページの画像直後の HTML コメントにも同じメモがあります）。

> 推奨: ライトテーマ・ウィンドウは既定サイズ・サンプルデータ（本番情報は写さない）で撮影してください。

| ファイル名 | 使用ページ | 撮影シチュエーション |
|-----------|-----------|--------------------|
| `overview.png` | README | query 画面全体。左にテーブル一覧、中央に Data タブ（結果テーブルが数十行）、上部にタブが 2〜3 枚 |
| `screen-layout.png` | getting-started | メイン画面。プロファイル数件・グループ 1 つ、右上に Docker ボタンとハンバーガーメニュー。全体が入るように |
| `profile-edit.png` | getting-started / connections | 設定ウィンドウ。ホスト/ポート/ユーザー/パスワード/DB、SSL プルダウン、SSH セクション、下部に「接続テスト」「保存」。サンプル値入り |
| `profile-list-colored.png` | connections | メイン画面。グループ 1〜2 個、プロファイルに色（赤=本番, 緑=検証 など）とタグ |
| `ssh-settings.png` | ssh | 設定ウィンドウの SSH セクション。踏み台ホスト/ポート/ユーザー、認証方式（公開鍵/パスワード）切替、鍵パス、config 参照 |
| `docker-list.png` | docker | メイン画面で Docker ボタンを押した状態。検出コンテナ一覧＋資格情報入力（user/password/SSL）モーダル |
| `query-layout.png` | query | query 画面全体。左サイドバー（DB 名＋テーブル一覧）、上部タブバー、中央に結果テーブル |
| `query-data.png` | query | テーブルの Data タブ。結果テーブル数十行、下部にページング（前へ/次へ・50/100/200/500・件数） |
| `query-sql.png` | query | SQL タブ。上部にエディタ（SELECT 文）、下部に結果テーブル、実行/全実行/整形ボタン |
| `quickopen.png` | query | Ctrl+P で開いた QuickOpen。入力欄＋候補リスト。テーブル名で絞り込んだ状態 |
| `ai-settings.png` | ai-assist | AI 設定モーダル。プロバイダ（Claude/OpenAI/Gemini）とモデルのプルダウン、API キー入力欄 |
| `ai-chat.png` | ai-assist | AI アシストのチャット。自然言語プロンプトと生成 SQL（コピー/エディタ挿入ボタン付き） |
| `export-options.png` | export | エクスポートで形式選択後のオプションダイアログ。文字コード（UTF-8/BOM/Shift_JIS/EUC-JP）・改行（LF/CRLF）、保存/コピー/キャンセル |
| `sync-settings.png` | sync | メニュー →「接続の同期…」のモーダル。同期ファイルパス入力＋参照、今すぐ読み込む/書き出す/保存 |
