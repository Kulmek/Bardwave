# Verify the repeat toggle end to end:
#  1. config with looped=true  -> the row's toggle must render lit
#  2. click the toggle         -> config.toml must be rewritten with looped=false
#  3. click again              -> back to looped=true
# Captures are taken with the window forced topmost so nothing occludes them.
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U5 {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
}
"@
$HWND_TOPMOST = New-Object IntPtr(-1)
$HWND_NOTOPMOST = New-Object IntPtr(-2)
$SWP_NOSIZE = 0x0001; $SWP_NOMOVE = 0x0002

function Snap($hwnd, $file) {
  $rc = New-Object U5+RECT
  [void][U5]::GetClientRect($hwnd, [ref]$rc)
  $pt = New-Object U5+POINT
  [void][U5]::ClientToScreen($hwnd, [ref]$pt)
  $bmp = New-Object System.Drawing.Bitmap($rc.R, $rc.B)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen((New-Object System.Drawing.Point($pt.X, $pt.Y)), [System.Drawing.Point]::Empty, (New-Object System.Drawing.Size($rc.R, $rc.B)))
  $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  return @($rc.R, $rc.B)
}

# Topmost run of solid rose pixels in the left panel = a row's transport button.
function FindTransport($file) {
  $bmp = New-Object System.Drawing.Bitmap($file)
  $xLimit = [int]($bmp.Width * 0.4)
  $best = $null
  for ($y = 0; $y -lt $bmp.Height; $y++) {
    $run = 0; $start = -1; $bestRun = 0; $bestStart = -1
    for ($x = 0; $x -lt $xLimit; $x++) {
      $c = $bmp.GetPixel($x, $y)
      if ($c.R -gt 200 -and $c.G -lt 70 -and $c.B -gt 50 -and $c.B -lt 110) {
        if ($run -eq 0) { $start = $x }
        $run++
        if ($run -gt $bestRun) { $bestRun = $run; $bestStart = $start }
      } else { $run = 0 }
    }
    if ($bestRun -ge 26 -and $bestRun -le 40) {
      $best = @($bestStart, $bestRun, $y)
      break
    }
  }
  $bmp.Dispose()
  return $best
}

# Count "rose-ish" pixels inside the toggle box: the lit glyph is rose, the
# unlit one is zinc (R ~= G).
function LoopPixels($file, $cx, $cy) {
  $bmp = New-Object System.Drawing.Bitmap($file)
  $n = 0
  for ($y = $cy - 9; $y -le $cy + 9; $y++) {
    for ($x = $cx - 9; $x -le $cx + 9; $x++) {
      if ($x -lt 0 -or $y -lt 0 -or $x -ge $bmp.Width -or $y -ge $bmp.Height) { continue }
      $c = $bmp.GetPixel($x, $y)
      if (($c.R - $c.G) -gt 40 -and $c.R -gt 90) { $n++ }
    }
  }
  $bmp.Dispose()
  return $n
}

function ClickAt($hwnd, $lx, $ly) {
  $pt = New-Object U5+POINT
  $pt.X = $lx; $pt.Y = $ly
  [void][U5]::ClientToScreen($hwnd, [ref]$pt)
  [void][U5]::SetCursorPos($pt.X, $pt.Y)
  Start-Sleep -Milliseconds 300
  [U5]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 90
  [U5]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 300
}

function LoopedLine($cfg) {
  $line = Select-String -Path $cfg -Pattern '^looped' | Select-Object -First 1
  if ($null -eq $line) { return "<absent>" }
  return $line.Line.Trim()
}

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\debug\soundshared.exe'
$cfg = Join-Path $root 'target\debug\config.toml'
$bak = Join-Path $PSScriptRoot 'config.toml.bak'
Copy-Item $cfg $bak -Force

# --- 1. start with the sound set to repeat ---
$text = Get-Content $cfg -Raw
if ($text -notmatch '(?m)^looped') {
  $text = $text -replace "(?m)^(volume = [0-9.]+)$", "`$1`nlooped = true"
}
$text | Set-Content $cfg -NoNewline
"config before launch: $(LoopedLine $cfg)"

Get-Process soundshared -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
$p = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput (Join-Path $PSScriptRoot 'loop.log') -RedirectStandardError (Join-Path $PSScriptRoot 'loop.err.log')

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  $p.Refresh()
  if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $h = $p.MainWindowHandle; break }
}
if ($h -eq [IntPtr]::Zero) { "NO WINDOW"; Get-Content (Join-Path $PSScriptRoot 'loop.log') -Tail 15; exit 1 }
[void][U5]::ShowWindow($h, 9)
[void][U5]::MoveWindow($h, 30, 30, 1296, 839, $true)
[void][U5]::SetWindowPos($h, $HWND_TOPMOST, 0, 0, 0, 0, $SWP_NOSIZE -bor $SWP_NOMOVE)
[void][U5]::SetForegroundWindow($h)
Start-Sleep -Seconds 4

$on = Join-Path $PSScriptRoot 'loop_on.png'
$geo = Snap $h $on
$tr = FindTransport $on
if ($null -eq $tr) { "NO TRANSPORT BUTTON FOUND"; [void][U5]::SetWindowPos($h, $HWND_NOTOPMOST, 0,0,0,0,3); exit 1 }
$toggleX = $tr[0] + $tr[1] + 17
$toggleY = $tr[2]
"row transport at x=$($tr[0]) len=$($tr[1]) y=$($tr[2])  -> toggle center ($toggleX,$toggleY)"
"lit pixels with looped=true : $(LoopPixels $on $toggleX $toggleY)"

# --- 2. click the toggle off ---
ClickAt $h $toggleX $toggleY
Start-Sleep -Seconds 3
$off = Join-Path $PSScriptRoot 'loop_off.png'
$null = Snap $h $off
"lit pixels after click    : $(LoopPixels $off $toggleX $toggleY)"
"config after click        : $(LoopedLine $cfg)"

# --- 3. click it back on ---
ClickAt $h $toggleX $toggleY
Start-Sleep -Seconds 3
$on2 = Join-Path $PSScriptRoot 'loop_on2.png'
$null = Snap $h $on2
"lit pixels after 2nd click: $(LoopPixels $on2 $toggleX $toggleY)"
"config after 2nd click    : $(LoopedLine $cfg)"

[void][U5]::SetWindowPos($h, $HWND_NOTOPMOST, 0, 0, 0, 0, $SWP_NOSIZE -bor $SWP_NOMOVE)
Get-Process soundshared -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
Copy-Item $bak $cfg -Force
"config restored: $(LoopedLine $cfg)"
