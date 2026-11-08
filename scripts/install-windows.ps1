# Install MarkdownX for the current Windows user.
#
#   pwsh -File scripts/install-windows.ps1
#
# Builds are expected in target\release. The desktop app is copied to
# %LOCALAPPDATA%\Programs\MarkdownX together with the Rust collaboration
# server, and the server is registered to start at sign-in. No administrator
# rights are needed; all registry writes are under HKCU.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $env:LOCALAPPDATA 'Programs\MarkdownX'
$data = Join-Path $env:LOCALAPPDATA 'MarkdownX\server'
$app = Join-Path $root 'target\release\markdownx.exe'
$server = Join-Path $root 'target\release\markdownx-server.exe'
$icon = Join-Path $root 'assets\branding\app-icon.ico'
$version = '0.1.7'
$taskName = 'MarkdownX Collaboration Server'
$port = 8787

foreach ($f in @($app, $server, $icon)) {
    if (-not (Test-Path -LiteralPath $f)) { throw "Missing build output: $f. Run cargo build --release --locked -p markdownx -p markdownx-server first." }
}
# Windows locks running executables. Stop both processes before replacing
# files; the server is started again below.
foreach ($name in @('markdownx', 'markdownx-server')) {
    Get-Process $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 400

New-Item -ItemType Directory -Force -Path $dest, (Join-Path $dest 'server'), $data | Out-Null
Copy-Item -LiteralPath $app -Destination (Join-Path $dest 'markdownx.exe') -Force
Copy-Item -LiteralPath $server -Destination (Join-Path $dest 'server\markdownx-server.exe') -Force
Copy-Item -LiteralPath $icon -Destination (Join-Path $dest 'app-icon.ico') -Force
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $dest -Force
Copy-Item -LiteralPath (Join-Path $root 'THIRD_PARTY.md') -Destination $dest -Force
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $dest -Force
$webDest = Join-Path $dest 'server\web'
if (Test-Path -LiteralPath $webDest) { Remove-Item -LiteralPath $webDest -Recurse -Force }
Copy-Item -LiteralPath (Join-Path $root 'server\web') -Destination $webDest -Recurse -Force

# Keep an existing token so reinstalling does not log every browser out.
$tokenFile = Join-Path $data 'token.txt'
if (Test-Path -LiteralPath $tokenFile) {
    $token = (Get-Content -LiteralPath $tokenFile -Raw).Trim()
} else {
    $bytes = New-Object byte[] 24
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $token = -join ($bytes | ForEach-Object { $_.ToString('x2') })
    Set-Content -LiteralPath $tokenFile -Value $token -NoNewline
}
Set-Content -LiteralPath (Join-Path $data 'server-url.txt') -Value "http://127.0.0.1:$port/web/`nToken: $token"

$startScript = Join-Path $dest 'server\start-server.ps1'
@'
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $PSCommandPath
$data = Join-Path $env:LOCALAPPDATA 'MarkdownX\server'
$token = (Get-Content -LiteralPath (Join-Path $data 'token.txt') -Raw).Trim()
$log = Join-Path $data 'server.log'
if ((Test-Path -LiteralPath $log) -and ((Get-Item -LiteralPath $log).Length -gt 5MB)) {
    Move-Item -LiteralPath $log -Destination ($log + '.1') -Force
}
& (Join-Path $here 'markdownx-server.exe') -addr 127.0.0.1:8787 -root $data -token $token -web (Join-Path $here 'web') 2>&1 |
    Out-File -LiteralPath $log -Append -Encoding utf8
'@ | Set-Content -LiteralPath $startScript -Encoding UTF8

$openScript = Join-Path $dest 'server\open-web.ps1'
@'
$data = Join-Path $env:LOCALAPPDATA 'MarkdownX\server'
$token = (Get-Content -LiteralPath (Join-Path $data 'token.txt') -Raw).Trim()
$url = 'http://127.0.0.1:8787/web/?token=' + [Uri]::EscapeDataString($token)
Start-Process $url
'@ | Set-Content -LiteralPath $openScript -Encoding UTF8

$uninstallScript = Join-Path $dest 'uninstall.ps1'
@'
$ErrorActionPreference = 'SilentlyContinue'
Get-Process markdownx-server | Stop-Process -Force
Get-Process markdownx | Stop-Process -Force
Unregister-ScheduledTask -TaskName 'MarkdownX Collaboration Server' -Confirm:$false
Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Startup')) 'MarkdownX Server.lnk') -Force
Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Programs')) 'MarkdownX.lnk') -Force
Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Programs')) 'MarkdownX Server.lnk') -Force
Remove-Item -LiteralPath (Join-Path ([Environment]::GetFolderPath('Desktop')) 'MarkdownX.lnk') -Force
Remove-Item -Path 'HKCU:\Software\Classes\.mdxb' -Recurse -Force
Remove-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book' -Recurse -Force
Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\MarkdownX' -Recurse -Force
Start-Process $env:ComSpec -ArgumentList '/c', ('timeout /t 2 >nul & rd /s /q "' + $PSScriptRoot + '"') -WindowStyle Hidden
'@ | Set-Content -LiteralPath $uninstallScript -Encoding UTF8

# Shortcuts: Start Menu, desktop, and a server status page.
$shell = New-Object -ComObject WScript.Shell
function New-Shortcut($path, $target, $arguments, $description) {
    $s = $shell.CreateShortcut($path)
    $s.TargetPath = $target
    $s.Arguments = $arguments
    $s.WorkingDirectory = $dest
    $s.IconLocation = (Join-Path $dest 'app-icon.ico')
    $s.Description = $description
    $s.Save()
}
$programs = [Environment]::GetFolderPath('Programs')
$desktop = [Environment]::GetFolderPath('Desktop')
New-Shortcut (Join-Path $programs 'MarkdownX.lnk') (Join-Path $dest 'markdownx.exe') '' 'MarkdownX interactive books'
New-Shortcut (Join-Path $desktop 'MarkdownX.lnk') (Join-Path $dest 'markdownx.exe') '' 'MarkdownX interactive books'
New-Shortcut (Join-Path $programs 'MarkdownX Collaboration.lnk') 'powershell.exe' ('-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $openScript + '"') 'MarkdownX collaboration workspace'
# Remove the older "start the server" shortcut: the server now starts at sign-in,
# so the Start Menu entry opens the workspace instead.
Remove-Item -LiteralPath (Join-Path $programs 'MarkdownX Server.lnk') -Force -ErrorAction SilentlyContinue

# File association and Add/Remove Programs entry (per user).
New-Item -Path 'HKCU:\Software\Classes\.mdxb' -Force | Out-Null
Set-Item -Path 'HKCU:\Software\Classes\.mdxb' -Value 'MarkdownX.Book'
New-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book' -Force | Out-Null
Set-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book' -Value 'MarkdownX Book'
New-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book\DefaultIcon' -Force | Out-Null
Set-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book\DefaultIcon' -Value ((Join-Path $dest 'markdownx.exe') + ',0')
New-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book\shell\open\command' -Force | Out-Null
Set-Item -Path 'HKCU:\Software\Classes\MarkdownX.Book\shell\open\command' -Value ('"' + (Join-Path $dest 'markdownx.exe') + '" "%1"')
$uninstall = Join-Path $dest 'uninstall.ps1'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\MarkdownX'
New-Item -Path $uninstallKey -Force | Out-Null
Set-ItemProperty -Path $uninstallKey -Name DisplayName -Value 'MarkdownX'
Set-ItemProperty -Path $uninstallKey -Name DisplayVersion -Value $version
Set-ItemProperty -Path $uninstallKey -Name Publisher -Value 'neko233-com'
Set-ItemProperty -Path $uninstallKey -Name DisplayIcon -Value (Join-Path $dest 'markdownx.exe')
Set-ItemProperty -Path $uninstallKey -Name InstallLocation -Value $dest
Set-ItemProperty -Path $uninstallKey -Name UninstallString -Value ('powershell.exe -NoProfile -ExecutionPolicy Bypass -File "' + $uninstall + '"')

# Background server: a per-user logon task, restarted up to three times.
try {
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $startScript + '"')
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Description 'MarkdownX collaboration server on 127.0.0.1:8787' -Force | Out-Null
} catch {
    # Older Windows without the ScheduledTasks module: a hidden Startup link.
    $startup = [Environment]::GetFolderPath('Startup')
    New-Shortcut (Join-Path $startup 'MarkdownX Server.lnk') 'powershell.exe' ('-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $startScript + '"') 'MarkdownX collaboration server'
}

# Stop an older server, then start the installed one now.
Get-Process markdownx-server -ErrorAction SilentlyContinue | Stop-Process -Force
if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
    Start-ScheduledTask -TaskName $taskName
} else {
    Start-Process powershell.exe -ArgumentList ('-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $startScript + '"') -WindowStyle Hidden
}
Start-Sleep -Seconds 2
try {
    $health = Invoke-RestMethod -Uri "http://127.0.0.1:$port/api/health" -TimeoutSec 5
    Write-Host "MarkdownX installed to $dest"
    Write-Host "Collaboration server: http://127.0.0.1:$port/web/ (token in $tokenFile)"
    Write-Host "Health: $($health | ConvertTo-Json -Compress)"
} catch {
    Write-Warning "MarkdownX installed, but the server did not answer on port $port. Start it from the Start Menu shortcut, or see $data\server.log."
}
