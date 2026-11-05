# Generates small demo WAV files used to populate the console for a screenshot.
# Lives in .scratch/ and is deleted after the visual check.
param(
    [string]$OutDir = "$env:TEMP\soundshared-demo"
)

function New-Wav {
    param(
        [string]$Path,
        [int]$SampleRate,
        [double]$Seconds,
        [double]$Freq
    )

    $channels = 2
    $bits = 16
    $frames = [int]($SampleRate * $Seconds)
    $blockAlign = $channels * ($bits / 8)
    $dataBytes = $frames * $blockAlign

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)

    # RIFF header
    $bw.Write([char[]]'RIFF')
    $bw.Write([int](36 + $dataBytes))
    $bw.Write([char[]]'WAVE')

    # fmt chunk
    $bw.Write([char[]]'fmt ')
    $bw.Write([int]16)
    $bw.Write([int16]1)
    $bw.Write([int16]$channels)
    $bw.Write([int]$SampleRate)
    $bw.Write([int]($SampleRate * $blockAlign))
    $bw.Write([int16]$blockAlign)
    $bw.Write([int16]$bits)

    # data chunk — a quiet two-tone so the files are audibly distinct
    $bw.Write([char[]]'data')
    $bw.Write([int]$dataBytes)
    for ($i = 0; $i -lt $frames; $i++) {
        $t = $i / $SampleRate
        $env = [Math]::Sin([Math]::PI * $t / $Seconds)
        $sample = 0.16 * $env * ([Math]::Sin(2 * [Math]::PI * $Freq * $t) +
                                 0.4 * [Math]::Sin(2 * [Math]::PI * $Freq * 2 * $t))
        $v = [int16]([Math]::Round($sample * 32767))
        $bw.Write([int16]$v)
        $bw.Write([int16]$v)
    }

    $bw.Flush()
    [System.IO.File]::WriteAllBytes($Path, $ms.ToArray())
    $bw.Dispose()
    $ms.Dispose()
}

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

New-Wav -Path "$OutDir\intro-theme.wav"    -SampleRate 48000 -Seconds 42 -Freq 220
New-Wav -Path "$OutDir\rain-ambience.wav" -SampleRate 44100 -Seconds 60 -Freq 140
New-Wav -Path "$OutDir\applause.wav"      -SampleRate 48000 -Seconds 8  -Freq 320
New-Wav -Path "$OutDir\sword-hit.wav"     -SampleRate 48000 -Seconds 2  -Freq 660

Get-ChildItem $OutDir -Filter *.wav |
    Select-Object Name, @{ n = 'MB'; e = { [Math]::Round($_.Length / 1MB, 1) } } |
    Format-Table -AutoSize
