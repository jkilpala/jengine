# Generates a procedural diffuse + matching normal-map PNG pair for the
# scene 12 (materials / normal-maps) test-lab demo.
#
# Output:
#   assets/sprites/bumps128.png      - tan tile with 4 round bumps
#   assets/sprites/bumps128_n.png    - matching tangent-space normal map
#
# Format: 128x128 RGBA8. Normal map uses the standard encoding
# (rgb = normal * 0.5 + 0.5, b = +Z) so the engine's flat-normal
# fallback (128,128,255) means "facing camera".
#
# Run from anywhere: this script writes via absolute paths.

Add-Type -AssemblyName System.Drawing

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$SpriteDir = Join-Path $ScriptDir 'sprites'
if (-not (Test-Path $SpriteDir)) { New-Item -ItemType Directory -Path $SpriteDir | Out-Null }

$W = 128
$H = 128
$Bumps = @(
    @{ cx = 32; cy = 32; r = 24 },
    @{ cx = 96; cy = 32; r = 24 },
    @{ cx = 32; cy = 96; r = 24 },
    @{ cx = 96; cy = 96; r = 24 }
)

$diffuse = New-Object System.Drawing.Bitmap($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$normal  = New-Object System.Drawing.Bitmap($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)

# Base colour: warm tan; bumps are a slightly lighter highlight.
$baseR = 180; $baseG = 150; $baseB = 110
$bumpR = 220; $bumpG = 190; $bumpB = 140

for ($y = 0; $y -lt $H; $y++) {
    for ($x = 0; $x -lt $W; $x++) {
        # Find the closest bump.
        $nx = 0.0; $ny = 0.0; $nz = 1.0
        $onBump = $false
        $bumpStrength = 0.0
        foreach ($b in $Bumps) {
            $dx = $x - $b.cx
            $dy = $y - $b.cy
            $r  = $b.r
            $d2 = $dx * $dx + $dy * $dy
            $r2 = $r * $r
            if ($d2 -le $r2) {
                $onBump = $true
                # Hemisphere height: h(x,y) = sqrt(r^2 - dx^2 - dy^2) / r
                # Normal in tangent space:
                #   grad_h = (-dx/r * 1/sqrt(...), -dy/r * 1/sqrt(...))
                # Surface normal = normalize(-grad_h_x, -grad_h_y, 1)
                # On a perfect hemisphere this simplifies to (dx, dy, sqrt(r^2-d2)) / r.
                $hz = [Math]::Sqrt($r2 - $d2)
                $len = [Math]::Sqrt($dx * $dx + $dy * $dy + $hz * $hz)
                $nx = $dx / $len
                $ny = $dy / $len
                $nz = $hz / $len
                $bumpStrength = 1.0 - ([Math]::Sqrt($d2) / $r)  # 1 at center, 0 at rim
                break
            }
        }

        # Diffuse: subtle grain via a sine pattern + bump highlight.
        $grain = [Math]::Sin($x * 0.4) * [Math]::Cos($y * 0.4) * 8.0
        if ($onBump) {
            $tr = [int]($baseR + ($bumpR - $baseR) * $bumpStrength + $grain)
            $tg = [int]($baseG + ($bumpG - $baseG) * $bumpStrength + $grain)
            $tb = [int]($baseB + ($bumpB - $baseB) * $bumpStrength + $grain)
        } else {
            $tr = [int]($baseR + $grain)
            $tg = [int]($baseG + $grain)
            $tb = [int]($baseB + $grain)
        }
        $tr = [Math]::Max(0, [Math]::Min(255, $tr))
        $tg = [Math]::Max(0, [Math]::Min(255, $tg))
        $tb = [Math]::Max(0, [Math]::Min(255, $tb))
        $diffuse.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, $tr, $tg, $tb))

        # Normal: encode tangent-space normal to RGB.
        # Note PNG y-axis goes downward; engine shader treats UV y as
        # pixel-space, so we keep the raw nx/ny (no flip).
        $nrR = [int]([Math]::Round(($nx * 0.5 + 0.5) * 255.0))
        $nrG = [int]([Math]::Round(($ny * 0.5 + 0.5) * 255.0))
        $nrB = [int]([Math]::Round(($nz * 0.5 + 0.5) * 255.0))
        $nrR = [Math]::Max(0, [Math]::Min(255, $nrR))
        $nrG = [Math]::Max(0, [Math]::Min(255, $nrG))
        $nrB = [Math]::Max(0, [Math]::Min(255, $nrB))
        $normal.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, $nrR, $nrG, $nrB))
    }
}

$diffusePath = Join-Path $SpriteDir 'bumps128.png'
$normalPath  = Join-Path $SpriteDir 'bumps128_n.png'
$diffuse.Save($diffusePath, [System.Drawing.Imaging.ImageFormat]::Png)
$normal.Save($normalPath,  [System.Drawing.Imaging.ImageFormat]::Png)
$diffuse.Dispose()
$normal.Dispose()

Write-Host "wrote $diffusePath"
Write-Host "wrote $normalPath"
