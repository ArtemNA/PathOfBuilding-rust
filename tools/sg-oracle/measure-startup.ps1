# U9b: start-up time and memory of one process, sampled from outside, the same way for the port and for the original.
#   pob-app:  pwsh tools/sg-oracle/measure-startup.ps1 -Exe target/release/pob-app.exe -ExeArgs '--no-network --user-path <dir> --secs 8' -Out out.json
#   original: pwsh tools/sg-oracle/run.ps1 -Script tree-runner.lua -ScriptArgs idle -Inject measure-startup.ps1 -Timeout 90   (writes sg\out\audit\mem-timeline.json)
# With -Process it samples that process until it exits; with -Exe it starts it (POB_STARTUP_TRACE=1, stderr captured, marks of pob_gfx::trace joined with the samples).
param($Process, [string]$Out, [string]$Exe, [string]$ExeArgs = '', [string]$WorkDir = '.', [int]$EveryMs = 20, [int]$MaxSecs = 120)
$err = $null
if ($Exe) {
    $err = [IO.Path]::GetTempFileName()
    $env:POB_STARTUP_TRACE = '1'
    $Process = Start-Process -FilePath $Exe -ArgumentList $ExeArgs -WorkingDirectory $WorkDir -PassThru -RedirectStandardError $err
}
$p = $Process
$sw = [Diagnostics.Stopwatch]::StartNew()
$h = $p.Handle  # keeps the exit code readable
$samples = [Collections.Generic.List[object]]::new()
while (-not $p.HasExited -and $sw.Elapsed.TotalSeconds -lt $MaxSecs) {
    try {
        $p.Refresh()
        $samples.Add(@([int]$sw.ElapsedMilliseconds, [int]($p.WorkingSet64 / 1MB), [int]($p.PeakWorkingSet64 / 1MB), [int]($p.PrivateMemorySize64 / 1MB), [int]$p.Threads.Count))
    } catch { break }
    Start-Sleep -Milliseconds $EveryMs
}
$marks = @(); $lines = @()
if ($err) {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    $p.WaitForExit(20000) | Out-Null
    $lines = @(Get-Content $err)
    Remove-Item $err
    $marks = @($lines | Where-Object { $_ -match '^mark ' })
}
function At([double]$ms) { foreach ($s in $samples) { if ($s[0] -ge $ms) { return $s } }; $samples[$samples.Count - 1] }
$last = $samples[$samples.Count - 1]
$res = [ordered]@{
    last_sample = @{ ms = $last[0]; ws = $last[1]; peak_ws = $last[2]; private = $last[3]; threads = $last[4] }
    max_private = ($samples | ForEach-Object { $_[3] } | Measure-Object -Maximum).Maximum
    max_threads = ($samples | ForEach-Object { $_[4] } | Measure-Object -Maximum).Maximum
    marks = @($marks | ForEach-Object { $f = $_ -split ' ', 3; $s = At ([double]$f[1]); "$($f[1]) ms  ws $($s[1])  peak $($s[2])  private $($s[3])  threads $($s[4])  $($f[2])" })
    other_stderr = @($lines | Where-Object { $_ -notmatch '^mark ' } | Select-Object -Last 12)
    every_second = @($samples | Group-Object { [int]($_[0] / 1000) } | ForEach-Object { $s = $_.Group[$_.Group.Count - 1]; "$($s[0]) ms  ws $($s[1])  peak $($s[2])  private $($s[3])  threads $($s[4])" })
}
$json = $res | ConvertTo-Json -Depth 4
if ($Exe) { if ($Out) { $json | Set-Content $Out }; $json } else {
    New-Item -ItemType Directory -Force "$Out\out\audit" | Out-Null
    $json | Set-Content "$Out\out\audit\mem-timeline.json"
}
