#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$MsiPath,
    [string]$Sha256,
    [string]$ReleaseApi = 'https://api.github.com/repos/neko233-com/MarkdownX/releases',
    [switch]$NoLaunch,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'This installer requires Windows. On macOS or Linux, run sh install.sh.' }
if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [System.Runtime.InteropServices.Architecture]::X64) {
    throw 'The Windows MSI currently supports x64 Windows.'
}
$installRoot = Join-Path $env:LOCALAPPDATA 'Programs\MarkdownX'
$appPath = Join-Path $installRoot 'markdownx.exe'
$serverPath = Join-Path $installRoot 'server\markdownx-server.exe'
$upgradeCode = '{5F2E3D9A-8A1B-4C7E-9E4D-2B7C6A1F0D31}'
$scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).ProviderPath }
$allowLoopback = $PSBoundParameters.ContainsKey('ReleaseApi')
$tempRoot = $null

function Get-WorkspaceVersion([string]$Root) {
    $manifest = Join-Path $Root 'Cargo.toml'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { return $null }
    if (-not (Select-String -LiteralPath $manifest -Pattern '^name = "markdownx"$' -Quiet)) { return $null }
    $line = Select-String -LiteralPath $manifest -Pattern '^version = "(\d+\.\d+\.\d+)"' | Select-Object -First 1
    if ($null -eq $line) { throw 'Could not determine the MarkdownX workspace version.' }
    return $line.Matches[0].Groups[1].Value
}

function Get-Checksum([string]$ChecksumFile, [string]$Name) {
    $value = (Get-Content -LiteralPath $ChecksumFile -Raw).Trim()
    if ($value -match '^(?i:([0-9a-f]{64}))(?:\s+\*?([^\r\n]+))?$') {
        if ($Matches.ContainsKey(2) -and $Matches[2].Trim() -ine $Name) { throw 'The checksum names a different package.' }
        return $Matches[1].ToLowerInvariant()
    }
    throw 'The SHA-256 checksum file is malformed.'
}

function Assert-AllowedUrl([string]$Value) {
    $uri = $null
    if (-not [Uri]::TryCreate($Value, [UriKind]::Absolute, [ref]$uri)) { throw 'The release contains an invalid download URL.' }
    if ($uri.Scheme -ne 'https' -and -not ($allowLoopback -and $uri.Scheme -eq 'http' -and $uri.IsLoopback)) {
        throw 'Release downloads require HTTPS.'
    }
}

