# Runs a subset of scenes through scene-runner.lua (run.ps1, same sandbox + audit) WITHOUT touching the fixtures of
# other scenes: run.ps1 copies the runner's manifest over fixtures/scenes/manifest.json, so this wrapper restores that
# file and keeps the subset's manifest as fixtures/scenes/manifest-<Tag>.json.
#   pwsh tools/sg-oracle/run-scenes.ps1 -Scenes u12-frac,u12-dpi125,u12-image -Tag u12
param(
    [Parameter(Mandatory)][string[]]$Scenes,
    [Parameter(Mandatory)][string]$Tag
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$man = Join-Path $here 'fixtures\scenes\manifest.json'
$backup = [System.IO.Path]::GetTempFileName()
Copy-Item $man $backup -Force
try {
    & (Join-Path $here 'run.ps1') -Script scene-runner.lua -ScriptArgs ($Scenes -join ',')
    $code = $LASTEXITCODE
    if (Test-Path $man) { Copy-Item $man (Join-Path $here "fixtures\scenes\manifest-$Tag.json") -Force }
} finally {
    Copy-Item $backup $man -Force
    Remove-Item $backup
}
if ($code) { exit $code }
