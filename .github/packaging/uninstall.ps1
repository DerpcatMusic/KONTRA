$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { throw 'LOCALAPPDATA is required' }
$Common = Join-Path $env:LOCALAPPDATA 'Programs\Common'
foreach ($Path in @((Join-Path $Common 'CLAP\KONTRA.clap'), (Join-Path $Common 'VST3\KONTRA.vst3'))) {
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path -Recurse -Force }
}
Write-Output 'Removed installed KONTRA files. Libraries, settings and logs remain.'
