$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { throw 'LOCALAPPDATA is required' }
foreach ($Name in @('KONTRA.clap', 'KONTRA.vst3')) {
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $Name))) { throw "Missing package file: $Name" }
}
$Common = Join-Path $env:LOCALAPPDATA 'Programs\Common'
foreach ($Directory in @((Join-Path $Common 'CLAP'), (Join-Path $Common 'VST3'))) {
    New-Item -ItemType Directory -Path $Directory -Force | Out-Null
}
$Target = Join-Path $Common 'VST3\KONTRA.vst3'
$Temp = Join-Path $Common ('VST3\.KONTRA-install-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null
try {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'KONTRA.vst3') -Destination $Temp -Recurse
    if (Test-Path -LiteralPath $Target) { Move-Item -LiteralPath $Target -Destination (Join-Path $Temp 'previous') }
    try { Move-Item -LiteralPath (Join-Path $Temp 'KONTRA.vst3') -Destination $Target }
    catch {
        if (Test-Path -LiteralPath (Join-Path $Temp 'previous')) { Move-Item -LiteralPath (Join-Path $Temp 'previous') -Destination $Target }
        throw
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'KONTRA.clap') -Destination (Join-Path $Common 'CLAP\KONTRA.clap') -Force

} finally { Remove-Item -LiteralPath $Temp -Recurse -Force }
Write-Output "Installed KONTRA. Rescan plugins in your DAW."
