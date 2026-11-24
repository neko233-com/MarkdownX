# MarkdownX — local Windows package (PowerShell 5+)
#
# Produces the same shape as the release pipeline:
#   dist/release/windows-amd64/MarkdownX.exe
#   dist/release/windows-amd64/markdownx-setup-<version>-amd64.exe
#   dist/release/markdownx-cli-<version>-windows-amd64.zip
#   dist/release/SHA256SUMS
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/package.ps1
param([string]$Version = "")

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$cfg = Get-Content "mygo.json" -Raw | ConvertFrom-Json
if (-not $Version) { $Version = $cfg.version }
if ($Version -ne $cfg.version) {
    throw "Version $Version does not match mygo.json ($($cfg.version))."
}

$env:CGO_ENABLED = "0"
$rel = Join-Path (Get-Location) "dist/release"
New-Item -ItemType Directory -Force -Path $rel | Out-Null
Get-ChildItem $rel -File | Remove-Item -Force -ErrorAction SilentlyContinue

Write-Host "==> go vet" -ForegroundColor Cyan
go vet ./...
if ($LASTEXITCODE -ne 0) { throw "go vet failed" }

Write-Host "==> go test" -ForegroundColor Cyan
go test ./...
if ($LASTEXITCODE -ne 0) { throw "go test failed" }

Write-Host "==> go tool mygo build" -ForegroundColor Cyan
go tool mygo build -platform windows/amd64 -o $rel
if ($LASTEXITCODE -ne 0) { throw "mygo build failed" }

Write-Host "==> command-line tools" -ForegroundColor Cyan
$cli = Join-Path $rel "cli"
New-Item -ItemType Directory -Force -Path $cli | Out-Null
$bins = @(
    @{ Dir = "markdownx-server";  Name = "markdownx-server.exe" },
    @{ Dir = "markdownx-convert"; Name = "mdx-convert.exe" },
    @{ Dir = "mdx-enc";           Name = "mdx-enc.exe" }
)
foreach ($b in $bins) {
    go build -trimpath -ldflags "-s -w" -o (Join-Path $cli $b.Name) "./cmd/$($b.Dir)"
    if ($LASTEXITCODE -ne 0) { throw "build $($b.Name) failed" }
}
foreach ($doc in @("README.md", "README-CN.md", "LICENSE")) {
    if (Test-Path $doc) { Copy-Item $doc $cli -Force }
}
$zip = Join-Path $rel "markdownx-cli-$Version-windows-amd64.zip"
Compress-Archive -Path (Join-Path $cli "*") -DestinationPath $zip -Force
Remove-Item -Recurse -Force $cli

$assets = Get-ChildItem $rel -Recurse -File | Where-Object { $_.Name -ne "SHA256SUMS" }
$lines = foreach ($a in $assets) {
    $hash = (Get-FileHash $a.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $name = $a.FullName.Substring($rel.Length).TrimStart('\', '/')
    "$hash  $name"
}
[System.IO.File]::WriteAllLines((Join-Path $rel "SHA256SUMS"), $lines, (New-Object System.Text.UTF8Encoding($false)))

Write-Host ""
Write-Host "==> 产物" -ForegroundColor Cyan
Get-ChildItem $rel -Recurse -File | ForEach-Object {
    "{0,-58} {1,8:N2} MB" -f $_.FullName.Substring($rel.Length).TrimStart('\', '/'), ($_.Length / 1MB)
}
