# Repro: launch GUI, click "Start Server", then connect a WebSocket client
# (the same thing the web UI's "Establish Link" does) and measure whether the
# window still responds.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U2 {
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
  $rc = New-Object U2+RECT
  [void][U2]::GetClientRect($hwnd, [ref]$rc)
  $pt = New-Object U2+POINT
  [void][U2]::ClientToScreen($hwnd, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  return @($rc.R, $rc.B)
}

# Find a wide solid rose block (the primary button), ignoring small rose cells.
function FindButton($file, $xFrom) {
  $bmp = New-Object System.Drawing.Bitmap($file)
  $best = $null
  for ($y = 0; $y -lt $bmp.Height; $y++) {
    $run = 0; $start = -1; $bestRun = 0; $bestStart = -1
    for ($x = $xFrom; $x -lt $bmp.Width; $x++) {
      $c = $bmp.GetPixel($x, $y)
      if ($c.R -gt 200 -and $c.G -lt 70 -and $c.B -gt 50 -and $c.B -lt 110) {
        if ($run -eq 0) { $start = $x }
        $run++
        if ($run -gt $bestRun) { $bestRun = $run; $bestStart = $start }
      } else { $run = 0 }
    }
    if ($bestRun -ge 60) {
      if ($null -eq $best -or $bestRun -gt $best[3]) {
        $best = @([int]($bestStart + $bestRun/2), $y, $bestStart, $bestRun)
      }
    }
  }
  $bmp.Dispose()
  return $best
}

function ClickAt($hwnd, $lx, $ly) {
  $pt = New-Object U2+POINT
  $pt.X = $lx; $pt.Y = $ly
  [void][U2]::ClientToScreen($hwnd, [ref]$pt)
  [void][U2]::SetCursorPos($pt.X, $pt.Y)
  Start-Sleep -Milliseconds 300
  [U2]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 90
  [U2]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 300
}

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\debug\soundshared.exe'
Get-Process soundshared -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
$log = Join-Path $PSScriptRoot 'run2.log'
if (Test-Path $log) { Remove-Item $log -Force }
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $log -RedirectStandardError (Join-Path $PSScriptRoot 'run2.err.log')

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  $p.Refresh()
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }
}
if ($h -eq [IntPtr]::Zero) { "NO WINDOW"; exit 1 }
[void][U2]::ShowWindow($h, 9)
[void][U2]::MoveWindow($h, 40, 40, 1296, 839, $true)
[void][U2]::SetForegroundWindow($h)
Start-Sleep -Seconds 4

$geo = Snap $h (Join-Path $PSScriptRoot 'r_idle.png')
"client $($geo[0])x$($geo[1])"
$xFrom = [int]($geo[0] * 0.72)
$btn = FindButton (Join-Path $PSScriptRoot 'r_idle.png') $xFrom
if ($null -eq $btn) { "NO WIDE ROSE BUTTON FOUND"; exit 1 }
"start button at client=($($btn[0]),$($btn[1])) width=$($btn[3])"

ClickAt $h $btn[0] $btn[1]
Start-Sleep -Seconds 4
$p.Refresh()
"after start click: responding=$($p.Responding)"
Snap $h (Join-Path $PSScriptRoot 'r_running.png') | Out-Null
"log:"; Get-Content $log -Tail 6

# --- connect exactly like the web UI's "Establish Link" ---
$ws = New-Object System.Net.WebSockets.ClientWebSocket
$ct = New-Object System.Threading.CancellationTokenSource
$ct.CancelAfter(15000)
try {
  $ws.ConnectAsync((New-Object Uri 'ws://127.0.0.1:8080/ws/audio'), $ct.Token).Wait()
  "WS state: $($ws.State)"
} catch { "WS CONNECT FAILED: $($_.Exception.Message)"; }

# read a few messages (audio frames flow continuously)
$buf = New-Object byte[] 65536
$got = 0
for ($i = 0; $i -lt 5; $i++) {
  try {
    $seg = New-Object System.ArraySegment[byte] -ArgumentList @(,$buf)
    $rct = New-Object System.Threading.CancellationTokenSource
    $rct.CancelAfter(4000)
    $res = $ws.ReceiveAsync($seg, $rct.Token).Result
    $got += $res.Count
    if ($res.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) { break }
  } catch { "recv $i failed: $($_.Exception.Message)"; break }
}
"bytes received from server: $got"

$p.Refresh()
"after WS connect: responding=$($p.Responding)  cpu=$([math]::Round($p.TotalProcessorTime.TotalSeconds,2))s  threads=$($p.Threads.Count)"
Snap $h (Join-Path $PSScriptRoot 'r_ws.png') | Out-Null

# is the GUI still alive? move the mouse over a sound row and see if hover repaints
$t0 = $p.TotalProcessorTime
Start-Sleep -Seconds 3
$p.Refresh()
"cpu delta 3s idle: $([math]::Round(($p.TotalProcessorTime - $t0).TotalSeconds,2))s  responding=$($p.Responding)"

"=== log tail ==="
Get-Content $log -Tail 25
$ws.Dispose()
