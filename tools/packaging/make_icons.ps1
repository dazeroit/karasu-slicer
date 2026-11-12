# The application icon in every size the packages need, from the owner's master (2026-11-09:
# `karasu_icon_master_v2.png`, 1254 x 1254 with a transparent background, kept by the owner, not
# in the repository).
#
# - Crop: the emblem's bounding box (pixels at least 1/16 opaque) is centred in a square where it
#   spans $Fill of the side.
# - Outline (optional, $Outline of the side, 0 for none): a $Colour contour following the
#   emblem's silhouette, at least one pixel wide: the emblem drawn in that colour at offsets
#   around circles of that radius, under the emblem.
# - Each size is resampled from the master with high-quality bicubic filtering; sizes up to 48
#   pixels are then sharpened with an unsharp mask of $Sharpen (colour only; alpha is kept).
# Run on Windows (System.Drawing); tools/packaging/make_ico.py then packs the Windows icon.
param(
    [Parameter(Mandatory = $true)][string]$Master,
    [string]$Folder = '',
    [double]$Fill = 0.96,
    [double]$Outline = 0,
    [double]$Sharpen = 0.6,
    [string]$Colour = '#C9B2FF'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
# The default is resolved here: Windows PowerShell leaves $PSScriptRoot empty in the parameter
# defaults of a script with a mandatory parameter.
if (-not $Folder) { $Folder = Join-Path $PSScriptRoot '..\..\assets\ui\icon' }
$Folder = [System.IO.Path]::GetFullPath($Folder)

function Read-Pixels($bitmap) {
    $rect = New-Object System.Drawing.Rectangle(0, 0, $bitmap.Width, $bitmap.Height)
    $data = $bitmap.LockBits($rect, 'ReadOnly', [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bytes = New-Object byte[] ($data.Stride * $bitmap.Height)
    [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
    $bitmap.UnlockBits($data)
    return , $bytes
}

function Write-Pixels($bitmap, [byte[]]$bytes) {
    $rect = New-Object System.Drawing.Rectangle(0, 0, $bitmap.Width, $bitmap.Height)
    $data = $bitmap.LockBits($rect, 'WriteOnly', [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    [System.Runtime.InteropServices.Marshal]::Copy($bytes, 0, $data.Scan0, $bytes.Length)
    $bitmap.UnlockBits($data)
}

# Unsharp mask on the colour channels: each pixel moved away from its 3 x 3 neighbourhood's
# alpha-weighted mean by $amount. Bytes are blue, green, red, alpha.
function Sharpen([byte[]]$bytes, [int]$size, [double]$amount) {
    $out = [byte[]]$bytes.Clone()
    for ($y = 0; $y -lt $size; $y++) {
        for ($x = 0; $x -lt $size; $x++) {
            $i = 4 * ($y * $size + $x)
            if ($bytes[$i + 3] -eq 0) { continue }
            $sum = @(0.0, 0.0, 0.0); $weight = 0.0
            for ($dy = -1; $dy -le 1; $dy++) {
                for ($dx = -1; $dx -le 1; $dx++) {
                    $nx, $ny = ($x + $dx), ($y + $dy)
                    if ($nx -lt 0 -or $ny -lt 0 -or $nx -ge $size -or $ny -ge $size) { continue }
                    $j = 4 * ($ny * $size + $nx)
                    $w = $bytes[$j + 3] / 255.0
                    for ($c = 0; $c -lt 3; $c++) { $sum[$c] += $w * $bytes[$j + $c] }
                    $weight += $w
                }
            }
            for ($c = 0; $c -lt 3; $c++) {
                $value = $bytes[$i + $c] + $amount * ($bytes[$i + $c] - $sum[$c] / $weight)
                $out[$i + $c] = [byte][Math]::Min(255, [Math]::Max(0, [Math]::Round($value)))
            }
        }
    }
    return , $out
}

function New-Graphics($bitmap) {
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.Clear([System.Drawing.Color]::Transparent)
    return $graphics
}

$tint = [System.Drawing.ColorTranslator]::FromHtml($Colour)
$emblemImage = New-Object System.Drawing.Bitmap([System.IO.Path]::GetFullPath($Master))
try {
    # The emblem's bounding box.
    $pixels = Read-Pixels $emblemImage
    $width = $emblemImage.Width
    $minX, $minY, $maxX, $maxY = $width, $emblemImage.Height, -1, -1
    for ($y = 0; $y -lt $emblemImage.Height; $y++) {
        for ($x = 0; $x -lt $width; $x++) {
            if ($pixels[4 * ($y * $width + $x) + 3] -ge 16) {
                if ($x -lt $minX) { $minX = $x }; if ($x -gt $maxX) { $maxX = $x }
                if ($y -lt $minY) { $minY = $y }; if ($y -gt $maxY) { $maxY = $y }
            }
        }
    }
    $emblem = [Math]::Max($maxX - $minX + 1, $maxY - $minY + 1)
    $cx, $cy = (($minX + $maxX + 1) / 2.0), (($minY + $maxY + 1) / 2.0)
    # The emblem in the outline colour, alpha kept.
    $solid = New-Object System.Drawing.Imaging.ColorMatrix
    $solid.Matrix00 = 0; $solid.Matrix11 = 0; $solid.Matrix22 = 0
    $solid.Matrix40 = $tint.R / 255.0; $solid.Matrix41 = $tint.G / 255.0; $solid.Matrix42 = $tint.B / 255.0
    $tinted = New-Object System.Drawing.Imaging.ImageAttributes
    $tinted.SetColorMatrix($solid)
    $tinted.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::Clamp)
    $plain = New-Object System.Drawing.Imaging.ImageAttributes
    $plain.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::Clamp)
    $source = New-Object System.Drawing.Rectangle(0, 0, $emblemImage.Width, $emblemImage.Height)
    foreach ($size in 16, 24, 32, 48, 64, 128, 256, 512) {
        $radius = if ($Outline -gt 0) { [Math]::Max(1.0, $Outline * $size) } else { 0.0 }
        # Master pixels to icon pixels: the emblem spans $Fill of the side less the outline.
        $scale = ($Fill * $size - 2 * $radius) / $emblem
        $left = $size / 2.0 - $cx * $scale
        $top = $size / 2.0 - $cy * $scale
        $bitmap = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = New-Graphics $bitmap
        $place = {
            param($dx, $dy, $attributes)
            $x0, $y0 = ($left + $dx), ($top + $dy)
            $corners = [System.Drawing.PointF[]]@(
                (New-Object System.Drawing.PointF([single]$x0, [single]$y0)),
                (New-Object System.Drawing.PointF([single]($x0 + $emblemImage.Width * $scale), [single]$y0)),
                (New-Object System.Drawing.PointF([single]$x0, [single]($y0 + $emblemImage.Height * $scale))))
            $graphics.DrawImage($emblemImage, $corners, $source, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
        }
        # The outline: the tinted emblem around circles up to the radius, then the emblem.
        $rings = if ($Outline -gt 0) { [Math]::Max(1, [Math]::Ceiling($radius)) } else { 0 }
        for ($ring = 1; $ring -le $rings; $ring++) {
            $r = $radius * $ring / $rings
            $steps = [Math]::Max(12, [Math]::Ceiling(2 * [Math]::PI * $r * 2))
            for ($k = 0; $k -lt $steps; $k++) {
                $angle = 2 * [Math]::PI * $k / $steps
                & $place ($r * [Math]::Cos($angle)) ($r * [Math]::Sin($angle)) $tinted
            }
        }
        & $place 0 0 $plain
        $graphics.Dispose()
        if ($size -le 48 -and $Sharpen -gt 0) {
            Write-Pixels $bitmap (Sharpen (Read-Pixels $bitmap) $size $Sharpen)
        }
        $bitmap.Save((Join-Path $Folder "karasu-$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        $bitmap.Dispose()
    }
} finally {
    $emblemImage.Dispose()
}
