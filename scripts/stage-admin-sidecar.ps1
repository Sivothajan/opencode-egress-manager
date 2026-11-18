# Ensures Tauri's externalBin resource exists before cargo runs the build script.
#
# The admin helper is built from this same crate, but Tauri validates externalBin paths
# during the build-script phase, before that binary can exist on a clean checkout. Pass
# -Source to stage the freshly built helper; without it an empty placeholder is created so
# cargo commands work. Release builds always overwrite it with the real PE and verify it.
param(
    [string]$Source = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Dir = Join-Path $Root "src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $Dir | Out-Null

$Target = Join-Path $Dir "egress-manager-admin-x86_64-pc-windows-msvc.exe"

if ($Source -and (Test-Path $Source)) {
    Copy-Item $Source $Target -Force
    Write-Host "Staged admin helper from $Source"
} elseif (-not (Test-Path $Target)) {
    New-Item -ItemType File -Path $Target | Out-Null
    Write-Host "Created externalBin placeholder at $Target"
}

Write-Host "externalBin ready: $Target"
