# U1.4: mouse-only probe for lua/input-probe.lua (tag "drag", window under the idle OS cursor; PostMessage to the sandbox
# exe's own window only): click, drag (down, WM_MOUSEMOVE, up), click — as step 20 of inject-probe.ps1 without the
# keyboard steps before it.
#   pwsh tools/sg-oracle/run.ps1 -Script input-probe.lua -ScriptArgs 'drag 1080,320' -Inject inject-drag.ps1
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name D -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
$Process.Refresh(); $h = $Process.MainWindowHandle
function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.D]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
$n = 0
function Mark($what) { $script:n++; P 0x0102 (64 + $script:n) 1; L "mark $script:n $what"; Start-Sleep -Milliseconds 150 }
Start-Sleep -Milliseconds 300
Mark 'click at 100,100'; P 0x0201 1 (XY 100 100); Start-Sleep -Milliseconds 30; P 0x0202 0 (XY 100 100); Start-Sleep -Milliseconds 700
Mark 'move 100,100, down, 60, move 130,110, 60, up at 130,110'
P 0x0200 0 (XY 100 100); Start-Sleep -Milliseconds 30; P 0x0201 1 (XY 100 100); Start-Sleep -Milliseconds 60; P 0x0200 1 (XY 130 110); Start-Sleep -Milliseconds 90; P 0x0202 0 (XY 130 110); Start-Sleep -Milliseconds 700
Mark 'click at 100,100'; P 0x0201 1 (XY 100 100); Start-Sleep -Milliseconds 30; P 0x0202 0 (XY 100 100); Start-Sleep -Milliseconds 700
Mark 'same drag with WM_MOUSEMOVE wParam 0 (no MK_LBUTTON)'
P 0x0200 0 (XY 100 100); Start-Sleep -Milliseconds 30; P 0x0201 1 (XY 100 100); Start-Sleep -Milliseconds 60; P 0x0200 0 (XY 130 110); Start-Sleep -Milliseconds 90; P 0x0202 0 (XY 130 110); Start-Sleep -Milliseconds 700
L 'end'
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"
$script:n = 25; Mark 'end'
