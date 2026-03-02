<#
.SYNOPSIS
    Build an unsigned MSIX package for Microsoft Store submission.
.PARAMETER Version
    Four-part version string (e.g. "0.3.0.0").
.PARAMETER ExePath
    Path to the Store-variant musql.exe.
.PARAMETER OutputPath
    Destination path for the .msix file.
#>
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $ExePath,
    [Parameter(Mandatory)] [string] $OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$iconsDir = Join-Path $repoRoot 'src-tauri' 'icons'
$manifestTemplate = Join-Path $PSScriptRoot 'AppxManifest.xml'

# --- Staging directory ---
$staging = Join-Path $env:TEMP "musql-msix-$(Get-Random)"
New-Item -ItemType Directory -Path $staging | Out-Null
try {
    # Copy EXE
    Copy-Item $ExePath (Join-Path $staging 'musql.exe')

    # Copy icon assets
    $assetsDir = Join-Path $staging 'Assets'
    New-Item -ItemType Directory -Path $assetsDir | Out-Null
    $icons = @(
        'StoreLogo.png',
        'Square44x44Logo.png',
        'Square150x150Logo.png',
        'Square310x310Logo.png'
    )
    foreach ($icon in $icons) {
        $src = Join-Path $iconsDir $icon
        if (Test-Path $src) {
            Copy-Item $src (Join-Path $assetsDir $icon)
        } else {
            Write-Warning "Icon not found, skipping: $src"
        }
    }

    # Process manifest template
    $manifest = Get-Content $manifestTemplate -Raw
    $manifest = $manifest.Replace('{{VERSION}}', $Version)
    Set-Content -Path (Join-Path $staging 'AppxManifest.xml') -Value $manifest -Encoding UTF8

    # Find makeappx.exe from Windows SDK
    $sdkRoot = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    $makeappx = Get-ChildItem -Path $sdkRoot -Recurse -Filter 'makeappx.exe' -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match 'x64' } |
        Sort-Object { [version]($_.FullName -replace '.*\\(\d+\.\d+\.\d+\.\d+)\\.*','$1') } -Descending |
        Select-Object -First 1

    if (-not $makeappx) {
        throw "makeappx.exe not found. Install Windows 10 SDK."
    }

    Write-Host "Using: $($makeappx.FullName)"
    & $makeappx.FullName pack /d $staging /p $OutputPath /nv /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx.exe failed with exit code $LASTEXITCODE" }

    Write-Host "MSIX created: $OutputPath"
} finally {
    Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
}
