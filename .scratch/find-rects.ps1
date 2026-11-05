# Find axis-aligned solid rectangles of a given exact colour in a PNG.
param([string]$Path, [string]$Hex, [int]$MinW = 4, [int]$MinH = 4)
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Path).Path)
$W = $img.Width; $H = $img.Height
Write-Host "file: $Path  ${W}x${H}  colour #$Hex"

$target = [System.Drawing.ColorTranslator]::FromHtml("#$Hex")
$isT = New-Object 'bool[,]' $W, $H
for ($y = 0; $y -lt $H; $y++) {
  for ($x = 0; $x -lt $W; $x++) {
    $c = $img.GetPixel($x, $y)
    $isT[$x, $y] = ($c.R -eq $target.R -and $c.G -eq $target.G -and $c.B -eq $target.B)
  }
}

$rects = New-Object System.Collections.ArrayList
for ($y = 0; $y -lt $H; $y++) {
  for ($x = 0; $x -lt $W; $x++) {
    if (-not $isT[$x, $y]) { continue }
    if ($x -gt 0 -and $isT[($x-1), $y]) { continue }
    if ($y -gt 0 -and $isT[$x, ($y-1)]) { continue }
    # measure the run right and down
    $w = 0; while (($x + $w) -lt $W -and $isT[($x+$w), $y]) { $w++ }
    $h = 0; while (($y + $h) -lt $H -and $isT[$x, ($y+$h)]) { $h++ }
    if ($w -ge $MinW -and $h -ge $MinH) {
      [void]$rects.Add([pscustomobject]@{ x = $x; y = $y; w = $w; h = $h; area = $w * $h })
    }
  }
}

$sorted = $rects | Sort-Object -Property area -Descending
Write-Host "rects: $($sorted.Count)"
foreach ($r in $sorted) {
  Write-Host ("  x={0,-5} y={1,-5} w={2,-5} h={3,-5}  right={4,-5} bottom={5}" -f $r.x, $r.y, $r.w, $r.h, ($r.x + $r.w - 1), ($r.y + $r.h - 1))
}
$img.Dispose()
