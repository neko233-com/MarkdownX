# Build the per-user Windows MSI.
#
# Run from the repository root after `cargo build --release -p markdownx -p markdownx-server`:
#   pwsh -File scripts/build-msi.ps1
#
# WiX 3.14 is required (candle.exe and light.exe). The MSI installs under
# %LOCALAPPDATA%\Programs\MarkdownX and does not need administrator rights.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$versionLine = Select-String -LiteralPath (Join-Path $root 'Cargo.toml') -Pattern '^version = "([^"]+)"' | Select-Object -First 1
if (-not $versionLine) { throw 'Package version was not found in Cargo.toml.' }
$version = $versionLine.Matches[0].Groups[1].Value
$dist = Join-Path $root 'dist'
$wxs = Join-Path $root 'packaging\windows\markdownx.wxs'
$msi = Join-Path $dist "MarkdownX-$version-x64.msi"

$programFilesX86 = [Environment]::GetFolderPath('ProgramFilesX86')
if (-not $programFilesX86) { throw 'Windows Program Files (x86) directory was not found.' }
$wix = Join-Path $programFilesX86 'WiX Toolset v3.14\bin'
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
$wixobj = Join-Path $dist 'markdownx.wixobj'
$candleOut = Join-Path $dist 'candle.log'
$candleErr = Join-Path $dist 'candle.err.log'
$lightOut = Join-Path $dist 'light.log'
$lightErr = Join-Path $dist 'light.err.log'
$candleArgs = @('-nologo', "-dProductVersion=$version", '-out', ('"' + $wixobj + '"'), ('"' + $wxs + '"'))
$candle = Start-Process -FilePath (Join-Path $wix 'candle.exe') -ArgumentList $candleArgs -WorkingDirectory $root `
    -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $candleOut -RedirectStandardError $candleErr
if ($candle.ExitCode -ne 0) {
    Get-Content -LiteralPath $candleOut, $candleErr -ErrorAction SilentlyContinue
    throw "candle failed ($($candle.ExitCode))"
}
$lightArgs = @('-nologo', '-ext', 'WixUIExtension', '-out', ('"' + $msi + '"'), ('"' + $wixobj + '"'))
$light = Start-Process -FilePath (Join-Path $wix 'light.exe') -ArgumentList $lightArgs -WorkingDirectory $root `
    -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $lightOut -RedirectStandardError $lightErr
if ($light.ExitCode -ne 0) {
    Get-Content -LiteralPath $lightOut, $lightErr -ErrorAction SilentlyContinue
    throw "light failed ($($light.ExitCode))"
}

$hash = (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText(($msi + '.sha256'), ($hash + '  ' + [IO.Path]::GetFileName($msi) + "`n"), [Text.Encoding]::ASCII)
Get-Item -LiteralPath $msi | Select-Object FullName, Length, LastWriteTime
