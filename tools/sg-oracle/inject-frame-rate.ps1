# For lua/frame-rate-probe.lua: phases of WM_MOUSEMOVE posted to the sandbox exe's own window only, at a fixed rate, with and without a held button.
#   pwsh tools/sg-oracle/run.ps1 -Script frame-rate-probe.lua -Inject inject-frame-rate.ps1
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name R -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
Start-Sleep -Milliseconds 500
$Process.Refresh(); $h = $Process.MainWindowHandle
function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.R]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
function Mark([int]$n) { P 0x0102 (64 + $n) 1 }
# Posts moves every $periodMs for $ms, spinning: Start-Sleep cannot wait 1 ms.
function Moves([double]$periodMs, [int]$ms, [int64]$wparam) {
    $sw = [Diagnostics.Stopwatch]::StartNew(); $next = 0.0; $i = 0
    while ($sw.ElapsedMilliseconds -lt $ms) {
        if ($sw.Elapsed.TotalMilliseconds -ge $next) {
            $i++
            P 0x0200 $wparam ([int64](200 + $i % 400) -bor ([int64](300 + $i % 200) -shl 16))
            $next += $periodMs
        }
    }
}
Mark 1; Start-Sleep -Milliseconds 2000
Mark 2; Moves 8 2500 0
Mark 3; Moves 1 2500 0
Mark 4; P 0x0201 1 ([int64]200 -bor ([int64]300 -shl 16)); Moves 8 2500 1
Mark 5; Moves 1 2500 1; P 0x0202 0 ([int64]200 -bor ([int64]300 -shl 16))
Mark 6; Start-Sleep -Milliseconds 1000
P 0x0102 90 1
