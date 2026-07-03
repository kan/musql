# 撮影用の架空プロファイルをデバッグビルドの設定ディレクトリに配置する。
# デバッグビルド (tauri.dev.conf.json) は identifier が jp.co.communitylinks.musql.debug
# のため、インストール版のプロファイルには影響しない。
#
# -Variant full   : グループ 2 つ + 色/タグ付き 6 件 (profile-list-colored.png 等)
# -Variant simple : グループ 1 つ + 3 件 (screen-layout.png 用)
param(
    [ValidateSet('full', 'simple')]
    [string]$Variant = 'full'
)
$ErrorActionPreference = 'Stop'

$srcName = if ($Variant -eq 'simple') { 'connections.simple.json' } else { 'connections.debug.json' }
$src = Join-Path $PSScriptRoot $srcName
$dstDir = Join-Path $env:APPDATA 'jp.co.communitylinks.musql.debug'
$dst = Join-Path $dstDir 'connections.json'

New-Item -ItemType Directory -Force $dstDir | Out-Null

if (Test-Path $dst) {
    $backup = "$dst.bak-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Copy-Item $dst $backup
    Write-Host "既存の connections.json を $backup に退避しました"
}

Copy-Item $src $dst
Write-Host "配置完了 ($Variant): $dst"
Write-Host '「ローカル開発 (Docker)」のパスワード (demo1234) は初回に設定画面で入力して保存してください。'
