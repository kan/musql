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

<p align="center">
  <img src="docs/manual/img/overview.png" alt="muSQL の画面" width="820" />
</p>
<!-- 撮影: query 画面。左にテーブル一覧、中央に Data タブ（結果テーブルが数十行）、上部にタブが 2〜3 枚。ライトテーマ。docs/manual/img/overview.png と共用。 -->

---

**muSQL** は、SSH 踏み台・Docker コンテナ接続・AI アシスト・エクスポートをシンプルな 1 画面にまとめた、軽量な MySQL クライアントです。使い方は **[ユーザーマニュアル](docs/manual/README.md)** を参照してください。

## ダウンロード

<a href="https://apps.microsoft.com/detail/9mvgmmcf47gj?hl=ja-JP&gl=JP">
  <img src="https://get.microsoft.com/images/ja%20dark.svg" width="200" alt="Microsoft Store から入手" />
</a>

**[Microsoft Store](https://apps.microsoft.com/detail/9mvgmmcf47gj?hl=ja-JP&gl=JP) からのインストールを推奨します。** Microsoft による署名・自動更新・クリーンアンインストールに対応しています。

代替手段として [GitHub Releases](https://github.com/kan/musql/releases/latest) から `.exe` インストーラーを直接ダウンロードすることも可能です（こちらはアプリ内自動更新機能付き）。

> **Note:** GitHub Releases 版は未署名のため Windows SmartScreen の警告が表示されます。「詳細情報」→「実行」で起動できます。Store 版ではこの警告は表示されません。

## 主な機能

- **SSH 踏み台接続** — 純 Rust `russh` 実装（`ssh.exe` 不要）。公開鍵 / パスワード認証、SSH Agent（1Password 対応）、`~/.ssh/config` 参照
- **SSL 接続** — DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY
- **Docker 連携** — 起動中コンテナの MySQL を自動検出してワンクリック接続（ポート未公開でも socat トンネル、WSL2 dockerd 対応）
- **SQL エディタ** — シンタックスハイライト・補完・整形、複数タブ、実行中クエリのキャンセル、履歴、`Ctrl+P` の QuickOpen
- **データ閲覧** — ページング・ソート・行詳細（JSON 自動整形）
- **エクスポート** — CSV / TSV / SQL / Markdown（[tbls](https://github.com/k1LoW/tbls) 互換）。文字コード・改行コード選択、クリップボードコピー
- **AI アシスト** — Claude / ChatGPT / Gemini による自然言語からの SQL 生成
- **プロファイル管理** — グループ・色・タグで整理。外部 JSON 同期で複数マシン間共有
- **安全なパスワード保存** — Windows 資格情報マネージャー（都度入力も選択可）
- **その他** — ダーク / ライト、日英切替、長時間クエリの完了通知、Node.js 不要の静的 UI

詳しい使い方・画面ごとの説明は [ユーザーマニュアル](docs/manual/README.md) にあります。

## ドキュメント

- **[ユーザーマニュアル](docs/manual/README.md)** — 接続設定・SSH・Docker・クエリ・AI アシスト・エクスポート・同期などの使い方
- [CHANGELOG](CHANGELOG.md) — 変更履歴
- [CLAUDE.md](CLAUDE.md) — 開発者 / エージェント向けの内部情報

## ソースからビルド

```powershell
# 前提: Rust toolchain (rustup) と Visual Studio Build Tools (MSVC)
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
cargo install tauri-cli

# 起動（初回はクレートのビルドに時間がかかります）
cd src-tauri
cargo tauri dev --config tauri.dev.conf.json
```

ビルド構成・アーキテクチャ・開発上の注意は [CLAUDE.md](CLAUDE.md) を参照してください。

## ライセンス

MIT
