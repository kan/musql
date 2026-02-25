<p align="center">
  <img src="ui/icon.svg" width="96" height="96" alt="muSQL" />
</p>

<h1 align="center">muSQL</h1>

<p align="center">Windows 向け MySQL クライアント — Tauri v2 + Rust</p>

---

## 特徴

- **SSH 踏み台対応** — SSH bastion 経由の MySQL 接続をサポート
- **SSL 接続** — DISABLED / REQUIRED / VERIFY_CA / VERIFY_IDENTITY を選択可能
- **接続プール** — 同一設定の接続を自動再利用
- **SQL エディタ** — CodeMirror 5 ベース。シンタックスハイライト、キーワード・テーブル名補完、エラー箇所ハイライト
- **SQL 整形** — sql-formatter による MySQL 方言整形
- **クエリキャンセル** — 実行中クエリを `KILL QUERY` でキャンセル
- **データエクスポート** — CSV / TSV / SQL 形式で保存
- **パスワード安全保存** — Windows Credential Manager で管理
- **プロファイル管理** — グループ・色・タグで接続先を整理
- **Node.js 不要** — UI は素朴な HTML/JS/CSS

## セットアップ

### 前提条件

- Rust toolchain (`rustup`)
- Visual Studio Build Tools (MSVC リンカー・Windows SDK)
- Windows OpenSSH Client (`C:\Windows\System32\OpenSSH\ssh.exe`)

### インストール

```powershell
# Rust toolchain
winget install Rustlang.Rustup

# Visual Studio Build Tools
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# Tauri CLI
cargo install tauri-cli
```

### 起動

```powershell
cd src-tauri
cargo tauri dev
```

## 使い方

1. MySQL 接続情報を入力
2. SSH 踏み台が必要なら `Enable` を ON にして bastion 情報を入力
3. SSL Mode を選択（VERIFY_CA / VERIFY_IDENTITY では CA 証明書を指定可能）
4. `接続テスト` または `接続` でクエリウィンドウを開く

## ライセンス

MIT
