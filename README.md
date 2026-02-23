# MuSQL (Tauri v2 + Rust)

Windowsで動くMySQLクライアントです。SSH踏み台経由の接続をサポートし、踏み台接続時はOSの`ssh-agent`を使います（`ssh.exe`の標準動作）。

## 前提

- Rust toolchain
- Node.jsは不要（静的UIを同梱）
- WindowsのOpenSSH Client (`ssh.exe`)
- `ssh-agent`が起動済みで鍵が追加済み
- MySQLで`require_secure_transport=ON`の場合はTLS有効化が必要

Windows PowerShell例:

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
2. SSH踏み台が必要なら`Enable`をONにしてbastion情報を入力
3. TLSが必要な場合は`TLS を使用`をON
4. 証明書検証をスキップする場合は`TLS証明書検証をスキップ`をON
5. `接続テスト`または`クエリ実行`

## 実装メモ

- `src-tauri/src/main.rs`
- SSHは`ssh -N -L 127.0.0.1:<local>:<mysql_host>:<mysql_port> user@bastion`を子プロセスとして起動
- トンネル確立後にRust側がMySQL接続し、処理完了後にSSHプロセスを終了
- クエリ結果はJSONとしてUIに返却（最大500行）
- TLSは`mysql`クレートの`native-tls`を利用。UIの設定に応じて有効化/検証スキップを切り替え
