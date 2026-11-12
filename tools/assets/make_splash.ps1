# The startup splash's artwork, assets/ui/images/splash.png, from the owner's artwork
# (1672 x 941, which carries the product name; kept by the owner, not in the repository).
#
# The master is resampled with high-quality bicubic filtering to cover 1440 x 810 pixels, twice
# the splash's 720 x 405 points so it stays sharp on high-DPI displays, centred and cropped by at
# most a pixel. Square and opaque, as the application's splash expects (TD-141,
# crates/karasu-app/src/splash.rs, docs/UI-DESIGN.md). Run on Windows (System.Drawing).
param(
    [Parameter(Mandatory = $true)][string]$Master,
    [string]$Output = ''
)
$ErrorActionPreference = 'Stop'
# Resolved here: Windows PowerShell leaves $PSScriptRoot empty in the parameter defaults of a
# script with a mandatory parameter.
if (-not $Output) { $Output = Join-Path $PSScriptRoot '..\..\assets\ui\images\splash.png' }
Add-Type -AssemblyName System.Drawing
$W, $H = 1440, 810
$source = [System.Drawing.Image]::FromFile([System.IO.Path]::GetFullPath($Master))
$bitmap = New-Object System.Drawing.Bitmap($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
try {
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
    # Edge pixels sampled from inside the image, so the border stays opaque.
    $attributes = New-Object System.Drawing.Imaging.ImageAttributes
    $attributes.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
    # Cover the target: the larger scale, the source rectangle centred.
    $scale = [Math]::Max($W / $source.Width, $H / $source.Height)
    $sw, $sh = ($W / $scale), ($H / $scale)
    $sx, $sy = (($source.Width - $sw) / 2), (($source.Height - $sh) / 2)
    $graphics.DrawImage($source, (New-Object System.Drawing.Rectangle(0, 0, $W, $H)),
        [single]$sx, [single]$sy, [single]$sw, [single]$sh, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
    $graphics.Dispose()
    $bitmap.Save([System.IO.Path]::GetFullPath($Output), [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $bitmap.Dispose()
    $source.Dispose()
}
