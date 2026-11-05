# Seeds target/debug/config.toml with a demo board (groups + sounds) so the
# console can be screenshotted with content. Deleted after the visual check.

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$demo = "$env:TEMP\soundshared-demo"

$combat = [guid]::NewGuid().ToString()
$ambience = [guid]::NewGuid().ToString()

$sounds = @(
    @{ path = "$demo\intro-theme.wav";   name = 'Intro Theme';   volume = 0.80; group = $null },
    @{ path = "$demo\rain-ambience.wav"; name = 'Rain Ambience'; volume = 0.55; group = $ambience },
    @{ path = "$demo\applause.wav";      name = 'Applause';      volume = 0.65; group = $combat },
    @{ path = "$demo\sword-hit.wav";     name = 'Sword Hit';     volume = 0.90; group = $combat }
)

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('[server]')
[void]$sb.AppendLine('port = 8080')
[void]$sb.AppendLine('host = "0.0.0.0"')
[void]$sb.AppendLine('')
[void]$sb.AppendLine('[audio]')
[void]$sb.AppendLine('buffer_size = 1024')
[void]$sb.AppendLine('sample_rate = 48000')
[void]$sb.AppendLine('channels = 2')
[void]$sb.AppendLine('master_volume = 0.8')
[void]$sb.AppendLine('')
[void]$sb.AppendLine('[logging]')
[void]$sb.AppendLine('level = "info"')
[void]$sb.AppendLine('file_logging = false')
[void]$sb.AppendLine('')
[void]$sb.AppendLine('[library]')
[void]$sb.AppendLine('groups = [')
[void]$sb.AppendLine("  { id = `"$combat`", name = `"Combat`" },")
[void]$sb.AppendLine("  { id = `"$ambience`", name = `"Ambience`" },")
[void]$sb.AppendLine(']')
[void]$sb.AppendLine('sounds = [')
foreach ($s in $sounds) {
    $group = if ($s.group) { "Some(`"$($s.group)`")" } else { 'None' }
    # toml-rs writes Option as either the value or the key omitted; use omit.
    if ($s.group) {
        [void]$sb.AppendLine("  { path = '$($s.path)', name = '$($s.name)', volume = $($s.volume), group = `"$($s.group)`" },")
    }
    else {
        [void]$sb.AppendLine("  { path = '$($s.path)', name = '$($s.name)', volume = $($s.volume) },")
    }
}
[void]$sb.AppendLine(']')

$target = Join-Path $repo 'target\debug\config.toml'
[System.IO.File]::WriteAllText($target, $sb.ToString())
"seeded: $target"
"combat:   $combat"
"ambience: $ambience"
