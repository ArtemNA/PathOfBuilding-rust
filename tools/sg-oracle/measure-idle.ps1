# U3a: CPU and window state of one process over a static tree. The window is made topmost WITHOUT activation (no focus, no input): 30 s visible,
# then pushed to the bottom of the z-order: 10 s (covered when another window lies over it). For the original (run.ps1 -Inject) and for pob-app.
#   -Process <process>  -Out <sandbox sg dir: waits for sg\ready, writes out\tree\idle-measure.json>   or   -OutFile <file: waits 5 s>
param($Process, [string]$Out, [string]$OutFile)
$sig = '[DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr after, int x, int y, int cx, int cy, uint flags);' +
    '[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);' +
    '[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr h);' +
    '[DllImport("kernel32.dll")] public static extern bool QueryProcessCycleTime(System.IntPtr p, out ulong c);' +
    '[DllImport("kernel32.dll")] public static extern bool QueryThreadCycleTime(System.IntPtr t, out ulong c);' +
    '[DllImport("kernel32.dll")] public static extern System.IntPtr GetCurrentThread();'
Add-Type -Namespace U3a -Name W -MemberDefinition $sig
if ($Out) { for ($i = 0; $i -lt 300 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 } } else { Start-Sleep -Milliseconds 5000 }
$h = (Get-Process -Id $Process.Id).MainWindowHandle
$flags = 0x0001 -bor 0x0002 -bor 0x0010  # SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE
[U3a.W]::SetWindowPos($h, [IntPtr]::new(-1), 0, 0, 0, 0, $flags) | Out-Null
Start-Sleep -Milliseconds 2000
# Win32 process times are sampled on the 15.6 ms clock tick: a process that wakes after a tick and sleeps before the next one is charged ~0 (seen: 0 ms over
# 10 s at 30 fps). Cycle counters are exact: cycles per second calibrated by a 1 s busy loop of this thread.
$c0 = [uint64]0; $c1 = [uint64]0; [U3a.W]::QueryThreadCycleTime([U3a.W]::GetCurrentThread(), [ref]$c0) | Out-Null; $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt 1000) { }; [U3a.W]::QueryThreadCycleTime([U3a.W]::GetCurrentThread(), [ref]$c1) | Out-Null
$hz = ($c1 - $c0) / ($sw.ElapsedMilliseconds / 1000)
function Cycles { $c = [uint64]0; [U3a.W]::QueryProcessCycleTime($Process.Handle, [ref]$c) | Out-Null; $c }
function Slice([int]$secs) {
    $cy0 = Cycles
    $a = Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)"; $t = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds $secs
    $b = Get-CimInstance Win32_Process -Filter "ProcessId=$($Process.Id)"
    $cy = (Cycles) - $cy0
    $ms = (($b.UserModeTime + $b.KernelModeTime) - ($a.UserModeTime + $a.KernelModeTime)) / 10000
    [ordered]@{ wall_ms = $t.ElapsedMilliseconds; cpu_ms = [int]$ms; tick_percent = [math]::Round(100 * $ms / $t.ElapsedMilliseconds, 2); cycle_percent_of_one_core = [math]::Round(100 * $cy / ($hz * $t.ElapsedMilliseconds / 1000), 2) }
}
$visible = @(); foreach ($i in 1..6) { $visible += Slice 5 }
$vis = [U3a.W]::IsWindowVisible($h); $icon = [U3a.W]::IsIconic($h)
[U3a.W]::SetWindowPos($h, [IntPtr]::new(-2), 0, 0, 0, 0, $flags) | Out-Null   # HWND_NOTOPMOST
[U3a.W]::SetWindowPos($h, [IntPtr]::new(1), 0, 0, 0, 0, $flags) | Out-Null    # HWND_BOTTOM
Start-Sleep -Milliseconds 1000
$covered = Slice 10
$q = Get-Process -Id $Process.Id
$res = [ordered]@{ window_visible = $vis; iconic = $icon; visible_slices = $visible; bottom_of_z_order = $covered; working_set_mb = [int]($q.WorkingSet64 / 1MB); private_mb = [int]($q.PrivateMemorySize64 / 1MB) } | ConvertTo-Json -Depth 4
if ($OutFile) { $res | Set-Content $OutFile } else { New-Item -ItemType Directory -Force "$Out\out\tree" | Out-Null; $res | Set-Content "$Out\out\tree\idle-measure.json" }
