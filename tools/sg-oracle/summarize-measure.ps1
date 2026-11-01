# Builds fixtures/measure.json from fixtures/measure-raw-<mode>.json (lua/measure.lua + inject.ps1) and fixtures/audit/*.json.
#   pwsh tools/sg-oracle/summarize-measure.ps1
$ErrorActionPreference = 'Stop'
$fx = Join-Path $PSScriptRoot 'fixtures'
function Stats($d) {
    if (-not $d.Count) { return [ordered]@{ frames = 0 } }
    $s = $d | Sort-Object
    [ordered]@{ intervals = $d.Count; median_ms = $s[[int][math]::Floor($s.Count / 2)]; min_ms = $s[0]; max_ms = $s[-1]
        mean_ms = [math]::Round(($d | Measure-Object -Average).Average, 2) }
}
$modes = [ordered]@{}
foreach ($f in Get-ChildItem "$fx\measure-raw-*.json") {
    $r = Get-Content $f -Raw | ConvertFrom-Json
    $ts = @($r.frames | ForEach-Object { $_[0] })
    $marks = @{}; foreach ($e in $r.events) { if ($e[1] -eq 'mark') { $marks[[int]$e[2]] = $e[0] } }
    function Phase($a, $b) { Stats @(for ($i = 1; $i -lt $ts.Count; $i++) { if ($ts[$i - 1] -ge $a -and $ts[$i] -le $b) { $ts[$i] - $ts[$i - 1] } }) }
    if (-not $marks.Count) {   # manual mode: real input, no injector steps
        $all = @(for ($i = 1; $i -lt $ts.Count; $i++) { $ts[$i] - $ts[$i - 1] })
        $modes[$r.mode] = [ordered]@{ screen = $r.screen; scale = $r.scale; cadence = Stats $all; events = $r.events }
        continue
    }
    $desc = @{}; foreach ($l in $r.inject) { if ($l[1] -match '^mark (\d+) (.*)$') { $desc[[int]$Matches[1]] = $Matches[2] } }
    # events grouped by injector step; times relative to the step marker
    $steps = @(); $cur = $null
    foreach ($e in $r.events) {
        if ($e[1] -eq 'mark') { $cur = [ordered]@{ step = [int]$e[2]; what = $desc[[int]$e[2]]; events = [System.Collections.ArrayList]@() }; $steps += $cur; continue }
        if ($cur -and $e[1] -ne 'OnExit' -and $e[1] -ne 'CanExit') { [void]$cur.events.Add(@(@($e[0] - $marks[$cur.step]) + @($e | Select-Object -Skip 1))) }
    }
    $states = $r.frames | ForEach-Object { ($_ | Select-Object -Skip 1) -join ',' } | Group-Object | ForEach-Object { [ordered]@{ layer_sub_r_g_b_a = $_.Name; frames = $_.Count } }
    $modes[$r.mode] = [ordered]@{
        screen = $r.screen; scale = $r.scale; paths = $r.paths; injector = $r.inject[1][1]
        cadence = [ordered]@{
            idle_static = Phase ($ts[0] + 500) $marks[1]; animated = Phase ($marks[1] + 100) $marks[2]
            during_input = Phase $marks[3] $marks[17]; minimized = Phase ($marks[18] + 300) $marks[19]
            minimized_gap_ms = (@(for ($i = 1; $i -lt $ts.Count; $i++) { $ts[$i] - $ts[$i - 1] }) | Measure-Object -Maximum).Maximum
            after_restore = Phase ($marks[19] + 300) $marks[26]
        }
        frame_start_state = @($states); input_steps = $steps
    }
}
$audits = [ordered]@{}
foreach ($a in Get-ChildItem "$fx\audit\*.json") { $audits[$a.BaseName] = Get-Content $a -Raw | ConvertFrom-Json }
[ordered]@{
    generated_by = 'tools/sg-oracle/summarize-measure.ps1'; exe = 'Path{space}of{space}Building.exe 2.67.2, SimpleGraphic v2.5-3b1a346 (sandbox copy)'
    modes = $modes; audits = $audits
} | ConvertTo-Json -Depth 12 | Set-Content "$fx\measure.json"
"measure.json: modes $($modes.Keys -join ', '); audits $($audits.Count)"