function Get-DownloadedPackage([string]$Api, [string]$ExpectedWorkspaceVersion) {
    Assert-AllowedUrl $Api
    try { $responseData = Invoke-RestMethod -Uri $Api -Headers @{ 'User-Agent' = 'MarkdownX-Installer'; 'Accept' = 'application/vnd.github+json' } -TimeoutSec 30 }
    catch {
        $response = $_.Exception.PSObject.Properties['Response']
        if (($response -and $response.Value -and [int]$response.Value.StatusCode -eq 404) -or $ExpectedWorkspaceVersion) { return $null }
        throw
    }
    if ($responseData -is [array]) {
        $available = @($responseData | Where-Object { -not $_.draft })
        $release = @($available | Select-Object -First 1)
        if (-not $release.Count) { return $null }
        $release = $release[0]
    } else { $release = $responseData }
    if ($release.prerelease) { Write-Warning 'Installing from a GitHub pre-release.' }
    $assets = @($release.assets | Where-Object { $_.name -match '^MarkdownX-(\d+\.\d+\.\d+)-x64\.msi$' })
    if ($assets.Count -ne 1) { throw 'The latest release does not contain exactly one Windows x64 MSI.' }
    $asset = $assets[0]
    if ($asset.name -notmatch '^MarkdownX-(\d+\.\d+\.\d+)-x64\.msi$') { throw 'Invalid release package name.' }
    $version = $Matches[1]
    if ($ExpectedWorkspaceVersion -and [version]$version -lt [version]$ExpectedWorkspaceVersion) { return $null }
    if ($asset.PSObject.Properties['size'] -and ($asset.size -le 0 -or $asset.size -gt 1073741824)) { throw 'Invalid release package size.' }
    Assert-AllowedUrl ([string]$asset.browser_download_url)
    $checksumAsset = @($release.assets | Where-Object { $_.name -ceq ($asset.name + '.sha256') })
    $digest = if ($asset.PSObject.Properties['digest'] -and $asset.digest -match '^sha256:([0-9a-fA-F]{64})$') { $Matches[1].ToLowerInvariant() } else { $null }
    if ($checksumAsset.Count -gt 1 -or (-not $digest -and $checksumAsset.Count -ne 1)) { throw 'The release needs a SHA-256 digest or an adjacent .sha256 file.' }
    $script:tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('markdownx-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $script:tempRoot -ErrorAction Stop | Out-Null
    $package = Join-Path $script:tempRoot 'package.msi'
    if ($checksumAsset.Count -eq 1) {
        Assert-AllowedUrl ([string]$checksumAsset[0].browser_download_url)
        $sumPath = Join-Path $script:tempRoot 'package.sha256'
        Invoke-WebRequest -Uri $checksumAsset[0].browser_download_url -OutFile $sumPath -TimeoutSec 30
        $listed = Get-Checksum $sumPath $asset.name
        if ($digest -and $digest -ne $listed) { throw 'Release digest and checksum file disagree.' }
        $digest = $listed
    }
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $package -TimeoutSec 300
    return [pscustomobject]@{ Path = $package; Version = $version; Digest = $digest; Downloaded = $true }
}

function Build-WorkspacePackage([string]$Root, [string]$Version) {
    $builder = Join-Path $Root 'scripts\build-msi.ps1'
    $cleaner = Join-Path $Root 'scripts\clean_test_artifacts.py'
    if (-not (Test-Path -LiteralPath $builder -PathType Leaf) -or -not (Get-Command cargo -ErrorAction SilentlyContinue) -or -not (Get-Command python -ErrorAction SilentlyContinue)) {
        throw 'No MSI was found. Building requires Rust, Python, Visual Studio C++ tools and WiX 3.14.'
    }
    if ($DryRun) { throw 'No MSI is available; a dry run does not build one.' }
    Push-Location $Root
    try {
        & python $cleaner
        if ($LASTEXITCODE -ne 0) { throw 'Test-artifact cleanup failed.' }
        & cargo build --release --locked -p markdownx -p markdownx-server
        if ($LASTEXITCODE -ne 0) { throw 'The MarkdownX release build failed.' }
        & (Get-Process -Id $PID).Path -NoProfile -File $builder
        if ($LASTEXITCODE -ne 0) { throw 'The MSI build failed.' }
    } finally { Pop-Location }
    $package = Join-Path $Root "dist\MarkdownX-$Version-x64.msi"
    if (-not (Test-Path -LiteralPath $package -PathType Leaf)) { throw 'The expected MSI was not produced.' }
    return [pscustomobject]@{ Path = $package; Version = $Version; Digest = $null; Downloaded = $false }
}

function Read-MsiInfo([string]$Path) {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $null
    try {
        $database = $installer.OpenDatabase($Path, 0)
        $properties = @{}
        foreach ($name in @('ProductName', 'ProductVersion', 'ProductCode', 'UpgradeCode')) {
            $query = 'SELECT `Value` FROM `Property` WHERE `Property`=' + "'$name'"
            $view = $database.OpenView($query)
            try {
                [void]$view.Execute()
                $record = $view.Fetch()
                if ($null -eq $record) { throw "MSI is missing $name." }
                try { $properties[$name] = $record.StringData(1) } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record) }
            } finally { [void]$view.Close(); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view) }
        }
        if ($properties.ProductName -cne 'MarkdownX' -or $properties.UpgradeCode -ine $upgradeCode -or $properties.ProductVersion -notmatch '^\d+\.\d+\.\d+$') {
            throw 'The MSI is not a supported MarkdownX package.'
        }
        return [pscustomobject]$properties
    } finally {
        if ($database) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
    }
}

