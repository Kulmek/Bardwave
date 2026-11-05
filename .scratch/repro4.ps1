# Capture the app's client area via PrintWindow so other windows cannot occlude it.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U4 {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@

function Snap($hwnd, $file) {
  $rc = New-Object U4+RECT
  [void][U4]::GetClientRect($hwnd, [ref]$rc)
  $w = $rc.R; $h = $rc.B
  $bmp = New-Object System.Drawing.Bitmap($w, $h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  # PW_CLIENTONLY (1) | PW_RENDERFULLCONTENT (2)
  $ok = [U4]::PrintWindow($hwnd, $hdc, 3)
  $g.ReleaseHdc($hdc)
  $g.Dispose()

  # Did we get real content? Sample a grid; a pure-black result means PrintWindow failed.
  $nonBlack = 0
  for ($y = 0; $y -lt $h; $y += 40) {
    for ($x = 0; $x -lt $w; $x += 40) {
      $c = $bmp.GetPixel($x, $y)
      if ($c.R + $c.G + $c.B -gt 40) { $nonBlack++ }
    }
  }
  $method = "PrintWindow"
  if (-not $ok -or $nonBlack -lt 5) {
    # Fall back to a screen grab of the client rect.
    $pt = New-Object U4+POINT
    [void][U4]::ClientToScreen($hwnd, [ref]$pt)
    $bmp2 = New-Object System.Drawing.Bitmap($w, $h)
    $g2 = [System.Drawing.Graphics]::FromImage($bmp2)
    $g2.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($w, $h)))
    $g2.Dispose()
    $bmp.Dispose()
    $bmp = $bmp2
    $method = "CopyFromScreen"
  }
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  return @($w, $h, $method, $nonBlack)
}

function FitClient($hwnd, $wantW, $wantH) {
  $w = $wantW + 16; $h = $wantH + 39
  for ($i = 0; $i -lt 6; $i++) {
    [void][U4]::MoveWindow($hwnd, 40, 40, $w, $h, $true)
    Start-Sleep -Milliseconds 500
    $rc = New-Object U4+RECT
    [void][U4]::GetClientRect($hwnd, [ref]$rc)
    $dw = $wantW - $rc.R; $dh = $wantH - $rc.B
    if ([Math]::Abs($dw) -le 1 -and [Math]::Abs($dh) -le 1) { return @($rc.R, $rc.B) }
    $w += $dw; $h += $dh
  }
  $rc = New-Object U4+RECT
  [void][U4]::GetClientRect($hwnd, [ref]$rc)
  return @($rc.R, $rc.B)
}

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\debug\soundshared.exe'
Get-Process soundshared -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput (Join-Path $PSScriptRoot 'fit.log') -RedirectStandardError (Join-Path $PSScriptRoot 'fit.err.log')

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  $p.Refresh()
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }
}
if ($h -eq [IntPtr]::Zero) { "NO WINDOW"; exit 1 }
[void][U4]::ShowWindow($h, 9)
[void][U4]::SetForegroundWindow($h)

$g1 = FitClient $h 1280 800
Start-Sleep -Seconds 2
$r1 = Snap $h (Join-Path $PSScriptRoot 'fit_1280x800.png')
"default: client $($g1[0])x$($g1[1])  capture=$($r1[2])  contentSamples=$($r1[3])"

$g2 = FitClient $h 960 620
Start-Sleep -Seconds 2
$r2 = Snap $h (Join-Path $PSScriptRoot 'fit_960x620.png')
"minimum: client $($g2[0])x$($g2[1])  capture=$($r2[2])  contentSamples=$($r2[3])"
"responding=$($p.Responding)"
