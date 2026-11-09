# Build the per-user Windows MSI.
#
# Run from the repository root after `cargo build --release -p markdownx -p markdownx-server`:
#   pwsh -File scripts/build-msi.ps1
#
# WiX 3.14 is required (candle.exe and light.exe). The MSI installs under
# %LOCALAPPDATA%\Programs\MarkdownX and does not need administrator rights.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = '0.1.8'
$dist = Join-Path $root 'dist'
$wxs = Join-Path $root 'packaging\windows\markdownx.wxs'
$msi = Join-Path $dist "MarkdownX-$version-x64.msi"

$wix = Join-Path ${env:ProgramFiles(x86)} 'WiX Toolset v3.14\bin'
if (-not (Test-Path -LiteralPath (Join-Path $wix 'candle.exe'))) {
    throw "WiX 3.14 was not found at $wix. Install WiX Toolset v3.14."
}

foreach ($file in @(
    'target\release\markdownx.exe',
    'target\release\markdownx-server.exe',
    'assets\branding\app-icon.ico',
    'server\start-server.ps1',
    'server\open-web.ps1',
    'server\start-server.vbs',
    'server\open-web.vbs'
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $file))) {
        throw "Missing build input: $file"
    }
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
Push-Location $root
try {
    & (Join-Path $wix 'candle.exe') -nologo -out (Join-Path $dist 'markdownx.wixobj') $wxs
    if ($LASTEXITCODE -ne 0) { throw "candle failed ($LASTEXITCODE)" }
    & (Join-Path $wix 'light.exe') -nologo -ext WixUIExtension `
        -out $msi (Join-Path $dist 'markdownx.wixobj')
    if ($LASTEXITCODE -ne 0) { throw "light failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

Get-Item -LiteralPath $msi | Select-Object FullName, Length, LastWriteTime
