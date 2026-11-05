# Capture the SoundShared console client area (CONFIGURATION verification).
# Usage: pwsh -File .scratch\server-config-shot.ps1 -Out cfg-a.png [-ClickX 940 -ClickY 560] [-Keys '9123']
param(
  [string]$Out = 'cfg-shot.png',
  [int]$ClickX = -1,
  [int]$ClickY = -1,
  [string]$Keys = '',
  [switch]$Kill
)
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@
[void][W]::SetProcessDPIAware()

$exe = (Resolve-Path (Join-Path $PSScriptRoot '..\target\release\soundshared.exe')).Path
$existing = Get-Process -Name soundshared -ErrorAction SilentlyContinue
if ($existing) {
  $proc = $existing | Select-Object -First 1
} else {
  $proc = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -PassThru
}
Start-Sleep -Seconds 4

$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 40; $i++) {
  if (-not (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue)) { break }
  $p = Get-Process -Id $proc.Id
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $p.MainWindowHandle; break }
  Start-Sleep -Milliseconds 250
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Host "NO WINDOW (pid $($proc.Id))"; exit 1 }

[void][W]::ShowWindow($hwnd, 5)
[void][W]::SetWindowPos($hwnd, (New-Object IntPtr(-1)), 0, 0, 0, 0, 0x0001 -bor 0x0002)
[void][W]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 700

$rc = New-Object W+RECT
[void][W]::GetClientRect($hwnd, [ref]$rc)
$pt = New-Object W+POINT
[void][W]::ClientToScreen($hwnd, [ref]$pt)
Write-Host "client ${($rc.R)}x$($rc.B) at screen ($($pt.X),$($pt.Y))"

if ($ClickX -ge 0 -and $ClickY -ge 0) {
  [void][W]::SetCursorPos($pt.X + $ClickX, $pt.Y + $ClickY)
  Start-Sleep -Milliseconds 250
  [W]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)  # left down
  [W]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)  # left up
  Start-Sleep -Milliseconds 400
  Write-Host "clicked client ($ClickX,$ClickY)"
}

if ($Keys -ne '') {
  Add-Type -AssemblyName System.Windows.Forms
  if ($Keys -eq 'CLEAR') {
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait('{DEL}')
  } else {
    [System.Windows.Forms.SendKeys]::SendWait('^a')
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
  }
  Start-Sleep -Milliseconds 500
  Write-Host "typed '$Keys'"
}

Start-Sleep -Milliseconds 600
$bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
$outPath = Join-Path $PSScriptRoot $Out
$bmp.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
$g.Dispose()
Write-Host "captured $($rc.R)x$($rc.B) -> $outPath (pid $($proc.Id))"

if ($Kill) { Stop-Process -Id $proc.Id -Force; Write-Host "killed pid $($proc.Id)" }
