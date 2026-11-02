# U1.4: F10 key-up probe for lua/input-probe.lua (tag "f10" / "f10in"; PostMessage to the sandbox exe's own window only),
# then a posted click and a screenshot (mark 25) of the window state after the F-keys.
#   pwsh tools/sg-oracle/run.ps1 -Script input-probe.lua -ScriptArgs f10 -Inject inject-f10.ps1
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name F -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
$Process.Refresh(); $h = $Process.MainWindowHandle
function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.F]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
$n = 0
function Mark($what) { $script:n++; P 0x0102 (64 + $script:n) 1; L "mark $script:n $what"; Start-Sleep -Milliseconds 150 }
function K([uint32]$down, [uint32]$up, [int]$vk, [int]$sc) { P $down $vk (1 -bor ($sc -shl 16)); Start-Sleep -Milliseconds 60; P $up $vk (1 -bor ($sc -shl 16) -bor 0xC0000000); Start-Sleep -Milliseconds 300 }
Start-Sleep -Milliseconds 300
Mark 'F10 via WM_SYSKEYDOWN/WM_SYSKEYUP'; K 0x0104 0x0105 0x79 0x44
Mark 'F10 via WM_KEYDOWN/WM_KEYUP'; K 0x0100 0x0101 0x79 0x44
Mark 'F9 via WM_KEYDOWN/WM_KEYUP'; K 0x0100 0x0101 0x78 0x43
Mark 'F10 via WM_KEYDOWN/WM_KEYUP again'; K 0x0100 0x0101 0x79 0x44
Mark 'LMB click at 100,100 (lParam), then 1 s'; P 0x0201 1 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 30; P 0x0202 0 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 1000
$script:n = 24; Mark 'TakeScreenshot (Lua), 1.5 s'; Start-Sleep -Milliseconds 1500
L 'end'
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"
$script:n = 25; Mark 'end'
