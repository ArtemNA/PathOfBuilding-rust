#requires -Version 7
# Re-take every LuaJIT / pob-ts fixture of crates/*/tests/fixtures and data-patches/ against the upstream of tools/upstream.json
# (R-UP; UPDATE-PROMPT.md step 6). Probes run strictly one after another (one docker container at a time, W12), each logged to
# tools/.scratch/retake/<probe>.log; a failing probe does not stop the rest -- the summary lists it.
#
#   tools/retake-fixtures.ps1 [-Only <regex>] [-Skip <regex>]
#
# Needs: Docker Desktop running (image ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest), Node 24 on PATH, the Rust
# workspace built (export-engine / export-real call the release pob-cli). Order matters only for data-patches (first: pob-data
# refuses data whose manifest sha differs from the patches') and for `export-power run` -> `pack`.
param([string]$Only = '.', [string]$Skip = '^$')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$log = Join-Path $root 'tools\.scratch\retake'
New-Item -ItemType Directory -Force -Path $log | Out-Null

# probe (tools/lua-corpus/<name>.ts) + args -> what it writes
$probes = @(
    @('export-data-patch', @(), 'data-patches/mod-numbers.json'),
    @('export-jewel-upvalues', @(), 'data-patches/mod-parser-upvalues.json'),
    @('export-legion', @(), 'data-patches/legion-passives.json'),
    @('export-tattoo', @(), 'data-patches/tattoo-passives.json'),
    @('export-tree-versions', @(), 'data-patches/tree-versions.json'),
    @('export-gemdata', @(), 'crates/pob-data/tests/fixtures/gemdata-*.json + data-patches/skill-levels-shape.json'),
    @('export-numeric', @(), 'crates/pob-lua/tests/fixtures/luajit-numeric.json (docker)'),
    @('export-libm-desktop', @(), 'crates/pob-lua/tests/fixtures/desktop-libm.json (desktop lua51.dll of pobRoot/runtime)'),
    @('export-patterns', @(), 'crates/pob-lua/tests/fixtures/luajit-{patterns-modparser,patterns-fuzz,bit}.json'),
    @('export-modstore', @(), 'crates/pob-mods/tests/fixtures/modstore-*.json'),
    @('export-modparser', @(), 'crates/pob-parser/tests/fixtures/modparser-NN.json'),
    @('export-build-model', @(), 'crates/pob-build/tests/fixtures/<build>.model.json (pob-ts)'),
    @('export-xml-config', @(), 'crates/pob-build/tests/fixtures/{xml-lua,config-load}.json'),
    @('export-config-apply', @(), 'crates/pob-build/tests/fixtures/config-apply.json'),
    @('export-radius', @(), 'crates/pob-tree/tests/fixtures/radius-*.json'),
    @('export-timeless', @(), 'crates/pob-tree/tests/fixtures/timeless-*.json'),
    @('export-buildraw', @(), 'crates/pob-items/tests/fixtures/buildraw.json'),
    @('export-env', @(), 'crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json'),
    @('export-skills', @(), 'crates/pob-calc/tests/fixtures/<build>.skills*.json'),
    @('export-r4', @(), 'crates/pob-calc/tests/fixtures/<build>.{r4-pobts,pools.luajit}.json'),
    @('export-defence-only', @(), 'crates/pob-calc/tests/fixtures/<build>.defence-pobts.json (pob-ts)'),
    @('export-perform-only', @(), 'crates/pob-calc/tests/fixtures/<build>.perform-pobts.json (pob-ts)'),
    @('export-pools-synth', @(), 'crates/pob-calc/tests/fixtures/pools-synth.luajit.json'),
    @('export-defence-synth', @('coverage'), 'tools/.scratch/defence-synth-coverage*.json (line hook cache keyed by upstream line numbers: first!)'),
    @('export-defence-synth', @(), 'crates/pob-calc/tests/fixtures/defence-synth/*'),
    @('export-defence-synth2', @('coverage'), 'tools/.scratch/defence-synth2-coverage.json (first!)'),
    @('export-defence-synth2', @(), 'crates/pob-calc/tests/fixtures/defence-synth2/*'),
    @('export-offence', @(), 'crates/pob-calc/tests/fixtures/<build>.offence.json'),
    @('export-offence', @('runs'), 'crates/pob-calc/tests/fixtures/<build>.offence-runs*.json'),
    @('export-offence-synth', @('coverage'), 'tools/.scratch/offence-synth-coverage.json (first!)'),
    @('export-offence-synth', @(), 'crates/pob-calc/tests/fixtures/offence-synth/*'),
    @('export-power', @('run'), 'tools/.scratch/power-raw/*'),
    @('export-power', @('pack'), 'crates/pob-calc/tests/fixtures/power/*'),
    @('export-breakdown', @(), 'crates/pob-calc/tests/fixtures/breakdown/*'),
    @('export-r8', @(), 'crates/pob-engine/tests/fixtures/r8/*'),
    @('export-calcs', @(), 'crates/pob-engine/tests/fixtures/calcs/*'),
    @('export-engine', @(), 'crates/pob-engine/tests/fixtures/engine/* (needs the Rust build)')
)
$rows = @()
Push-Location $root
try {
    foreach ($p in $probes) {
        $name, $args_, $what = $p
        $id = ($name + ' ' + ($args_ -join ' ')).Trim()
        if ($id -notmatch $Only -or $id -match $Skip) { continue }
        $file = Join-Path $log (($id -replace ' ', '-') + '.log')
        # the coverage caches are keyed by the XML sha256 only, but hold line numbers of the upstream they were taken on: drop them
        if ($args_ -contains 'coverage') {
            $stem = $name -replace '^export-', ''
            Get-ChildItem -LiteralPath (Join-Path $root 'tools\.scratch') -Filter "$stem-coverage*.json" -File -ErrorAction SilentlyContinue |
                Remove-Item -Force
            if ($stem -eq 'offence-synth') { Remove-Item -LiteralPath (Join-Path $root 'tools\.scratch\r7s2-coverage.json') -Force -ErrorAction SilentlyContinue }
        }
        $t = [Diagnostics.Stopwatch]::StartNew()
        & node "tools/lua-corpus/$name.ts" @args_ *> $file
        $code = $LASTEXITCODE
        $t.Stop()
        $rows += [pscustomobject]@{ probe = $id; exit = $code; seconds = [math]::Round($t.Elapsed.TotalSeconds); writes = $what }
        Write-Output ("{0,-28} exit {1}  {2,5} s  {3}" -f $id, $code, [math]::Round($t.Elapsed.TotalSeconds), $what)
    }
} finally { Pop-Location }
$bad = @($rows | Where-Object { $_.exit -ne 0 })
Write-Output "probes: $($rows.Count), failed: $($bad.Count) $(($bad | ForEach-Object probe) -join ', ')"
if ($bad.Count) { exit 1 }
