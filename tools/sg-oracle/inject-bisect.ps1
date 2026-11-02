# U1.4: which posted key makes later posted clicks disappear (window under the idle OS cursor; PostMessage to the
# sandbox exe's own window only). Each mark = one key (or none), then a click at 100,100.
#   pwsh tools/sg-oracle/run.ps1 -Script input-probe.lua -ScriptArgs 'bisect 1080,320' -Inject inject-bisect.ps1
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name B -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
$Process.Refresh(); $h = $Process.MainWindowHandle
function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.B]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
function KL([int]$sc, [bool]$up, [bool]$alt) { $l = 1 -bor (($sc -band 0xFF) -shl 16); if ($sc -band 0x100) { $l = $l -bor 0x01000000 }; if ($alt) { $l = $l -bor 0x20000000 }; if ($up) { $l = $l -bor 0xC0000000 }; $l }
function Key([int]$vk, [int]$sc) { P 0x0100 $vk (KL $sc $false $false); Start-Sleep -Milliseconds 40; P 0x0101 $vk (KL $sc $true $false); Start-Sleep -Milliseconds 80 }
function Click { P 0x0201 1 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 30; P 0x0202 0 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 600 }
$n = 0
function Mark($what) { $script:n++; if ($script:n -in 2, 3) { $script:n = 4 }; P 0x0102 (64 + $script:n) 1; L "mark $script:n $what"; Start-Sleep -Milliseconds 150 }
Start-Sleep -Milliseconds 300
Mark 'nothing'; Click
Mark 'wheel +1 -1 0'; foreach ($d in 1, -1, 0) { P 0x020A (([int64]($d -band 0xFFFF)) -shl 16) (140 -bor (140 -shl 16)); Start-Sleep -Milliseconds 150 }; Click
Mark 'hwheel +120'; P 0x020E (([int64]120) -shl 16) (140 -bor (140 -shl 16)); Start-Sleep -Milliseconds 200; Click
Mark 'WM_CHAR 7E A0 80 20'; foreach ($c in 0x7E, 0xA0, 0x80, 0x20) { P 0x0102 $c 1; Start-Sleep -Milliseconds 60 }; Click
Mark 'LSHIFT down, a, LSHIFT up'; P 0x0100 0x10 (KL 0x2A $false $false); Start-Sleep -Milliseconds 60; Key 0x41 0x1E; P 0x0101 0x10 (KL 0x2A $true $false); Start-Sleep -Milliseconds 150; Click
Mark 'RCTRL RSHIFT RALT'; Key 0x11 0x11D; Key 0x10 0x36; P 0x0104 0x12 (KL 0x138 $false $true); Start-Sleep -Milliseconds 60; P 0x0105 0x12 (KL 0x138 $true $false); Start-Sleep -Milliseconds 200; Click
Mark 'F2..F9'; for ($i = 0; $i -lt 8; $i++) { Key (0x71 + $i) (0x3C + $i) }; Click
Mark 'nav PGUP PGDN END HOME INSERT UP DOWN RIGHT'; foreach ($k in @(@(0x21, 0x149), @(0x22, 0x151), @(0x23, 0x14F), @(0x24, 0x147), @(0x2D, 0x152), @(0x26, 0x148), @(0x28, 0x150), @(0x27, 0x14D))) { Key $k[0] $k[1] }; Click
Mark 'printables 1 = space'; Key 0x31 0x02; Key 0xBB 0x0D; Key 0x20 0x39; Click
Mark 'a x3 repeat'; P 0x0100 0x41 (KL 0x1E $false $false); Start-Sleep -Milliseconds 40; P 0x0100 0x41 ((KL 0x1E $false $false) -bor 0x40000000); Start-Sleep -Milliseconds 40; P 0x0101 0x41 (KL 0x1E $true $false); Start-Sleep -Milliseconds 150; Click
L 'end'
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"
$script:n = 25; Mark 'end'
