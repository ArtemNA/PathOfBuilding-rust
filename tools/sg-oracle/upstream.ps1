# The upstream PoB checkout the SG oracle runs on: `pobRoot` / `pobSha` of tools/upstream.json (the ONE upstream config,
# R-UP); env POB_ROOT overrides the root (as for tools/upstream.ts and the Rust tests). Dot-sourced by setup.ps1 / run.ps1.
#   . (Join-Path $PSScriptRoot 'upstream.ps1'); $up = Get-SgUpstream   # -> @{ Root = 'C:/.../PathOfBuilding'; Sha = '...' }
function Get-SgUpstream {
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $cfg = Get-Content (Join-Path $repo 'tools\upstream.json') -Raw | ConvertFrom-Json
    $root = if ($env:POB_ROOT) { $env:POB_ROOT } else { $cfg.pobRoot }
    if (-not [System.IO.Path]::IsPathRooted($root)) { $root = Join-Path $repo $root }
    $root = ([System.IO.Path]::GetFullPath($root) -replace '\\', '/').TrimEnd('/')
    if (-not (Test-Path (Join-Path $root 'runtime\SimpleGraphic.dll'))) { throw "upstream $root has no runtime/SimpleGraphic.dll" }
    @{ Root = $root; Sha = $cfg.pobSha; Version = $cfg.pobVersion }
}
