# Runs every U2b UI scenario (scenes/ui/index.json) in its own oracle process, so each starts from the same state
# (fresh sandbox build folder and Settings.xml), like the Rust test does.
#   pwsh tools/sg-oracle/run-ui.ps1 [-Names smoke,nav]
param([string[]]$Names)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$Names = @($Names | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
if (-not $Names) { $Names = Get-Content "$here\scenes\ui\index.json" -Raw | ConvertFrom-Json }
foreach ($n in $Names) {
    "=== $n"
    & pwsh -NoProfile -File "$here\run.ps1" -Script ui-runner.lua -ScriptArgs $n -Timeout 180 | Select-String 'exit code|outside watched|PoB-like|STOP'
    if ($LASTEXITCODE -eq 2) { throw "STOP: the oracle changed files outside the sandbox ($n)" }
}
