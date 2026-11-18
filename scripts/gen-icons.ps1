# Generate Tauri bundle icons from the official OpenCode brand assets.
# Recolors the transparent "O" mark to the app accent (emerald #1FA97A) on a
# dark rounded tile so it reads correctly on Windows 10/11 light and dark themes.
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$BrandAssets = Join-Path $env:USERPROFILE "Downloads\opencode-brand-assets\OpenCode Brand Assets"
$LogoSource = Join-Path $BrandAssets "Logo Transparent\opencode-logo-transparent-dark.png"
$OutDir = Join-Path $Root "src-tauri\icons"
$UiAssetDir = Join-Path $Root "src\assets"
New-Item -ItemType Directory -Force -Path $OutDir, $UiAssetDir | Out-Null

Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $LogoSource)) {
    throw "OpenCode brand asset not found: $LogoSource"
}

$Accent = [System.Drawing.Color]::FromArgb(255, 31, 169, 122)     # #1FA97A emerald
$Tile = [System.Drawing.Color]::FromArgb(255, 12, 18, 16)         # near-black green tile
$TileLight = [System.Drawing.Color]::FromArgb(255, 238, 238, 238) # brand #EEEEEE

function Get-RoundedRectPath([int]$Width, [int]$Height, [single]$Radius) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $Radius * 2
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($Width - $d, 0, $d, $d, 270, 90)
    $path.AddArc($Width - $d, $Height - $d, $d, $d, 0, 90)
    $path.AddArc(0, $Height - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

# Recolor the transparent mark while preserving its per-pixel alpha shading.
function New-RecoloredMark([int]$Width, [int]$Height) {
    $out = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $src = New-Object System.Drawing.Bitmap($LogoSource)
    $scaleX = $Width / $src.Width
    $scaleY = $Height / $src.Height
    for ($y = 0; $y -lt $Height; $y++) {
        $sy = [int][Math]::Floor($y / $scaleY)
        if ($sy -ge $src.Height) { $sy = $src.Height - 1 }
        for ($x = 0; $x -lt $Width; $x++) {
            $sx = [int][Math]::Floor($x / $scaleX)
            if ($sx -ge $src.Width) { $sx = $src.Width - 1 }
            $p = $src.GetPixel($sx, $sy)
            if ($p.A -eq 0) {
                $out.SetPixel($x, $y, [System.Drawing.Color]::Transparent)
            } else {
                $out.SetPixel($x, $y, [System.Drawing.Color]::FromArgb($p.A, $Accent.R, $Accent.G, $Accent.B))
            }
        }
    }
    $src.Dispose()
    return $out
}

function Save-Png($Image, [string]$Path) {
    $Image.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

function Save-Ico($Bitmap, [string]$Path) {
    $handle = $Bitmap.GetHicon()
    $icon = [System.Drawing.Icon]::FromHandle($handle)
    try {
        $fs = [IO.File]::Create($Path)
        try { $icon.Save($fs) } finally { $fs.Dispose() }
    } finally {
        $icon.Dispose()
    }
}

$markHi = New-RecoloredMark 480 600

# Standalone UI mark (transparent background) for the React sidebar.
$markUi = New-RecoloredMark 512 640
Save-Png $markUi (Join-Path $UiAssetDir "logo-mark.png")

function New-TileIcon([int]$Size, [System.Drawing.Color]$TileColor) {
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.Clear([System.Drawing.Color]::Transparent)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

        $path = Get-RoundedRectPath $Size $Size ($Size * 0.22)
        $brush = New-Object System.Drawing.SolidBrush($TileColor)
        try { $g.FillPath($brush, $path) } finally { $brush.Dispose(); $path.Dispose() }

        $markH = [int]($Size * 0.74)
        $markW = [int]($markH * 0.8)
        $rect = New-Object System.Drawing.Rectangle(
            [int](($Size - $markW) / 2),
            [int](($Size - $markH) / 2),
            $markW,
            $markH
        )
        $attrs = New-Object System.Drawing.Imaging.ImageAttributes
        try {
            # Keep the source alpha exactly; the mark's built-in shading stays.
            $g.DrawImage($markHi, $rect, 0, 0, $markHi.Width, $markHi.Height, [System.Drawing.GraphicsUnit]::Pixel, $attrs)
        } finally { $attrs.Dispose() }
    } finally {
        $g.Dispose()
    }
    return $bmp
}

$darkTiles = @{}
foreach ($size in @(16, 32, 48, 128, 256, 512)) {
    $bmp = New-TileIcon $size $Tile
    if ($size -in @(32, 128, 256, 512)) {
        Save-Png $bmp (Join-Path $OutDir ("{0}x{0}.png" -f $size))
    }
    if ($size -eq 256) {
        Save-Ico $bmp (Join-Path $OutDir "icon.ico")
        Save-Png $bmp (Join-Path $OutDir "128x128@2x.png")
    }
    if ($size -eq 512) { Save-Png $bmp (Join-Path $OutDir "icon.png") }
    $darkTiles[$size] = $bmp
}

# Light-tile variant for light-theme contexts (not required by the bundle yet).
$light = New-TileIcon 256 $TileLight
Save-Png $light (Join-Path $UiAssetDir "logo-tile-light.png")

$markHi.Dispose()
$markUi.Dispose()
foreach ($k in $darkTiles.Keys) { $darkTiles[$k].Dispose() }
$light.Dispose()

Write-Host "Icons written to $OutDir and UI assets to $UiAssetDir"
