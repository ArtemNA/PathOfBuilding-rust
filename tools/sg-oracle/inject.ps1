# Posts synthetic input to the sandbox exe's own window (PostMessage to its HWND only: no SendInput, the real mouse/keyboard
# and other windows are untouched). Called by run.ps1 -Inject inject.ps1 for lua/measure.lua.
# Each step is preceded by WM_CHAR chr(64+n) = "A".."Y" (step marker; tests only produce lowercase chars), "Z" = end; logged to <Out>/inject.json with a local timestamp.
param($Process, [string]$Out)   # $Out = runtime/sg (script dir); handshake files live there, not in out/
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name W -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint tid);
[DllImport("user32.dll")] public static extern uint GetDoubleClickTime();
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
'@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
L "ready seen (lua wrote sg/out/ready at frame 3)"
$Process.Refresh(); $h = $Process.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { throw 'no main window' }
$tid = 0; $pidOut = 0; $tid = [SgOracle.W]::GetWindowThreadProcessId($h, [ref]$pidOut)
$hkl = '{0:X8}' -f ([SgOracle.W]::GetKeyboardLayout($tid).ToInt64() -band 0xFFFFFFFF)
L "hwnd=$h pid=$pidOut layout=$hkl dblclick=$([SgOracle.W]::GetDoubleClickTime()) foreground=$([SgOracle.W]::GetForegroundWindow() -eq $h)"

function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.W]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
$n = 0
function Mark($what) { $script:n++; P 0x0102 (64 + $script:n) 1; L "mark $script:n $what"; Start-Sleep -Milliseconds 150 }
function Key([int]$vk, [int]$sc) {
    P 0x0100 $vk (1 -bor ($sc -shl 16)); Start-Sleep -Milliseconds 60
    P 0x0101 $vk (1 -bor ($sc -shl 16) -bor 0xC0000000); Start-Sleep -Milliseconds 120
}
function Click([int]$down, [int]$up, [int]$mk) { $xy = 100 -bor (100 -shl 16); P $down $mk $xy; Start-Sleep -Milliseconds 30; P $up 0 $xy }
function Wheel([int]$delta) { P 0x020A (([int64]($delta -band 0xFFFF)) -shl 16) ((140 -shl 16) -bor 140); Start-Sleep -Milliseconds 200 }

Start-Sleep -Milliseconds 2000; Mark 'anim on (idle 2 s before)'          # 1
Start-Sleep -Milliseconds 2000; Mark 'anim off'                           # 2
Start-Sleep -Milliseconds 500;  Mark 'wheel +120 -120 +240 +60 +60'       # 3
Wheel 120; Wheel -120; Wheel 240; Wheel 60; Wheel 60
Mark 'key A (vk 41 sc 1E)'; Key 0x41 0x1E                                 # 4
Mark 'ctrl down, key Z, ctrl up'                                          # 5
P 0x0100 0x11 (1 -bor (0x1D -shl 16)); Start-Sleep -Milliseconds 60; Key 0x5A 0x2C; P 0x0101 0x11 (1 -bor (0x1D -shl 16) -bor 0xC0000000)
Mark 'BACK'; Key 0x08 0x0E                                                # 6
Mark 'RETURN'; Key 0x0D 0x1C                                              # 7
Mark 'TAB'; Key 0x09 0x0F                                                 # 8
Mark 'ESCAPE'; Key 0x1B 0x01                                              # 9
Mark 'F1, DELETE(ext), LEFT(ext)'; Key 0x70 0x3B; Key 0x2E (0x153); Key 0x25 (0x14B)   # 10
Mark 'WM_CHAR U+044F U+00E9 U+20AC 0x1A 0x08 0x7F 0x0D'                    # 11
foreach ($c in 0x44F, 0xE9, 0x20AC, 0x1A, 0x08, 0x7F, 0x0D) { P 0x0102 $c 1; Start-Sleep -Milliseconds 80 }
Mark 'LMB x2, 80 ms apart'                                                # 12
Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 80; Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 700
Mark 'LMB x2, 300 ms apart'                                               # 13
Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 300; Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 700
Mark 'LMB x2, 650 ms apart'                                               # 14
Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 650; Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 700
Mark 'LMB x3, 80 ms apart'                                                # 15
Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 80; Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 80; Click 0x0201 0x0202 1; Start-Sleep -Milliseconds 700
Mark 'RMB x2, 80 ms; MMB x1'                                              # 16
Click 0x0204 0x0205 2; Start-Sleep -Milliseconds 80; Click 0x0204 0x0205 2; Start-Sleep -Milliseconds 200; Click 0x0207 0x0208 0x10
Start-Sleep -Milliseconds 500
Mark 'LMB down,up,WM_LBUTTONDBLCLK,up (real double-click sequence with CS_DBLCLKS)'   # 17
P 0x0201 1 (100 -bor (100 -shl 16)); P 0x0202 0 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 80
P 0x0203 1 (100 -bor (100 -shl 16)); P 0x0202 0 (100 -bor (100 -shl 16)); Start-Sleep -Milliseconds 700
Mark 'minimize 2 s'                                                       # 18
[void][SgOracle.W]::ShowWindow($h, 6); L 'minimized'; Start-Sleep -Milliseconds 2000
Mark 'restore'                                              # 19
[void][SgOracle.W]::ShowWindow($h, 9); L 'restored (SW_RESTORE)'; Start-Sleep -Milliseconds 2000
L 'end'
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"   # read and embedded by measure.lua
$script:n = 25; Mark 'end'                                                # 26 = 'Z' ends the Lua side
