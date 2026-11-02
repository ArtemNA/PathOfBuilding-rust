# U3b: the real input path of the Rust window on the public golden: PostMessage to pob-app's own HWND (only) -> winit -> OnKeyDown / OnKeyUp -> the tree view.
# Hover, click-allocate, Ctrl+Z, drag, wheel; the state comes from pob-app --state-file. A sandbox user folder in the temp dir; nothing touches the real mouse or keyboard.
#   pwsh tools/sg-oracle/rust-input.ps1 [-Exe target/release/pob-app.exe]
param([string]$Exe = "target/release/pob-app.exe")
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name R -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$up = Get-Content (Join-Path $PSScriptRoot '../upstream.json') -Raw | ConvertFrom-Json
$root = Join-Path ([IO.Path]::GetTempPath()) ("pob-rust-input-" + $PID)
$builds = Join-Path $root 'Path of Building (Rust)/Builds'
New-Item -ItemType Directory -Force $builds | Out-Null
$name = 'Generals Perforate Zerker'
Copy-Item "$($up.pobRoot)/spec/TestBuilds/3.13/$name.xml" (Join-Path $builds "$name.xml")
$stateFile = Join-Path $root 'state.json'
$p = Start-Process -FilePath $Exe -ArgumentList @('--user-path', "`"$root`"", '--open', "`"$name`"", '--state-file', "`"$stateFile`"", '--secs', '60') -PassThru
function State { for ($i = 0; $i -lt 50; $i++) { try { return Get-Content $stateFile -Raw | ConvertFrom-Json } catch { Start-Sleep -Milliseconds 20 } } throw 'no state' }
function Wait($ms = 250) { Start-Sleep -Milliseconds $ms }
$results = [ordered]@{}
try {
    for ($i = 0; $i -lt 300; $i++) { if ((Test-Path $stateFile) -and (State).ready) { break }; Start-Sleep -Milliseconds 100 }
    if (-not (State).ready) { throw 'the build did not open' }
    Wait 1500
    $p.Refresh(); $h = $p.MainWindowHandle
    function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.R]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
    function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
    # the wheel: three notches out at the centre of the view (WM_MOUSEWHEEL, the view takes the cursor winit last saw)
    $s = State
    P 0x0200 0 (XY 796 372); Wait 100
    foreach ($k in 1..3) { P 0x020A ((-120 -band 0xFFFF) -shl 16) (XY 796 372); Wait 100 }
    Wait 300
    $s0 = State
    $results.wheelOut = @{ before = $s.zoomLevel; after = $s0.zoomLevel; ok = ($s0.zoomLevel -eq $s.zoomLevel - 3) }
    $t = $s0.target
    if (-not $t) { throw 'no target node on screen' }
    # hover
    P 0x0200 0 (XY $t.x $t.y); Wait
    $s1 = State
    $results.hover = @{ want = $t.id; got = $s1.hover; ok = ($s1.hover -eq $t.id) }
    # click-allocate
    P 0x0201 1 (XY $t.x $t.y); Wait 60; P 0x0202 0 (XY $t.x $t.y); Wait 600
    $s2 = State
    $results.alloc = @{ before = $s0.alloc; after = $s2.alloc; ok = ($s2.alloc -eq $s0.alloc + $t.path) }
    # Ctrl+Z (VK_CONTROL 0x11 with its scan code, 'Z' 0x5A)
    P 0x0100 0x11 ((0x1D -shl 16) -bor 1); Wait 60
    P 0x0100 0x5A ((0x2C -shl 16) -bor 1); Wait 60
    P 0x0101 0x5A ((0x2C -shl 16) -bor 0xC0000001); Wait 60
    P 0x0101 0x11 ((0x1D -shl 16) -bor 0xC0000001); Wait 600
    $s3 = State
    $results.undo = @{ want = $s0.alloc; got = $s3.alloc; ok = ($s3.alloc -eq $s0.alloc) }
    # drag on an empty part of the view
    $x0 = 1100; $y0 = 100
    P 0x0200 0 (XY $x0 $y0); Wait 60
    P 0x0201 1 (XY $x0 $y0); Wait 100
    P 0x0200 1 (XY ($x0 - 40) ($y0 + 30)); Wait 150
    P 0x0202 0 (XY ($x0 - 40) ($y0 + 30)); Wait 300
    $s4 = State
    $results.drag = @{ dx = $s4.zoomX - $s3.zoomX; dy = $s4.zoomY - $s3.zoomY; ok = ([Math]::Abs($s4.zoomX - $s3.zoomX + 40) -lt 1e-6 -and [Math]::Abs($s4.zoomY - $s3.zoomY - 30) -lt 1e-6) }
    # the wheel: lParam of WM_MOUSEWHEEL is in screen coordinates; the view takes the cursor winit last saw
    P 0x020A ((120 -shl 16)) (XY 900 300); Wait 300
    $s5 = State
    $results.wheel = @{ before = $s4.zoomLevel; after = $s5.zoomLevel; ok = ($s5.zoomLevel -eq $s4.zoomLevel + 1) }
} finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    $results | ConvertTo-Json -Compress
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
