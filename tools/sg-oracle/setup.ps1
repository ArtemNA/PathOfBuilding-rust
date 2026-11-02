# Recreates the SG-oracle sandbox tools/sg-oracle/runtime/ by COPYING the original PoB runtime.
# Sources are only read. The original exe is ever launched only from this copy (see run.ps1).
#   pwsh tools/sg-oracle/setup.ps1 [-Source <PoB checkout>] [-Force]
# -Source defaults to the upstream of tools/upstream.json (upstream.ps1; env POB_ROOT overrides).
param(
    [string]$Source,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
. (Join-Path $here 'upstream.ps1')
if (-not $Source) { $Source = (Get-SgUpstream).Root }
$dst = Join-Path $here 'runtime'
$src = Join-Path $Source 'runtime'
if (-not (Test-Path (Join-Path $src 'SimpleGraphic.dll'))) { throw "no SimpleGraphic.dll under $src" }

if (Test-Path $dst) {
    if (-not $Force) { throw "$dst exists; pass -Force to recreate" }
    # Safety: only ever delete the sandbox folder inside this tool directory.
    $full = (Resolve-Path $dst).Path
    if (-not $full.EndsWith('\tools\sg-oracle\runtime')) { throw "refusing to delete $full" }
    Remove-Item -Recurse -Force $full
}
New-Item -ItemType Directory $dst | Out-Null
# Copy exe, DLLs, fonts, runtime Lua libs. Update.exe is not needed and is skipped on purpose.
Get-ChildItem $src -Force | Where-Object { $_.Name -ne 'Update.exe' } |
    ForEach-Object { Copy-Item $_.FullName -Destination $dst -Recurse }

# Sanity: copied files are byte-identical to the source.
$bad = 0
Get-ChildItem $dst -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($dst.Length)
    $s = Join-Path $src $rel
    if ((Get-FileHash $_.FullName).Hash -ne (Get-FileHash $s).Hash) { $bad++; Write-Warning "hash mismatch $rel" }
}
$n = (Get-ChildItem $dst -Recurse -File).Count
"sandbox: $dst ($n files, $bad mismatches)"
if ($bad) { exit 1 }
