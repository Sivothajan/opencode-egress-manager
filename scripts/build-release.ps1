# Build the full release bundle:
#   1. compile the privileged helper (egress-manager-admin.exe)
#   2. stage it for Tauri's externalBin bundling
#   3. build the frontend + Tauri installer (NSIS), optionally code-signed
#   4. emit SHA-256 checksums
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Tauri = Join-Path $Root "src-tauri"
$TargetTriple = "x86_64-pc-windows-msvc"

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $sdk = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
    if ($sdk) { return $sdk.FullName }
    return $null
}

function Sign-File {
    param([Parameter(Mandatory)][string]$Path)
    $tool = Find-SignTool
    if (-not $tool) {
        Write-Warning "signtool.exe not found; skipping signing for $Path"
        return
    }
    $signArgs = @("sign", "/fd", "SHA256", "/tr", "http://timestamp.digicert.com", "/td", "SHA256")
    if ($env:OUC_SIGN_PFX) {
        $signArgs += @("/f", $env:OUC_SIGN_PFX)
        if ($env:OUC_SIGN_PASSWORD) { $signArgs += @("/p", $env:OUC_SIGN_PASSWORD) }
    } elseif ($env:OUC_SIGN_THUMBPRINT) {
        $signArgs += @("/sha1", $env:OUC_SIGN_THUMBPRINT)
    } else {
        Write-Host "No signing certificate configured; skipping $Path"
        return
    }
    $signArgs += $Path
    & $tool @signArgs
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Path (exit $LASTEXITCODE)" }
    Write-Host "Signed $Path" -ForegroundColor Green
}

# Tauri validates externalBin paths while its build script runs, which happens before the
# admin helper can be compiled. Stage a placeholder first, then overwrite it with the real PE.
& (Join-Path $PSScriptRoot "stage-admin-sidecar.ps1")

Write-Host "== Building privileged helper ==" -ForegroundColor Cyan
Push-Location $Tauri
try {
    cargo build --release --bin egress-manager-admin
} finally {
    Pop-Location
}

$AdminSource = Join-Path $Tauri "target\release\egress-manager-admin.exe"
if (-not (Test-Path $AdminSource)) { throw "admin helper was not built: $AdminSource" }
& (Join-Path $PSScriptRoot "stage-admin-sidecar.ps1") -Source $AdminSource

$Staged = Join-Path $Tauri "binaries\egress-manager-admin-$TargetTriple.exe"
$Header = [IO.File]::ReadAllBytes($Staged)[0..1]
if ($Header[0] -ne 0x4D -or $Header[1] -ne 0x5A) {
    throw "staged admin sidecar is not a PE executable: $Staged"
}

Write-Host "== Building application ==" -ForegroundColor Cyan
& bun run tauri build
if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }

Sign-File -Path (Join-Path $Tauri "target\release\opencode-egress-manager.exe")
$Installer = Get-ChildItem (Join-Path $Tauri "target\release\bundle\nsis") -Filter "*_setup.exe" |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($Installer) { Sign-File -Path $Installer.FullName }

Write-Host "== Checksums ==" -ForegroundColor Cyan
$BundleRoot = Join-Path $Tauri "target\release\bundle"
$ChecksumFile = Join-Path $Root "SHA256SUMS.txt"
$lines = @()
Get-ChildItem $BundleRoot -Recurse -File -Include *.exe,*.msi | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $relative = $_.FullName.Substring($Root.Length + 1)
    $lines += "$hash  $relative"
}
$lines | Set-Content -Path $ChecksumFile -Encoding ascii
Get-Content $ChecksumFile

Write-Host "Release artifacts ready. Checksums written to $ChecksumFile" -ForegroundColor Green
