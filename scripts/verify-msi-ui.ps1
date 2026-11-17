param([Parameter(Mandatory=$true)][string]$MsiPath)

# Inspect the MSI tables directly; this does not install or run the package.
# WiX dark.exe may report DARK1059 while decompiling valid ControlEvent rows.
$ErrorActionPreference = 'Stop'
$path = (Resolve-Path -LiteralPath $MsiPath).Path
$installer = New-Object -ComObject WindowsInstaller.Installer
$db = $installer.OpenDatabase($path, 0)
$controls = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$view = $db.OpenView('SELECT `Dialog_`, `Control` FROM `Control`')
try {
    $view.Execute()
    while ($row = $view.Fetch()) { [void]$controls.Add($row.StringData(1) + '/' + $row.StringData(2)) }
} finally { $view.Close() }
$view = $db.OpenView('SELECT `Dialog_`, `Control_`, `Event` FROM `ControlEvent`')
$missing = [Collections.Generic.List[string]]::new()
$total = 0
try {
    $view.Execute()
    while ($row = $view.Fetch()) {
        $total++
        $key = $row.StringData(1) + '/' + $row.StringData(2)
        if (-not $controls.Contains($key)) { $missing.Add($key + ': ' + $row.StringData(3)) }
    }
} finally { $view.Close() }
if ($total -eq 0) { throw 'MSI has no UI control events.' }
if ($missing.Count) { throw "MSI contains control events without controls: $($missing -join ', ')" }
Write-Output "Verified $total MSI control events against the Control table."

# Verify the shipped database, not just the WiX source. The existing .md
# default is deliberately not overwritten; Windows lets the user choose.
$registry = @{}
$view = $db.OpenView('SELECT `Root`, `Key`, `Name`, `Value` FROM `Registry`')
try {
    $view.Execute()
    while ($row = $view.Fetch()) {
        if ($row.IntegerData(1) -eq 1) { $registry[$row.StringData(2) + '|' + $row.StringData(3)] = $row.StringData(4) }
    }
} finally { $view.Close() }
$expected = @{
    'Software\Classes\.md\OpenWithProgids|MarkdownX.Markdown' = ''
    'Software\Classes\MarkdownX.Markdown\shell\open\command|' = '"[INSTALLDIR]markdownx.exe" "%1"'
    'Software\MarkdownX\Capabilities\FileAssociations|.md' = 'MarkdownX.Markdown'
    'Software\RegisteredApplications|MarkdownX' = 'Software\MarkdownX\Capabilities'
}
foreach ($key in $expected.Keys) {
    if (-not $registry.ContainsKey($key) -or $registry[$key] -cne $expected[$key]) { throw "MSI Markdown registration is missing or invalid: $key" }
}
if ($registry.ContainsKey('Software\Classes\.md|')) { throw 'MSI must not overwrite the existing .md default.' }
Write-Output 'Verified MSI Markdown default-app registration.'
