$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $PSCommandPath
$data = Join-Path $env:LOCALAPPDATA 'MarkdownX\server'
$tokenFile = Join-Path $data 'token.txt'
$health = 'http://127.0.0.1:8877/api/health'

$ready = $false
for ($i = 0; $i -lt 5; $i++) {
    try { Invoke-RestMethod -Uri $health -TimeoutSec 1 | Out-Null; $ready = $true; break } catch { Start-Sleep -Milliseconds 300 }
}
if (-not $ready) {
    Start-Process powershell.exe -ArgumentList @(
        '-NoProfile', '-WindowStyle', 'Hidden', '-ExecutionPolicy', 'Bypass',
        '-File', ('"' + (Join-Path $here 'start-server.ps1') + '"')
    ) -WindowStyle Hidden
    for ($i = 0; $i -lt 25; $i++) {
        try { Invoke-RestMethod -Uri $health -TimeoutSec 1 | Out-Null; $ready = $true; break } catch { Start-Sleep -Milliseconds 400 }
    }
}
if (-not $ready) {
    Add-Type -AssemblyName PresentationFramework
    [System.Windows.MessageBox]::Show('MarkdownX could not start its collaboration server. See %LOCALAPPDATA%\MarkdownX\server\server.log and server.err.log.', 'MarkdownX') | Out-Null
    return
}
$token = (Get-Content -LiteralPath $tokenFile -Raw).Trim()
Start-Process ('http://127.0.0.1:8877/web/?token=' + [Uri]::EscapeDataString($token))
