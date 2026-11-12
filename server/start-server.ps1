$ErrorActionPreference = 'Stop'

# The MSI installs this file under <app>\server. The collaboration workspace
# lives in %LOCALAPPDATA%\MarkdownX\server so uninstalling the app does not
# delete a team's Git repository.
$here = Split-Path -Parent $PSCommandPath
$data = Join-Path $env:LOCALAPPDATA 'MarkdownX\server'
New-Item -ItemType Directory -Force -Path $data | Out-Null

$tokenFile = Join-Path $data 'token.txt'
if (-not (Test-Path -LiteralPath $tokenFile)) {
    $bytes = New-Object byte[] 32
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    (($bytes | ForEach-Object { $_.ToString('x2') }) -join '') |
        Set-Content -LiteralPath $tokenFile -Encoding ascii -NoNewline
}
$token = (Get-Content -LiteralPath $tokenFile -Raw).Trim()

$log = Join-Path $data 'server.log'
$err = Join-Path $data 'server.err.log'
if ((Test-Path -LiteralPath $log) -and ((Get-Item -LiteralPath $log).Length -gt 5MB)) {
    Move-Item -LiteralPath $log -Destination ($log + '.1') -Force
}

$port = 8877
$health = "http://127.0.0.1:$port/api/health"

# Do not start a second copy if the port is already healthy.
try {
    Invoke-RestMethod -Uri $health -TimeoutSec 1 | Out-Null
    exit 0
} catch {}

$exe = Join-Path $here 'markdownx-server.exe'
$web = Join-Path $here 'web'
$serverArgs = @(
    '-addr', "127.0.0.1:$port",
    '-root', ('"' + $data + '"'),
    '-web', ('"' + $web + '"')
)
$previousToken = [Environment]::GetEnvironmentVariable('MARKDOWNX_SERVER_TOKEN', 'Process')
try {
    # Pass the credential in the inherited environment rather than exposing
    # it in the server's process command line.
    [Environment]::SetEnvironmentVariable('MARKDOWNX_SERVER_TOKEN', $token, 'Process')
    Start-Process -FilePath $exe -ArgumentList $serverArgs -WorkingDirectory $data `
        -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $err | Out-Null
} finally {
    [Environment]::SetEnvironmentVariable('MARKDOWNX_SERVER_TOKEN', $previousToken, 'Process')
}

for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Milliseconds 250
    try {
        Invoke-RestMethod -Uri $health -TimeoutSec 1 | Out-Null
        exit 0
    } catch {}
}
exit 1
