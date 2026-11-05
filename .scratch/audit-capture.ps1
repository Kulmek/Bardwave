# Capture the SoundShared console window client area for spacing analysis.
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
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@
[void][W]::SetProcessDPIAware()

$exe = Join-Path $PSScriptRoot '..\target\release\soundshared.exe'
$exe = (Resolve-Path $exe).Path
$proc = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -PassThru
Start-Sleep -Seconds 4

$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt 40; $i++) {
  $p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
  if ($null -eq $p) { break }
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $p.MainWindowHandle; break }
  Start-Sleep -Milliseconds 250
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Host "NO WINDOW (pid $($proc.Id) alive: $((Get-Process -Id $proc.Id -ErrorAction SilentlyContinue) -ne $null))"; exit 1 }

[void][W]::ShowWindow($hwnd, 5)
[void][W]::SetForegroundWindow($hwnd)
[void][W]::SetWindowPos($hwnd, (New-Object IntPtr(-1)), 0, 0, 0, 0, 0x0001 -bor 0x0002)
Start-Sleep -Milliseconds 900

$rc = New-Object W+RECT
[void][W]::GetClientRect($hwnd, [ref]$rc)
$pt = New-Object W+POINT
[void][W]::ClientToScreen($hwnd, [ref]$pt)
$bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
$out = Join-Path $PSScriptRoot 'audit-gui.png'
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "captured $($rc.R)x$($rc.B) -> $out (pid $($proc.Id))"
Write-Host "PID=$($proc.Id)"
