#requires -Version 7.0
# Compatibility entry point; the repository-root installer is self-contained.
[CmdletBinding()]
param(
    [string]$MsiPath,
    [string]$Sha256,
    [string]$ReleaseApi,
    [switch]$NoLaunch,
    [switch]$DryRun
)
$root = Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'install.ps1') @PSBoundParameters
