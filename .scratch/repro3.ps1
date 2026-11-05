# Verify the panels fit: capture at exactly 1280x800 client and at the 960x620 minimum.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U3 {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@

function Snap($hwnd, $file) {
  $rc = New-Object U3+RECT
  [void][U3]::GetClientRect($hwnd, [ref]$rc)
  $pt = New-Object U3+POINT
  [void][U3]::ClientToScreen($hwnd, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  return @($rc.R, $rc.B)
}

function FitClient($hwnd, $wantW, $wantH) {
  $w = $wantW + 16; $h = $wantH + 39
  for ($i = 0; $i -lt 6; $i++) {
    [void][U3]::MoveWindow($hwnd, 40, 40, $w, $h, $true)
    Start-Sleep -Milliseconds 500
    $rc = New-Object U3+RECT
    [void][U3]::GetClientRect($hwnd, [ref]$rc)
    $dw = $wantW - $rc.R; $dh = $wantH - $rc.B
    if ([Math]::Abs($dw) -le 1 -and [Math]::Abs($dh) -le 1) { return @($rc.R, $rc.B) }
    $w += $dw; $h += $dh
  }
  $rc = New-Object U3+RECT
  [void][U3]::GetClientRect($hwnd, [ref]$rc)
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
[void][U3]::ShowWindow($h, 9)
[void][U3]::SetForegroundWindow($h)

$g1 = FitClient $h 1280 800
"client @ default: $($g1[0])x$($g1[1])"
Start-Sleep -Seconds 2
Snap $h (Join-Path $PSScriptRoot 'fit_1280x800.png') | Out-Null

$g2 = FitClient $h 960 620
"client @ minimum: $($g2[0])x$($g2[1])"
Start-Sleep -Seconds 2
Snap $h (Join-Path $PSScriptRoot 'fit_960x620.png') | Out-Null

"responding=$($p.Responding)"
"done"
