Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Bitmap]::FromFile((Resolve-Path '.scratch\audit-gui.png').Path)
$W = $img.Width; $H = $img.Height
Write-Host "image ${W}x${H}"

function Kind($c) {
  # classify a pixel into one of the known surfaces
  if ($c.R -eq 10 -and $c.G -eq 10 -and $c.B -eq 10) { return 'APP' }
  if ($c.R -eq 17 -and $c.G -eq 17 -and $c.B -eq 17) { return 'PANEL' }
  if ($c.R -eq 9  -and $c.G -eq 9  -and $c.B -eq 11) { return 'WELL' }
  if ($c.R -eq 24 -and $c.G -eq 24 -and $c.B -eq 27) { return 'ROW' }
  if ($c.R -eq 39 -and $c.G -eq 39 -and $c.B -eq 42) { return 'BORD' }
  if ($c.R -eq 63 -and $c.G -eq 63 -and $c.B -eq 70) { return 'BORD7' }
  if ($c.R -eq 225 -and $c.G -eq 29 -and $c.B -eq 72) { return 'ROSE6' }
  if ($c.R -eq 244 -and $c.G -eq 63 -and $c.B -eq 94) { return 'ROSE5' }
  return 'other'
}

function RowScan($y) {
  $runs = @()
  $prev = $null; $start = 0
  for ($x = 0; $x -lt $W; $x++) {
    $c = Kind $img.GetPixel($x, $y)
    if ($c -ne $prev) {
      if ($null -ne $prev) { $runs += "$prev@$start+$($x-$start)" }
      $prev = $c; $start = $x
    }
  }
  $runs += "$prev@$start+$($W-$start)"
  # drop the noisy 'other' runs, keep structural ones
  return ($runs | Where-Object { $_ -notmatch '^other@\d+\+[0-9]$' }) -join '  '
}

function ColScan($x) {
  $runs = @()
  $prev = $null; $start = 0
  for ($y = 0; $y -lt $H; $y++) {
    $c = Kind $img.GetPixel($x, $y)
    if ($c -ne $prev) {
      if ($null -ne $prev) { $runs += "$prev@$start+$($y-$start)" }
      $prev = $c; $start = $y
    }
  }
  $runs += "$prev@$start+$($H-$start)"
  return ($runs | Where-Object { $_ -notmatch '^other@\d+\+[0-9]$' }) -join '  '
}

Write-Host ""
Write-Host "=== COLUMN SCANS (x -> vertical structure) ==="
foreach ($x in 5, 12, 24, 30, 48, 60, 200, 400, 700, 900, 1000, 1030, 1040, 1048, 1100, 1300, 1370) {
  if ($x -lt $W) { Write-Host ("col x={0,-5} {1}" -f $x, (ColScan $x)) }
}

Write-Host ""
Write-Host "=== ROW SCANS (y -> horizontal structure) ==="
foreach ($y in 2, 10, 32, 60, 63, 64, 65, 66, 80, 120, 200, 300, 400, 500, 600, 700, 730, 760, 762, 765, 780, 787) {
  if ($y -lt $H) { Write-Host ("row y={0,-5} {1}" -f $y, (RowScan $y)) }
}
$img.Dispose()
