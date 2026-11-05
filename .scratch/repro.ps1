# Reproduce: start SoundShared GUI, click "Start Server", observe freeze.
# Evidence collected: screenshots, stdout log, process CPU time.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@

function Snap($hwnd, $file) {
  $rc = New-Object U+RECT
  [void][U]::GetClientRect($hwnd, [ref]$rc)
  $pt = New-Object U+POINT
  [void][U]::ClientToScreen($hwnd, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  return @($rc.R, $rc.B, $pt.X, $pt.Y)
}

function FindRose($file, $xFromFrac) {
  $bmp = New-Object System.Drawing.Bitmap($file)
  $x0 = [int]($bmp.Width * $xFromFrac)
  $minX = 99999; $minY = 99999; $maxX = -1; $maxY = -1; $n = 0
  for ($y = 0; $y -lt $bmp.Height; $y += 2) {
    for ($x = $x0; $x -lt $bmp.Width; $x += 2) {
      $c = $bmp.GetPixel($x, $y)
      if ($c.R -gt 200 -and $c.G -lt 70 -and $c.B -gt 50 -and $c.B -lt 110) {
        $n++
        if ($x -lt $minX) { $minX = $x }; if ($x -gt $maxX) { $maxX = $x }
        if ($y -lt $minY) { $minY = $y }; if ($y -gt $maxY) { $maxY = $y }
      }
    }
  }
  $bmp.Dispose()
  if ($n -eq 0) { return $null }
  return @([int](($minX+$maxX)/2), [int](($minY+$maxY)/2), $n, $minX, $minY, $maxX, $maxY)
}

function ClickAt($hwnd, $lx, $ly) {
  $pt = New-Object U+POINT
  $pt.X = $lx; $pt.Y = $ly
  [void][U]::ClientToScreen($hwnd, [ref]$pt)
  [void][U]::SetCursorPos($pt.X, $pt.Y)
  Start-Sleep -Milliseconds 250
  [U]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
  Start-Sleep -Milliseconds 80
  [U]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
  Start-Sleep -Milliseconds 250
}

Get-Process soundshared -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\debug\soundshared.exe'
if (-not (Test-Path $exe)) { "MISSING: $exe"; exit 1 }
$log = Join-Path $PSScriptRoot 'run.log'
if (Test-Path $log) { Remove-Item $log -Force }

$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $log -RedirectStandardError (Join-Path $PSScriptRoot 'run.err.log')
"started pid=$($p.Id)"

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  $p.Refresh()
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }
}
if ($h -eq [IntPtr]::Zero) { "NO WINDOW"; Get-Content $log -Tail 20; exit 1 }

[void][U]::ShowWindow($h, 9)
[void][U]::MoveWindow($h, 40, 40, 1296, 839, $true)
[void][U]::SetForegroundWindow($h)
Start-Sleep -Seconds 3

$base = Join-Path $PSScriptRoot 'gui_base.png'
$geo = Snap $h $base
"client=$($geo[0])x$($geo[1]) origin=($($geo[2]),$($geo[3]))"

$rose = FindRose $base 0.6
if ($null -eq $rose) { "NO ROSE BUTTON FOUND in right half"; exit 1 }
"primary button center client=($($rose[0]),$($rose[1])) bbox x[$($rose[3])..$($rose[5])] y[$($rose[4])..$($rose[6])]"

$t0 = $p.TotalProcessorTime
ClickAt $h $rose[0] $rose[1]
Start-Sleep -Seconds 4

$p.Refresh()
$t1 = $p.TotalProcessorTime
"CPU delta over 4s: $([math]::Round(($t1 - $t0).TotalSeconds, 2))s   responding=$($p.Responding)  threads=$($p.Threads.Count)"

$after = Join-Path $PSScriptRoot 'gui_after_start.png'
$null = Snap $h $after

# Second click in the same place (would be "Stop Server" if the UI reacted)
ClickAt $h $rose[0] $rose[1]
Start-Sleep -Seconds 4
$p.Refresh()
$t2 = $p.TotalProcessorTime
"CPU delta 2nd window: $([math]::Round(($t2 - $t1).TotalSeconds, 2))s   responding=$($p.Responding)"
$after2 = Join-Path $PSScriptRoot 'gui_after_stop.png'
$null = Snap $h $after2

"=== stdout tail ==="
if (Test-Path $log) { Get-Content $log -Tail 30 }
"=== stderr tail ==="
$errl = Join-Path $PSScriptRoot 'run.err.log'
if (Test-Path $errl) { Get-Content $errl -Tail 15 }
