<p align="center">
  <img src="ui/icon.svg" width="96" height="96" alt="muSQL" />
</p>

<h1 align="center">muSQL</h1>

<p align="center">
  Windows 向け MySQL クライアント — Tauri v2 + Rust
</p>

<p align="center">
  <a href="https://github.com/kan/musql/releases/latest"><img src="https://img.shields.io/github/v/release/kan/musql?style=flat-square" alt="Release" /></a>
  <a href="https://github.com/kan/musql/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/kan/musql/ci.yml?branch=main&style=flat-square&label=CI" alt="CI" /></a>
  <img src="https://img.shields.io/badge/Rust-1.80+-f74c00?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Tauri-v2-24c8db?style=flat-square&logo=tauri&logoColor=white" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/platform-Windows-0078d4?style=flat-square&logo=windows&logoColor=white" alt="Windows" />
  <a href="LICENSE"><img src="https://img.shields.io/github/license/kan/musql?style=flat-square" alt="License" /></a>
</p>

---

<!-- TODO: スクリーンショットを追加 -->

## ダウンロード

<a href="https://apps.microsoft.com/detail/9mvgmmcf47gj?hl=ja-JP&gl=JP">
  <img src="https://get.microsoft.com/images/ja%20dark.svg" width="200" alt="Microsoft Store から入手" />
</a>

**[Microsoft Store](https://apps.microsoft.com/detail/9mvgmmcf47gj?hl=ja-JP&gl=JP) からのインストールを推奨します。** Microsoft による署名・自動更新・クリーンアンインストールに対応しています。

代替手段として [GitHub Releases](https://github.com/kan/musql/releases/latest) から `.exe` インストーラーを直接ダウンロードすることも可能です（こちらはアプリ内自動更新機能付き）。

> **Note:** GitHub Releases 版は未署名のため Windows SmartScreen の警告が表示されます。「詳細情報」→「実行」で起動できます。Store 版ではこの警告は表示されません。

## 特徴

- **SSH 踏み台対応** — SSH bastion 経由の MySQL 接続（純 Rust `russh` 実装、ssh.exe 不要）。公開鍵認証・パスワード認証の両方に対応
- **SSH Agent / Config** — SSH Agent（1Password 対応）、`~/.ssh/config` 自動読み込み
- **SSL 接続** — DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY を選択可能
- **接続プール** — 同一設定の接続を自動再利用
- **SQL エディタ** — CodeMirror 5 ベース（シンタックスハイライト、キーワード・テーブル名補完、エラー箇所ハイライト）
- **マルチ SQL タブ** — 複数の SQL タブを同時に開いて作業（タブ内容・順序を自動保存、ドラッグで並び替え）
- **SQL 整形** — sql-formatter による MySQL 方言整形
- **クエリキャンセル** — 実行中クエリを `KILL QUERY` でキャンセル
- **行詳細モーダル** — テーブル行クリックで全カラムの詳細表示（JSON 自動整形）
- **カラムソート** — ヘッダークリックで ASC / DESC / なし の 3 ステートソート
- **BLOB/TEXT 切り詰め** — 大きなカラムを自動切り詰め表示（トグル可能）
- **データエクスポート** — CSV / TSV / SQL / Markdown（[tbls](https://github.com/k1LoW/tbls) 互換）形式で保存
- **パスワード安全保存** — Windows Credential Manager で管理（keyring 保存 or 接続時に都度入力を選択可能）
- **プロファイル管理** — グループ・色・タグ・ドラッグ＆ドロップで接続先を整理
- **インポート/エクスポート** — 接続設定の JSON エクスポート・インポート（重複検出付き）
- **ダークモード** — システム設定に追従 + 手動切替
- **日英切替 (i18n)** — 日本語 / English をワンクリックで切替
- **AI アシスト** — Claude / ChatGPT / Gemini によるチャット形式 SQL 生成（自然言語で依頼 → SQL をコピー/エディタ挿入）
- **Docker コンテナ自動検出** — Docker 上の MySQL コンテナを自動検出・ワンクリック接続。ports バインドなしのコンテナにも socat トンネル経由で接続可能。Docker Desktop / WSL2 dockerd 対応
- **ネイティブメニュー** — ハンバーガーメニュー + キーボードショートカット
- **アプリ内更新** — メニューから更新チェック・ワンクリック更新
- **Node.js 不要** — UI は素朴な HTML/JS/CSS

## 開発環境セットアップ

### 前提条件

- Rust toolchain (`rustup`)
- Visual Studio Build Tools (MSVC リンカー・Windows SDK)

### インストール

```powershell
# Rust toolchain
winget install Rustlang.Rustup

# Visual Studio Build Tools
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# Tauri CLI
cargo install tauri-cli
```

### 起動

```powershell
cd src-tauri
cargo tauri dev
```

初回ビルドはクレートのダウンロード・コンパイルに時間がかかります。

## 使い方

1. メイン画面で「新規」ボタンをクリックして接続プロファイルを作成
2. MySQL 接続情報（ホスト・ポート・ユーザー・パスワード）を入力
3. SSH 踏み台が必要な場合は「SSH Bastion」を有効にして bastion 情報を入力
4. SSL Mode を選択（VERIFY_CA / VERIFY_IDENTITY では CA 証明書を指定可能）
5. 「接続テスト」で接続確認、「接続」でクエリウィンドウを開く
6. データベースを選択し、テーブル一覧からデータ閲覧・SQL 実行

## ライセンス

MIT
