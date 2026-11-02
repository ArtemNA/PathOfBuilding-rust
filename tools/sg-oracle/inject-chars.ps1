# U1.4: WM_CHAR code points posted to the sandbox exe's own window for lua/input-probe.lua in "chars" mode (PostMessage
# only). No step marks in this mode (upper-case letters are data): every code point is preceded by a '~' (0x7E) delimiter;
# the run ends when this script writes <Out>/done. Before the chars: 'a' held while IsKeyDown("A") is sampled.
#   pwsh tools/sg-oracle/run.ps1 -Script input-probe.lua -ScriptArgs chars -Inject inject-chars.ps1
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name C -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
$Process.Refresh(); $h = $Process.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { throw 'no main window' }
$fails = 0
function P([uint32]$msg, [int64]$w, [int64]$l) { if (-not [SgOracle.C]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l)) { $script:fails++ } }
Start-Sleep -Milliseconds 300
L "a down (IsKeyDown('A') sampled), a up"
P 0x0100 0x41 (1 -bor (0x1E -shl 16)); Start-Sleep -Milliseconds 100; P 0x0101 0x41 (1 -bor (0x1E -shl 16) -bor 0xC0000000); Start-Sleep -Milliseconds 300
$ranges = @(@(0x80, 0x2FF), @(0x370, 0x4FF), @(0x2000, 0x20CF), @(0x2100, 0x215F), @(0xFF00, 0xFF5F))
$cps = foreach ($r in $ranges) { $r[0]..$r[1] }
L "code points: $($cps.Count) in ranges $(($ranges | % { '{0:X}-{1:X}' -f $_[0], $_[1] }) -join ' ')"
$k = 0
foreach ($c in $cps) { P 0x0102 0x7E 1; P 0x0102 $c 1; $k++; if ($k % 8 -eq 0) { Start-Sleep -Milliseconds 20 } }
P 0x0102 0x7E 1
Start-Sleep -Milliseconds 1500
L "end; PostMessage failures: $fails"
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"
Set-Content "$Out\done" 'done'
