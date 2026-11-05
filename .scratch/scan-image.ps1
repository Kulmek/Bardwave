Add-Type -AssemblyName System.Drawing
$path = (Resolve-Path $args[0]).Path
$img = [System.Drawing.Bitmap]::FromFile($path)
$W = $img.Width; $H = $img.Height
Write-Host "file: $path"
Write-Host "image ${W}x${H}"

$counts = @{}
for ($y = 0; $y -lt $H; $y += 2) {
  for ($x = 0; $x -lt $W; $x += 2) {
    $c = $img.GetPixel($x, $y)
    $k = '{0:X2}{1:X2}{2:X2}' -f $c.R, $c.G, $c.B
    if ($counts.ContainsKey($k)) { $counts[$k]++ } else { $counts[$k] = 1 }
  }
}
Write-Host "=== top 14 colors (sampled every 2px) ==="
$counts.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 14 | ForEach-Object { Write-Host ("  #{0}  {1}" -f $_.Key, $_.Value) }

function Kind($c) {
  $k = '{0:X2}{1:X2}{2:X2}' -f $c.R, $c.G, $c.B
  switch ($k) {
    '0A0A0A' { return 'APP' }
    '111111' { return 'PANEL' }
    '09090B' { return 'WELL' }
    '18181B' { return 'ROW' }
    '27272A' { return 'BORD' }
    '3F3F46' { return 'BORD7' }
    'E11D48' { return 'ROSE6' }
    'F43F5E' { return 'ROSE5' }
    '000000' { return 'BLACK' }
    default { return 'other' }
  }
}

function RowScan($y) {
  $runs = @(); $prev = $null; $start = 0
  for ($x = 0; $x -lt $W; $x++) {
    $c = Kind $img.GetPixel($x, $y)
    if ($c -ne $prev) { if ($null -ne $prev) { $runs += "$prev@$start+$($x-$start)" }; $prev = $c; $start = $x }
  }
  $runs += "$prev@$start+$($W-$start)"
  return (($runs | Where-Object { $_ -notmatch '^(other|BLACK)@\d+\+[0-9]$' }) -join '  ')
}

function ColScan($x) {
  $runs = @(); $prev = $null; $start = 0
  for ($y = 0; $y -lt $H; $y++) {
    $c = Kind $img.GetPixel($x, $y)
    if ($c -ne $prev) { if ($null -ne $prev) { $runs += "$prev@$start+$($y-$start)" }; $prev = $c; $start = $y }
  }
  $runs += "$prev@$start+$($H-$start)"
  return (($runs | Where-Object { $_ -notmatch '^(other|BLACK)@\d+\+[0-9]$' }) -join '  ')
}

Write-Host ""
Write-Host "=== COLUMN SCANS ==="
$xs = @(0, 1, 2, 3, 10, 20, 24, 25, 48, 100, $([int]($W*0.35)), $([int]($W*0.5)), $([int]($W*0.62)), $([int]($W*0.64)), $([int]($W*0.66)), $([int]($W*0.7)), ($W-40), ($W-30), ($W-25), ($W-20), ($W-10), ($W-2), ($W-1))
foreach ($x in ($xs | Sort-Object -Unique)) { if ($x -ge 0 -and $x -lt $W) { Write-Host ("col x={0,-5} {1}" -f $x, (ColScan $x)) } }

Write-Host ""
Write-Host "=== ROW SCANS ==="
$ys = @(0, 1, 30, 55, 60, 62, 63, 64, 65, 70, 100, 150, 200, 300, 400, 500, 600, ($H-60), ($H-31), ($H-30), ($H-29), ($H-20), ($H-4), ($H-2), ($H-1))
foreach ($y in ($ys | Sort-Object -Unique)) { if ($y -ge 0 -and $y -lt $H) { Write-Host ("row y={0,-5} {1}" -f $y, (RowScan $y)) } }
$img.Dispose()
