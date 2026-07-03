# マニュアル撮影用フィクスチャ

`docs/manual/img/README.md` の撮影リストを撮るための環境一式。デバッグビルド（`cargo tauri dev --config tauri.dev.conf.json`）での撮影を想定している。

## 構成

| ファイル | 内容 |
|---------|------|
| `docker-compose.yml` | 撮影用 MySQL 8.0（`127.0.0.1:3306`、コンテナ名 `musql-demo-mysql`） |
| `init/01-ec_demo.sql` | サンプル DB `ec_demo`（架空 EC サイト、8 テーブル + シードデータ） |
| `init/02-analytics.sql` | サンプル DB `analytics`（DB 切替の画面用、2 テーブル） |
| `connections.debug.json` | 架空の接続プロファイル一覧（グループ 2 つ + 色/タグ付き 6 件。`profile-list-colored.png` 等） |
| `connections.simple.json` | シンプル版（グループ 1 つ + 3 件。`screen-layout.png` 用） |
| `setup.ps1` | 上記 JSON をデバッグビルドの設定ディレクトリに配置（`-Variant full`（既定）/ `-Variant simple` で切替） |

## 使い方

1. MySQL コンテナを起動（Docker は WSL2 内の dockerd を想定。初回起動時に init SQL が流れる）:

   ```powershell
   wsl docker compose -f docs/manual/fixtures/docker-compose.yml up -d
   ```

2. 架空プロファイルを配置（既存の connections.json はタイムスタンプ付きで退避される）:

   ```powershell
   powershell -ExecutionPolicy Bypass -File docs/manual/fixtures/setup.ps1
   # screen-layout.png を撮るときはシンプル版に切替
   powershell -ExecutionPolicy Bypass -File docs/manual/fixtures/setup.ps1 -Variant simple
   ```

3. デバッグビルドを起動:

   ```powershell
   cd src-tauri
   cargo tauri dev --config tauri.dev.conf.json
   ```

4. 「ローカル開発 (Docker)」プロファイルを設定画面で開き、パスワード `demo1234` を入力して保存（パスワードは keyring 保存のため JSON では配布できない）。以降はワンクリックで接続できる。

## 接続情報

| 項目 | 値 |
|------|----|
| ホスト / ポート | `127.0.0.1:3306` |
| ユーザー / パスワード | `demo` / `demo1234`（root は `root-demo`） |
| DB | `ec_demo`（メイン）, `analytics` |

「ローカル開発 (Docker)」以外のプロファイル（EC 本番・ステージングなど）は撮影用の架空ホストで、接続はできない。EC 本番プロファイルは SSH 踏み台設定入りなので `ssh-settings.png` の撮影に使える。

Docker 検出モーダル（`docker-list.png`）用に、コンテナへ `musql.name` / `musql.user` / `musql.password` ラベルを付けてある。開発中の他の MySQL コンテナも一覧に写り込むため、撮影時は不要なコンテナを止めるか写り込みに注意。

## 片付け

```powershell
wsl docker compose -f docs/manual/fixtures/docker-compose.yml down -v
```

プロファイルを消す場合は `%APPDATA%\jp.co.communitylinks.musql.debug\connections.json` を削除（setup.ps1 が作った `.bak-*` から復元も可）。
