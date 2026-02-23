# MuSQL project notes (for agents)

## Overview
- Windows向けのMySQLクライアント。Tauri v2 + Rust backend + 静的UI。
- SSH踏み台経由のMySQL接続に対応。Windowsの `ssh.exe` と `ssh-agent` を利用。
- UIは `ui/` の素朴なHTML/JS/CSS。Node.js不要。

## How to run (dev)
- 前提: Rust toolchain, Windows OpenSSH Client (`ssh.exe`), `ssh-agent` 起動＆鍵追加済み。
- 起動:
  - `cd src-tauri`
  - `cargo tauri dev`

## Main flow
- UIで接続情報 + クエリを入力。
- `ui/app.js` からTauri `invoke`:
  - `test_connection` → `src-tauri/src/main.rs`
  - `run_query` → `src-tauri/src/main.rs`
- SSH有効時:
  - Rust側で `ssh -N -L 127.0.0.1:<local>:<mysql_host>:<mysql_port> user@bastion` を起動
  - ローカルポートへ接続できるまで待機 → MySQL接続 → 終了時にSSHプロセスをkill

## Key files
- `README.md` 前提や起動手順。
- `src-tauri/src/main.rs`
  - MySQL接続/クエリ実行、SSHトンネル管理、結果JSON化（最大500行）。
- `ui/index.html` 画面構成。
- `ui/app.js` Tauri `invoke` とフォーム収集。
- `ui/style.css` UIスタイル。
- `src-tauri/tauri.conf.json` Tauri設定。

## Behavior notes
- `test_connection` は `SELECT 1` を実行。
- `run_query` は任意SQLを実行して列名＋行＋`affected_rows` を返却。
- 返却はJSON文字列としてUIの `pre` に表示。
- `ssh.private_key_path` は任意。基本は `ssh-agent` 利用想定。
- README に TLS 有効化/検証スキップの使い方が追記済み。

## Limits / defaults
- 結果行は最大500件まで。
- SSHトンネル確立待ちタイムアウトは8秒。
