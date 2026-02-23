# MuSQL (Tauri v2 + Rust)

Windowsで動くMySQLクライアントです。SSH踏み台経由の接続をサポートし、踏み台接続時はOSの `ssh-agent` を使います（`ssh.exe` の標準動作）。

## 前提

- Rust toolchain
- Node.js は不要（静的UIを同梱）
- Windowsの OpenSSH Client (`ssh.exe`)
- `ssh-agent` が起動済みで鍵が追加済み

Windows PowerShell 例:

```powershell
Get-Service ssh-agent | Set-Service -StartupType Automatic
Start-Service ssh-agent
ssh-add $env:USERPROFILE\.ssh\id_ed25519
```

## 実行

```bash
cd src-tauri
cargo tauri dev
```

## 使い方

1. MySQL接続情報を入力
2. SSH踏み台が必要なら `Enable` をONにして bastion 情報を入力
3. `接続テスト` または `クエリ実行`

## 実装メモ

- `src-tauri/src/main.rs`
- SSHは `ssh -N -L 127.0.0.1:<local>:<mysql_host>:<mysql_port> user@bastion` を子プロセスとして起動
- トンネル確立後にRust側がMySQL接続し、処理完了後にSSHプロセスを終了
- クエリ結果はJSONとしてUIに返却（最大500行）