function Initialize-Native {
    if ('MarkdownXInstallNative' -as [type]) { return }
    Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Text;
using System.Runtime.InteropServices;
public static class MarkdownXInstallNative {
    [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("msi.dll", CharSet = CharSet.Unicode)] public static extern int MsiQueryProductStateW(string product);
    private delegate bool EnumCallback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumCallback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] private static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    public static string[] Arguments(string command) {
        int count; IntPtr vector = CommandLineToArgvW(command, out count);
        if (vector == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        try {
            string[] args = new string[count];
            for (int i = 0; i < count; i++) args[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(vector, i * IntPtr.Size));
            return args;
        } finally { LocalFree(vector); }
    }
    public static IntPtr[] Windows(int process) {
        var result = new List<IntPtr>();
        EnumWindows((window, data) => {
            uint pid; GetWindowThreadProcessId(window, out pid);
            var title = new StringBuilder(256); GetWindowTextW(window, title, title.Capacity);
            if (pid == process && IsWindowVisible(window) && GetWindow(window, 4) == IntPtr.Zero && title.ToString() == "MarkdownX") result.Add(window);
            return true;
        }, IntPtr.Zero);
        return result.ToArray();
    }
    public static void RequestClose(IntPtr window) {
        if (!PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    }
}
"@
}

function Get-InstalledProcesses([string]$Name, [string]$ExpectedPath) {
    return @(Get-Process -Name $Name -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and [string]::Equals($_.Path, $ExpectedPath, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
    })
}

function Assert-NoServerSessions([System.Diagnostics.Process]$Process) {
    try { $connections = @(Get-NetTCPConnection -OwningProcess $Process.Id -ErrorAction Stop) }
    catch {
        if ($Process.HasExited) { return }
        throw 'Could not verify collaboration sessions; the server was not stopped.'
    }
    $active = @($connections | Where-Object { [string]$_.State -notin @('Listen', 'TimeWait', 'Closed', 'DeleteTcb') })
    if ($active.Count) { throw 'A collaboration session is active; retry after the session ends.' }
}

function Stop-ServerGracefully([System.Diagnostics.Process]$Process) {
    if ($Process.HasExited) { return }
    Assert-NoServerSessions $Process
    # The console attachment happens in an isolated PowerShell job. It never
    # detaches the user's terminal or sets an inheritable ignore-Ctrl+C flag.
    $job = Start-Job -ArgumentList $Process.Id, $serverPath -ScriptBlock {
        param([int]$TargetId, [string]$ExpectedPath)
        $ErrorActionPreference = 'Stop'
        Add-Type -TypeDefinition @"
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public static class MarkdownXStopConsole {
    private delegate bool Handler(uint type);
    private static readonly Handler handler = value => value == 0;
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool FreeConsole();
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool AttachConsole(uint pid);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern uint GetConsoleProcessList(uint[] list, uint count);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool SetConsoleCtrlHandler(Handler handler, bool add);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool GenerateConsoleCtrlEvent(uint type, uint group);
    public static void Request(int target, string expected) {
        using (Process p = Process.GetProcessById(target)) {
            if (!String.Equals(p.MainModule.FileName, expected, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("Unexpected server executable.");
        }
        FreeConsole();
        if (!AttachConsole((uint)target)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        try {
            uint[] list = new uint[64]; uint count = GetConsoleProcessList(list, (uint)list.Length);
            uint self = (uint)Process.GetCurrentProcess().Id;
            if (count != 2 || !((list[0] == self && list[1] == target) || (list[1] == self && list[0] == target)))
                throw new InvalidOperationException("The server console is shared.");
            if (!SetConsoleCtrlHandler(handler, true) || !GenerateConsoleCtrlEvent(0, 0))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            Thread.Sleep(500);
        } finally { FreeConsole(); }
    }
}
"@
        [MarkdownXStopConsole]::Request($TargetId, $ExpectedPath)
    }
    try {
        if (-not (Wait-Job -Job $job -Timeout 15)) { throw 'The safe shutdown helper did not finish.' }
        Receive-Job -Job $job -ErrorAction Stop | Out-Null
        if ($job.State -ne 'Completed') { throw 'Could not request a safe collaboration-server shutdown.' }
    } finally {
        if ($job.State -in @('Completed', 'Failed', 'Stopped')) { Remove-Job -Job $job }
    }
    if (-not $Process.WaitForExit(20000)) { throw 'The collaboration server did not exit; it was not forced to stop.' }
}

function Start-InstalledServer {
    $launcher = Join-Path $installRoot 'server\start-server.ps1'
    if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) { throw 'The collaboration-server launcher is missing.' }
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    foreach ($arg in @('-NoProfile', '-File', $launcher)) { $start.ArgumentList.Add($arg) }
    $launcherProcess = [Diagnostics.Process]::Start($start)
    if (-not $launcherProcess.WaitForExit(30000) -or $launcherProcess.ExitCode -ne 0) { throw 'The collaboration server did not report healthy.' }
    if (@(Get-InstalledProcesses 'markdownx-server' $serverPath).Count -ne 1) { throw 'The installed collaboration-server process was not found.' }
}

function Start-InstalledApp([string[]]$Arguments) {
    $start = [Diagnostics.ProcessStartInfo]::new($appPath)
    # A GUI app must not inherit the installer's short-lived console streams.
    # Shell execution keeps the restored window alive when the installer exits.
    $start.UseShellExecute = $true
    $start.WorkingDirectory = $installRoot
    foreach ($arg in $Arguments) { $start.ArgumentList.Add($arg) }
    [void][Diagnostics.Process]::Start($start)
}

try {
    $workspaceVersion = Get-WorkspaceVersion $scriptRoot
    if ($MsiPath) {
        $package = [pscustomobject]@{ Path = (Resolve-Path -LiteralPath $MsiPath).ProviderPath; Version = $null; Digest = $null; Downloaded = $false }
    } else {
        $local = if ($workspaceVersion) { Join-Path $scriptRoot "dist\MarkdownX-$workspaceVersion-x64.msi" } else { $null }
        if ($local -and (Test-Path -LiteralPath $local -PathType Leaf)) {
            $package = [pscustomobject]@{ Path = $local; Version = $workspaceVersion; Digest = $null; Downloaded = $false }
        } else {
            $adjacent = @(Get-ChildItem -LiteralPath $scriptRoot -File -Filter 'MarkdownX-*-x64.msi' -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -match '^MarkdownX-(\d+\.\d+\.\d+)-x64\.msi$' } |
                Sort-Object { [version](($_.BaseName -replace '^MarkdownX-', '') -replace '-x64$', '') } -Descending)
            if (-not $workspaceVersion -and $adjacent.Count) {
                $package = [pscustomobject]@{ Path = $adjacent[0].FullName; Version = $null; Digest = $null; Downloaded = $false }
            } else {
                $package = Get-DownloadedPackage $ReleaseApi $workspaceVersion
                if (-not $package) {
                    if (-not $workspaceVersion) { throw 'No published MarkdownX release is available yet.' }
                    Write-Host 'No current release is available; building the local workspace.'
                    $package = Build-WorkspacePackage $scriptRoot $workspaceVersion
                }
            }
        }
    }
    if (-not (Test-Path -LiteralPath $package.Path -PathType Leaf) -or [IO.Path]::GetExtension($package.Path) -ine '.msi') { throw 'Choose an existing MSI package.' }
    $actual = (Get-FileHash -LiteralPath $package.Path -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumVerified = $false
    if ($Sha256 -and ($Sha256 -notmatch '^[0-9a-fA-F]{64}$' -or $actual -ine $Sha256)) { throw 'The supplied SHA-256 does not match the package.' }
    if ($package.Digest -and $actual -ne $package.Digest) { throw 'The downloaded MSI failed its SHA-256 check.' }
    if ($Sha256 -or $package.Digest) { $checksumVerified = $true }
    if (-not $package.Downloaded -and (Test-Path -LiteralPath ($package.Path + '.sha256') -PathType Leaf)) {
        if ($actual -ne (Get-Checksum ($package.Path + '.sha256') ([IO.Path]::GetFileName($package.Path)))) { throw 'The MSI failed its SHA-256 check.' }
        $checksumVerified = $true
    }
    $info = Read-MsiInfo $package.Path
    if ($package.Version -and $info.ProductVersion -ne $package.Version) { throw 'The MSI version does not match its release or workspace.' }
    Initialize-Native
    $applications = @(Get-InstalledProcesses 'markdownx' $appPath)
    $servers = @(Get-InstalledProcesses 'markdownx-server' $serverPath)
    if ($servers.Count -gt 1) { throw 'More than one installed collaboration server is running; no process was stopped.' }
    if ($servers.Count) { Assert-NoServerSessions $servers[0] }
    $windows = @()
    foreach ($application in $applications) {
        $handles = [MarkdownXInstallNative]::Windows($application.Id)
        if (-not $handles.Count) { throw 'An installed MarkdownX process has no closeable window; no process was stopped.' }
        $item = Get-CimInstance Win32_Process -Filter ("ProcessId = " + $application.Id) -ErrorAction Stop
        if (-not $item.CommandLine) { throw 'Could not preserve the running MarkdownX window arguments.' }
        $argvParts = [MarkdownXInstallNative]::Arguments($item.CommandLine)
        $tail = if ($argvParts.Count -gt 1) { @($argvParts[1..($argvParts.Count - 1)]) } else { @() }
        $windows += [pscustomobject]@{ Process = $application; Arguments = $tail; Handles = $handles }
    }
    if ($DryRun) {
        $hashStatus = if ($checksumVerified) { 'verified' } else { 'calculated (no reference supplied)' }
        Write-Host "Ready: MarkdownX $($info.ProductVersion); SHA-256 $hashStatus; $($windows.Count) app process(es); $($servers.Count) collaboration server(s)."
        return
    }
    $closed = [Collections.Generic.List[object]]::new()
    $serverStopped = $false
    $installed = $false
    $failure = $null
    try {
        foreach ($window in $windows) {
            if ($window.Process.HasExited) { continue }
            foreach ($handle in $window.Handles) { [MarkdownXInstallNative]::RequestClose($handle) }
            if (-not $window.Process.WaitForExit(30000)) { throw 'MarkdownX did not close; finish saving or dismiss its dialog and retry.' }
            $closed.Add($window)
        }
        if ($servers.Count -and -not $servers[0].HasExited) { Stop-ServerGracefully $servers[0]; $serverStopped = $true }
        $msiexec = Join-Path $env:SystemRoot 'System32\msiexec.exe'
        $quoted = [char]34 + $package.Path + [char]34
        $msi = Start-Process -FilePath $msiexec -ArgumentList @('/i', $quoted, '/qn', '/norestart', 'REBOOT=ReallySuppress', 'MSIRESTARTMANAGERCONTROL=Disable') -WindowStyle Hidden -Wait -PassThru
        if ($msi.ExitCode -eq 3010) { throw 'Windows requires a restart; the installation has not been verified.' }
        if ($msi.ExitCode -ne 0) { throw "The MSI installer failed with exit code $($msi.ExitCode)." }
        if ([MarkdownXInstallNative]::MsiQueryProductStateW($info.ProductCode) -ne 5 -or -not (Test-Path -LiteralPath $appPath -PathType Leaf)) { throw 'The installed MSI could not be verified.' }
        $installed = $true
    } catch { $failure = $_ }
    finally {
        $serverNeedsRestart = $serverStopped -or ($servers.Count -and $servers[0].HasExited) -or ($installed -and $servers.Count -eq 0)
        if ($serverNeedsRestart -and @(Get-InstalledProcesses 'markdownx-server' $serverPath).Count -eq 0) {
            try { Start-InstalledServer } catch { if (-not $failure) { $failure = $_ } else { Write-Warning 'The collaboration server could not be restarted.' } }
        }
        if (-not $NoLaunch -or -not $installed) {
            try {
                foreach ($window in $closed) { Start-InstalledApp $window.Arguments }
                if ($installed -and $applications.Count -eq 0 -and -not $NoLaunch) { Start-InstalledApp @() }
            } catch { if (-not $failure) { $failure = $_ } else { Write-Warning 'A MarkdownX window could not be reopened.' } }
        }
    }
    if ($failure) { throw $failure }
    Write-Host "MarkdownX $($info.ProductVersion) installed for the current user."
} finally {
    if ($tempRoot) {
        foreach ($name in @('package.msi', 'package.sha256')) {
            $path = Join-Path $tempRoot $name
            if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
        }
        # Antivirus scanners can briefly hold the downloaded package; retry, then
        # leave the temporary folder behind instead of failing a completed install.
        if (Test-Path -LiteralPath $tempRoot -PathType Container) {
            foreach ($attempt in 1..3) {
                try { Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction Stop } catch { }
                if (-not (Test-Path -LiteralPath $tempRoot)) { break }
                Start-Sleep -Milliseconds 150
            }
        }
    }
}
