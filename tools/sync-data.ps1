#requires -Version 7
# Mirror pob-ts/data into <repo>/data (manifest.files + manifest.json), verify sha256, write data.lock.
# Usage: tools/sync-data.ps1 [-Source <path>] [-Check]
param(
    [string]$Source,
    [switch]$Check
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'data'
$lockPath = Join-Path $root 'data.lock'
if (-not $Source) {
    $Source = if ($env:POB_TS_ROOT) { Join-Path $env:POB_TS_ROOT 'data' } else { 'C:\pob-work\pob-ts\data' }
}

function Get-Sha([string]$p) { (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant() }
function Get-RelFiles([string]$dir) {
    if (-not (Test-Path -LiteralPath $dir)) { return @() }
    $base = (Resolve-Path -LiteralPath $dir).Path.TrimEnd('\') + '\'
    Get-ChildItem -LiteralPath $dir -Recurse -File | ForEach-Object { $_.FullName.Substring($base.Length).Replace('\', '/') }
}

$problems = [System.Collections.Generic.List[string]]::new()
$copied = 0
$removed = 0

# Manifest: from source when syncing, from data\ when checking.
$manifestSrc = if ($Check) { Join-Path $dest 'manifest.json' } else { Join-Path $Source 'manifest.json' }
if (-not (Test-Path -LiteralPath $manifestSrc)) { Write-Output "FAIL: 1 problems`nmissing manifest: $manifestSrc"; exit 1 }
$manifest = Get-Content -LiteralPath $manifestSrc -Raw | ConvertFrom-Json
$entries = @($manifest.files)

if (-not $Check) {
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $toCopy = @($entries | ForEach-Object { [pscustomobject]@{ file = $_.file; bytes = $_.bytes; sha256 = $_.sha256 } })
    $toCopy += [pscustomobject]@{ file = 'manifest.json'; bytes = $null; sha256 = $null }
    foreach ($e in $toCopy) {
        $s = Join-Path $Source ($e.file.Replace('/', '\'))
        $d = Join-Path $dest ($e.file.Replace('/', '\'))
        if (-not (Test-Path -LiteralPath $s)) { $problems.Add("source missing: $($e.file)"); continue }
        $need = $true
        if (Test-Path -LiteralPath $d) {
            $si = Get-Item -LiteralPath $s; $di = Get-Item -LiteralPath $d
            if ($si.Length -eq $di.Length -and (Get-Sha $s) -eq (Get-Sha $d)) { $need = $false }
        }
        if ($need) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $d) | Out-Null
            Copy-Item -LiteralPath $s -Destination $d -Force
            $copied++
        }
    }
    # Remove files not in manifest.
    $allowed = [System.Collections.Generic.HashSet[string]]::new([string[]](@($entries.file) + 'manifest.json'))
    foreach ($rel in Get-RelFiles $dest) {
        if (-not $allowed.Contains($rel)) {
            Remove-Item -LiteralPath (Join-Path $dest $rel.Replace('/', '\')) -Force
            $removed++
        }
    }
    # Remove empty directories.
    Get-ChildItem -LiteralPath $dest -Recurse -Directory | Sort-Object { $_.FullName.Length } -Descending |
        Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } | Remove-Item -Force
}

# Verification (both modes).
$totalBytes = [long]0
foreach ($e in $entries) {
    $totalBytes += [long]$e.bytes
    $d = Join-Path $dest ($e.file.Replace('/', '\'))
    if (-not (Test-Path -LiteralPath $d)) { $problems.Add("missing: $($e.file)"); continue }
    $len = (Get-Item -LiteralPath $d).Length
    if ($len -ne $e.bytes) { $problems.Add("size mismatch: $($e.file) (expected $($e.bytes), got $len)"); continue }
    if ((Get-Sha $d) -ne $e.sha256.ToLowerInvariant()) { $problems.Add("sha256 mismatch: $($e.file)") }
}
$allowed2 = [System.Collections.Generic.HashSet[string]]::new([string[]](@($entries.file) + 'manifest.json'))
foreach ($rel in Get-RelFiles $dest) {
    if (-not $allowed2.Contains($rel)) { $problems.Add("extra file not in manifest: $rel") }
}

$manifestSha = if (Test-Path -LiteralPath (Join-Path $dest 'manifest.json')) { Get-Sha (Join-Path $dest 'manifest.json') } else { '' }

if ($Check) {
    if (-not (Test-Path -LiteralPath $lockPath)) { $problems.Add('data.lock missing') }
    else {
        $lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json
        if ($lock.manifestSha256 -ne $manifestSha) { $problems.Add("data.lock manifestSha256 mismatch ($($lock.manifestSha256) vs $manifestSha)") }
        if ($lock.pobSha -ne $manifest.pobSha) { $problems.Add("data.lock pobSha mismatch ($($lock.pobSha) vs $($manifest.pobSha))") }
        if ($lock.files -ne $entries.Count) { $problems.Add("data.lock files mismatch ($($lock.files) vs $($entries.Count))") }
    }
}

if ($problems.Count -eq 0 -and -not $Check) {
    $lock = [ordered]@{
        pobSha            = $manifest.pobSha
        converterVersion  = $manifest.converterVersion
        latestTreeVersion = $manifest.latestTreeVersion
        manifestSha256    = $manifestSha
        files             = $entries.Count
        bytes             = $totalBytes
        source            = 'pob-ts/data'
    }
    $json = (($lock | ConvertTo-Json -Depth 3) -replace "\r\n", "`n").TrimEnd("`n") + "`n"
    [IO.File]::WriteAllText($lockPath, $json, [Text.UTF8Encoding]::new($false))
}

if (-not $Check) { Write-Output "sync: copied $copied, removed $removed" }
if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Output "  - $_" }
    Write-Output "FAIL: $($problems.Count) problems"
    exit 1
}
Write-Output "data: $($entries.Count) files, $totalBytes bytes, pobSha $($manifest.pobSha.Substring(0, 9)), OK"
exit 0
