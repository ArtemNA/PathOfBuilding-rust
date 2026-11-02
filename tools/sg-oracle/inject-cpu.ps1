# U1.5: samples the sandbox exe's CPU time while lua/idle.lua draws a static scene (no input is posted).
param($Process, [string]$Out)
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
Start-Sleep -Milliseconds 2000
$c0 = (Get-Process -Id $Process.Id).TotalProcessorTime.TotalMilliseconds; $t0 = [Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Milliseconds 10000
$q = Get-Process -Id $Process.Id; $c1 = $q.TotalProcessorTime.TotalMilliseconds; $wall = $t0.ElapsedMilliseconds
$ws = [int]($q.WorkingSet64 / 1MB); $priv = [int]($q.PrivateMemorySize64 / 1MB)
[ordered]@{ cpu_ms = [int]($c1 - $c0); wall_ms = $wall; cpu_percent_of_one_core = [math]::Round(100 * ($c1 - $c0) / $wall, 2); working_set_mb = $ws; private_mb = $priv } |
    ConvertTo-Json | Set-Content "$Out\out\idle-cpu.json"
