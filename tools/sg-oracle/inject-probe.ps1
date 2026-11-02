# U1.4: posts synthetic input to the sandbox exe's own window for lua/input-probe.lua (PostMessage to its HWND only: no
# SendInput, the real mouse/keyboard, the keyboard layout and other windows are untouched).
#   pwsh tools/sg-oracle/run.ps1 -Script input-probe.lua -Inject inject-probe.ps1
# Each step is preceded by WM_CHAR chr(64+n) = "A".."Y" (step marker), "Z" = end; logged to <Out>/inject.json.
# Keys are posted as WM_KEYDOWN/WM_KEYUP with the set-1 scancode in lParam (GLFW maps keys by scancode; 0x1xx = extended).
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name P -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint tid);
[DllImport("user32.dll")] public static extern uint GetDoubleClickTime();
[DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
'@
$log = [System.Collections.Generic.List[object]]::new()
$sw = [Diagnostics.Stopwatch]::StartNew()
function L($what) { $log.Add(@($sw.ElapsedMilliseconds, $what)) }
for ($i = 0; $i -lt 100 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
L 'ready seen'
$Process.Refresh(); $h = $Process.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { throw 'no main window' }
$pidOut = 0; $tid = [SgOracle.P]::GetWindowThreadProcessId($h, [ref]$pidOut)
$hkl = '{0:X8}' -f ([SgOracle.P]::GetKeyboardLayout($tid).ToInt64() -band 0xFFFFFFFF)
# SM_CXDOUBLECLK = 36, SM_CYDOUBLECLK = 37
L "hwnd=$h pid=$pidOut layout=$hkl dblclick=$([SgOracle.P]::GetDoubleClickTime()) dblrect=$([SgOracle.P]::GetSystemMetrics(36))x$([SgOracle.P]::GetSystemMetrics(37))"

function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.P]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
$n = 0
function Mark($what) { $script:n++; P 0x0102 (64 + $script:n) 1; L "mark $script:n $what"; Start-Sleep -Milliseconds 150 }
function KL([int]$sc, [bool]$up, [bool]$alt) {
    $l = 1 -bor (($sc -band 0xFF) -shl 16)
    if ($sc -band 0x100) { $l = $l -bor 0x01000000 }
    if ($alt) { $l = $l -bor 0x20000000 }
    if ($up) { $l = $l -bor 0xC0000000 }
    $l
}
function Down([int]$vk, [int]$sc) { P 0x0100 $vk (KL $sc $false $false) }
function Up([int]$vk, [int]$sc) { P 0x0101 $vk (KL $sc $true $false) }
function Key([int]$vk, [int]$sc) { Down $vk $sc; Start-Sleep -Milliseconds 40; Up $vk $sc; Start-Sleep -Milliseconds 80 }
function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
function MoveTo([int]$x, [int]$y) { P 0x0200 0 (XY $x $y); Start-Sleep -Milliseconds 30 }
function Click([int]$down, [int]$up, [int64]$mk, [int]$x, [int]$y) { P $down $mk (XY $x $y); Start-Sleep -Milliseconds 20; P $up ($mk -band 0xFFFF0000) (XY $x $y) }

Start-Sleep -Milliseconds 500
Mark 'MoveTo 150,120 at 100%'                                                   # 1
MoveTo 150 120; Start-Sleep -Milliseconds 300
Mark 'dpi 150 (next frame), MoveTo 151,121 then 3,4'                            # 2
Start-Sleep -Milliseconds 300; MoveTo 151 121; Start-Sleep -Milliseconds 200; MoveTo 3 4; Start-Sleep -Milliseconds 300
Mark 'dpi 100'                                                                # 3
Start-Sleep -Milliseconds 300
Mark 'move+LMB x2 80 ms same pos'                                             # 4
MoveTo 100 100; Click 0x0201 0x0202 1 100 100; Start-Sleep -Milliseconds 80; MoveTo 100 100; Click 0x0201 0x0202 1 100 100; Start-Sleep -Milliseconds 700
Mark 'LMB x2 10 ms'                                                           # 5
Click 0x0201 0x0202 1 100 100; Start-Sleep -Milliseconds 10; Click 0x0201 0x0202 1 100 100; Start-Sleep -Milliseconds 700
Mark 'LMB x2 80 ms, second moved +10 px'                                      # 6
MoveTo 100 100; Click 0x0201 0x0202 1 100 100; Start-Sleep -Milliseconds 80; MoveTo 110 100; Click 0x0201 0x0202 1 110 100; Start-Sleep -Milliseconds 700
Mark 'RMB x2 80 ms with move'                                                 # 7
MoveTo 120 100; Click 0x0204 0x0205 2 120 100; Start-Sleep -Milliseconds 80; MoveTo 120 100; Click 0x0204 0x0205 2 120 100; Start-Sleep -Milliseconds 700
Mark 'XBUTTON1, XBUTTON2'                                                     # 8
Click 0x020B 0x020C (0x10000 -bor 0x20) 120 100; Start-Sleep -Milliseconds 150; Click 0x020B 0x020C (0x20000 -bor 0x40) 120 100; Start-Sleep -Milliseconds 300
Mark 'LSHIFT down, a, LSHIFT up'                                              # 9
Down 0x10 0x2A; Start-Sleep -Milliseconds 60; Key 0x41 0x1E; Up 0x10 0x2A; Start-Sleep -Milliseconds 150
Mark 'LALT via WM_SYSKEYDOWN/UP'                                              # 10
P 0x0104 0x12 (KL 0x38 $false $true); Start-Sleep -Milliseconds 80; P 0x0105 0x12 (KL 0x38 $true $false); Start-Sleep -Milliseconds 200
Mark 'RCTRL, RSHIFT, RALT(sys)'                                               # 11
Key 0x11 0x11D; Key 0x10 0x36; P 0x0104 0x12 (KL 0x138 $false $true); Start-Sleep -Milliseconds 60; P 0x0105 0x12 (KL 0x138 $true $false); Start-Sleep -Milliseconds 200
Mark 'nav: PAUSE PGUP PGDN END HOME INSERT UP DOWN RIGHT NUMLOCK SCROLLLOCK PRINTSCREEN'   # 12
foreach ($k in @(@(0x13, 0x45), @(0x21, 0x149), @(0x22, 0x151), @(0x23, 0x14F), @(0x24, 0x147), @(0x2D, 0x152), @(0x26, 0x148),
        @(0x28, 0x150), @(0x27, 0x14D), @(0x90, 0x145), @(0x91, 0x46), @(0x2C, 0x137))) { Key $k[0] $k[1] }
Mark 'F2..F15'                                                                # 13
$fsc = @(0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x57, 0x58, 0x64, 0x65, 0x66)
for ($i = 0; $i -lt 14; $i++) { Key (0x71 + $i) $fsc[$i] }
Mark 'printables 1 0 - = [ ] ; '' ` \ , . / space q m'                        # 14
foreach ($k in @(@(0x31, 0x02), @(0x30, 0x0B), @(0xBD, 0x0C), @(0xBB, 0x0D), @(0xDB, 0x1A), @(0xDD, 0x1B), @(0xBA, 0x27), @(0xDE, 0x28),
        @(0xC0, 0x29), @(0xDC, 0x2B), @(0xBC, 0x33), @(0xBE, 0x34), @(0xBF, 0x35), @(0x20, 0x39), @(0x51, 0x10), @(0x4D, 0x32))) { Key $k[0] $k[1] }
Mark 'numpad 0 1 9 * + - . / ENTER'                                           # 15
foreach ($k in @(@(0x60, 0x52), @(0x61, 0x4F), @(0x69, 0x49), @(0x6A, 0x37), @(0x6B, 0x4E), @(0x6D, 0x4A), @(0x6E, 0x53), @(0x6F, 0x135), @(0x0D, 0x11C))) { Key $k[0] $k[1] }
Mark 'CAPSLOCK, LWIN, APPS'                                                   # 16
Key 0x14 0x3A; Key 0x5B 0x15B; Key 0x5D 0x15D
Mark 'a down x3 (auto-repeat bit), a up'                                      # 17
Down 0x41 0x1E; Start-Sleep -Milliseconds 40
P 0x0100 0x41 ((KL 0x1E $false $false) -bor 0x40000000); Start-Sleep -Milliseconds 40
P 0x0100 0x41 ((KL 0x1E $false $false) -bor 0x40000000); Start-Sleep -Milliseconds 40; Up 0x41 0x1E; Start-Sleep -Milliseconds 150
Mark 'wheel +1, -1, 0; hwheel +120'                                           # 18
foreach ($d in 1, -1, 0) { P 0x020A (([int64]($d -band 0xFFFF)) -shl 16) (XY 140 140); Start-Sleep -Milliseconds 150 }
P 0x020E (([int64]120) -shl 16) (XY 140 140); Start-Sleep -Milliseconds 200
Mark 'WM_CHAR D83D DE00 (surrogates), 7E, A0, 80, 20'                         # 19
foreach ($c in 0xD83D, 0xDE00, 0x7E, 0xA0, 0x80, 0x20) { P 0x0102 $c 1; Start-Sleep -Milliseconds 60 }
Start-Sleep -Milliseconds 150
Mark 'LMB down, MoveTo 130,110, LMB up at 130,110'                              # 20
MoveTo 100 100; P 0x0201 1 (XY 100 100); Start-Sleep -Milliseconds 60; MoveTo 130 110; Start-Sleep -Milliseconds 60; P 0x0202 0 (XY 130 110); Start-Sleep -Milliseconds 200
L 'end'
$log | ConvertTo-Json -Compress | Set-Content "$Out\inject.json"
$script:n = 25; Mark 'end'                                                    # 26 = 'Z'
