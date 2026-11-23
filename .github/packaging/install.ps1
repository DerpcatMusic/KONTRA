$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { throw 'LOCALAPPDATA is required' }
foreach ($Name in @('KONTRA.clap', 'KONTRA.vst3\Contents\x86_64-win\KONTRA.vst3')) {
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $Name) -PathType Leaf)) {
        throw "Missing package file: $Name"
    }
}
$Common = Join-Path $env:LOCALAPPDATA 'Programs\Common'
foreach ($Directory in @((Join-Path $Common 'CLAP'), (Join-Path $Common 'VST3'))) {
    New-Item -ItemType Directory -Path $Directory -Force | Out-Null
}
$Vst3Target = Join-Path $Common 'VST3\KONTRA.vst3'
$ClapTarget = Join-Path $Common 'CLAP\KONTRA.clap'
$Temp = Join-Path $Common ('VST3\.KONTRA-install-' + [guid]::NewGuid())
$Vst3Backup = Join-Path $Temp 'previous-vst3'
$ClapBackup = Join-Path $Temp 'previous-clap'
$KeepBackup = $false
$HasVst3Backup = $false
$HasClapBackup = $false
$Vst3Installed = $false
$ClapInstalled = $false
New-Item -ItemType Directory -Path $Temp | Out-Null
try {
    # Stage both formats before modifying either live plugin path.
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'KONTRA.vst3') -Destination $Temp -Recurse
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'KONTRA.clap') -Destination $Temp
    try {
        if (Test-Path -LiteralPath $Vst3Target) {
            Move-Item -LiteralPath $Vst3Target -Destination $Vst3Backup
            $HasVst3Backup = $true
            $KeepBackup = $true
        }
        if (Test-Path -LiteralPath $ClapTarget) {
            Move-Item -LiteralPath $ClapTarget -Destination $ClapBackup
            $HasClapBackup = $true
            $KeepBackup = $true
        }
        Move-Item -LiteralPath (Join-Path $Temp 'KONTRA.vst3') -Destination $Vst3Target
        $Vst3Installed = $true
        Move-Item -LiteralPath (Join-Path $Temp 'KONTRA.clap') -Destination $ClapTarget
        $ClapInstalled = $true
    }
    catch {
        try {
            if ($ClapInstalled -and (Test-Path -LiteralPath $ClapTarget)) {
                Remove-Item -LiteralPath $ClapTarget -Force
            }
            if ($HasClapBackup) {
                Move-Item -LiteralPath $ClapBackup -Destination $ClapTarget
                $HasClapBackup = $false
            }
        } catch {
            Write-Warning "CLAP rollback failed: $_" -WarningAction Continue
        }
        try {
            if ($Vst3Installed -and (Test-Path -LiteralPath $Vst3Target)) {
                Remove-Item -LiteralPath $Vst3Target -Recurse -Force
            }
            if ($HasVst3Backup) {
                Move-Item -LiteralPath $Vst3Backup -Destination $Vst3Target
                $HasVst3Backup = $false
            }
        } catch {
            Write-Warning "VST3 rollback failed: $_" -WarningAction Continue
        }
        $KeepBackup = $HasClapBackup -or $HasVst3Backup
        throw
    }
    $KeepBackup = $false

} finally {
    if ($KeepBackup) {
        Write-Warning "Could not restore all previous KONTRA plugins. Remaining backups are at $Temp" -WarningAction Continue
    } else {
        Remove-Item -LiteralPath $Temp -Recurse -Force
    }
}
Write-Output "Installed KONTRA. Rescan plugins in your DAW."
