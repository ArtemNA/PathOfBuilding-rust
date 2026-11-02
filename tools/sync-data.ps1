#requires -Version 7
# Mirror pob-ts/data into <repo>/data (manifest.files + manifest.json), verify sha256, write data.lock.
# R5.5c: also copy the timeless-jewel LUTs (PathOfBuilding/src/Data/TimelessJewelData/*.zip, *.zip.partN -- binary RFC1950
# zlib streams, W6) verbatim into <repo>/data/timeless-luts/; the PoB checkout must be at the manifest's pobSha; the sha256 of
# every LUT file goes into data.lock (`timelessLuts`). pob-data inflates them lazily (flate2); the PoB checkout is never read
# at run time.
# R-UP: `-Convert` runs the pob-ts converter (READ-ONLY: only executed) against the upstream of tools/upstream.json
# instead of copying pob-ts/data (which stays at the sha pob-ts was converted for):
#   1. tools/upstream-mirror.ps1 -- Lua-5.4-loadable mirror (`..=` desugared, see that script);
#   2. `node <pob-ts>/tools/convert/cli.ts --wire-only` into tools/.scratch/convert-<sha9>/wire;
#   3. tools/convert-fixups.ts -- drop wire fields the strict pob-ts extractors reject but pob-rust does not consume;
#   4. `--extract-only` into tools/.scratch/convert-<sha9>/data, which then becomes -Source of the normal sync.
# Defaults (PoB root, pob-ts root) come from tools/upstream.json; env POB_ROOT / POB_TS_ROOT override.
# Usage: tools/sync-data.ps1 [-Convert] [-Source <path>] [-PobRoot <path>] [-Check]
param(
    [string]$Source,
    [string]$PobRoot,
    [switch]$Convert,
    [switch]$Check
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root 'data'
$lockPath = Join-Path $root 'data.lock'
$lutRel = 'timeless-luts'
$lutDest = Join-Path $dest $lutRel
$upstreamCfg = Get-Content -LiteralPath (Join-Path $root 'tools\upstream.json') -Raw | ConvertFrom-Json
$pobTsRoot = if ($env:POB_TS_ROOT) { $env:POB_TS_ROOT } else { $upstreamCfg.pobTsRoot }
if (-not $PobRoot) {
    $PobRoot = if ($env:POB_ROOT) { $env:POB_ROOT } else { $upstreamCfg.pobRoot }
}
$sourceLabel = 'pob-ts/data'
$wireFixups = @()
if ($Convert -and -not $Check) {
    $sha9 = $upstreamCfg.pobSha.Substring(0, 9)
    $work = Join-Path $root "tools\.scratch\convert-$sha9"
    $wire = Join-Path $work 'wire'
    $Source = Join-Path $work 'data'
    $mirrorOut = & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'upstream-mirror.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'upstream-mirror.ps1 failed' }
    Write-Output $mirrorOut
    $mirror = if ($env:POB_WASM_ROOT) { $env:POB_WASM_ROOT } else { Join-Path $root $upstreamCfg.wasmMirror }
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $Source | Out-Null
    $cli = Join-Path $pobTsRoot 'tools\convert\cli.ts'
    Push-Location $pobTsRoot
    try {
        & node $cli --wire-only --pob-root $mirror --wire-dir $wire --data-dir $Source | Out-File -LiteralPath (Join-Path $work 'wire.txt')
        if ($LASTEXITCODE -ne 0) { throw "converter --wire-only failed (log: $work\wire.txt)" }
        $fix = & node (Join-Path $PSScriptRoot 'convert-fixups.ts') $wire
        if ($LASTEXITCODE -ne 0) { throw 'convert-fixups.ts failed' }
        $wireFixups = @($fix | ConvertFrom-Json)
        & node $cli --extract-only --pob-root $mirror --wire-dir $wire --data-dir $Source | Out-File -LiteralPath (Join-Path $work 'extract.txt')
        if ($LASTEXITCODE -ne 0) { throw "converter --extract-only failed (log: $work\extract.txt)" }
    } finally { Pop-Location }
    $pobTsSha = (& git -C $pobTsRoot rev-parse --short HEAD).Trim()
    $sourceLabel = "pob-ts converter @ $pobTsSha (tools/sync-data.ps1 -Convert) on tools/upstream.json pobRoot"
}
if (-not $Source) { $Source = Join-Path $pobTsRoot 'data' }
$lutSrc = Join-Path $PobRoot 'src\Data\TimelessJewelData'
# `<stem>.zip` or `<stem>.zip.part<N>` -- what DataJewelFileLoader.lua reads.
function Test-LutName([string]$n) { $n -match '^[A-Za-z]+\.zip(\.part\d+)?$' }

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
    # Remove files not in manifest (the LUT directory is handled below).
    $allowed = [System.Collections.Generic.HashSet[string]]::new([string[]](@($entries.file) + 'manifest.json'))
    foreach ($rel in Get-RelFiles $dest) {
        if ($rel.StartsWith("$lutRel/")) { continue }
        if (-not $allowed.Contains($rel)) {
            Remove-Item -LiteralPath (Join-Path $dest $rel.Replace('/', '\')) -Force
            $removed++
        }
    }
    # Remove empty directories.
    Get-ChildItem -LiteralPath $dest -Recurse -Directory | Sort-Object { $_.FullName.Length } -Descending |
        Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } | Remove-Item -Force

    # Timeless-jewel LUTs from the PoB checkout at the manifest's pobSha.
    $pobHead = (& git -C $PobRoot rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $pobHead) { $problems.Add("PoB checkout not readable: $PobRoot") }
    elseif ($pobHead.Trim() -ne $manifest.pobSha) { $problems.Add("PoB checkout $PobRoot is at $($pobHead.Trim()), manifest pobSha is $($manifest.pobSha)") }
    elseif (-not (Test-Path -LiteralPath $lutSrc)) { $problems.Add("missing LUT source dir: $lutSrc") }
    else {
        New-Item -ItemType Directory -Force -Path $lutDest | Out-Null
        $lutNames = @(Get-ChildItem -LiteralPath $lutSrc -File | Where-Object { Test-LutName $_.Name } | ForEach-Object { $_.Name })
        foreach ($n in $lutNames) {
            $s = Join-Path $lutSrc $n
            $d = Join-Path $lutDest $n
            $need = $true
            if (Test-Path -LiteralPath $d) {
                $si = Get-Item -LiteralPath $s; $di = Get-Item -LiteralPath $d
                if ($si.Length -eq $di.Length -and (Get-Sha $s) -eq (Get-Sha $d)) { $need = $false }
            }
            if ($need) { Copy-Item -LiteralPath $s -Destination $d -Force; $copied++ }
        }
        $lutSet = [System.Collections.Generic.HashSet[string]]::new([string[]]$lutNames)
        foreach ($f in Get-ChildItem -LiteralPath $lutDest -File) {
            if (-not $lutSet.Contains($f.Name)) { Remove-Item -LiteralPath $f.FullName -Force; $removed++ }
        }
    }
}

# LUT files present in data/timeless-luts (name -> sha256), sorted by name (ordinal).
$lutFiles = [ordered]@{}
$lutBytes = [long]0
if (Test-Path -LiteralPath $lutDest) {
    foreach ($f in (Get-ChildItem -LiteralPath $lutDest -File | Sort-Object -Property Name -CaseSensitive:$false -Culture '')) {
        if (-not (Test-LutName $f.Name)) { $problems.Add("unexpected file in $lutRel/: $($f.Name)"); continue }
        $lutFiles[$f.Name] = Get-Sha $f.FullName
        $lutBytes += $f.Length
    }
}
if ($lutFiles.Count -eq 0) { $problems.Add("no timeless LUT files in $lutRel/") }

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
    if ($rel.StartsWith("$lutRel/")) { continue }
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
        if (-not $lock.timelessLuts) { $problems.Add('data.lock has no timelessLuts section') }
        else {
            $locked = $lock.timelessLuts.files.PSObject.Properties
            foreach ($p in $locked) {
                if (-not $lutFiles.Contains($p.Name)) { $problems.Add("missing LUT: $lutRel/$($p.Name)") }
                elseif ($lutFiles[$p.Name] -ne $p.Value) { $problems.Add("sha256 mismatch: $lutRel/$($p.Name)") }
            }
            $lockedNames = [System.Collections.Generic.HashSet[string]]::new([string[]]@($locked | ForEach-Object { $_.Name }))
            foreach ($n in $lutFiles.Keys) { if (-not $lockedNames.Contains($n)) { $problems.Add("LUT not in data.lock: $lutRel/$n") } }
        }
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
        source            = $sourceLabel
        wireFixups        = @($wireFixups | ForEach-Object { [ordered]@{ id = $_.id; upstream = $_.upstream; why = $_.why } })
        timelessLuts      = [ordered]@{
            source = 'PathOfBuilding/src/Data/TimelessJewelData'
            bytes  = $lutBytes
            files  = $lutFiles
        }
    }
    $json = (($lock | ConvertTo-Json -Depth 4) -replace "\r\n", "`n").TrimEnd("`n") + "`n"
    [IO.File]::WriteAllText($lockPath, $json, [Text.UTF8Encoding]::new($false))
}

if (-not $Check) { Write-Output "sync: copied $copied, removed $removed" }
if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Output "  - $_" }
    Write-Output "FAIL: $($problems.Count) problems"
    exit 1
}
Write-Output "data: $($entries.Count) files, $totalBytes bytes, pobSha $($manifest.pobSha.Substring(0, 9)), OK"
Write-Output "timeless LUTs: $($lutFiles.Count) files, $lutBytes bytes, OK"
exit 0
