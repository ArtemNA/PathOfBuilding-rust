# U3c: one input scenario for the real window of both sides, dot-sourced by rust-input.ps1 and inject-real.ps1. They give: P(msg, w, l) (PostMessage to the side's own HWND), Cursor(x, y)
# (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results.
function Run-RealInputScenario {
    function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
    function Goto-Xy([int]$x, [int]$y, [int64]$w = 0) { Cursor $x $y; P 0x0200 $w (XY $x $y) }
    $r = [ordered]@{}
    $r.start = State
    # the wheel: three notches out at the centre of the view
    Goto-Xy 796 372; Wait 150
    foreach ($k in 1..3) { P 0x020A ((-120 -band 0xFFFF) -shl 16) (XY 796 372); Wait 120 }
    Wait 300
    $r.wheelOut = State
    $t = $r.wheelOut.target
    if (-not $t) { throw 'no target node on screen' }
    # hover, click-allocate
    Goto-Xy $t.x $t.y; Wait 300
    $r.hover = State
    P 0x0201 1 (XY $t.x $t.y); Wait 80; P 0x0202 0 (XY $t.x $t.y); Wait 900
    $r.alloc = State
    # Ctrl+Z (VK_CONTROL with its scan code, 'Z')
    P 0x0100 0x11 ((0x1D -shl 16) -bor 1); Wait 80
    P 0x0100 0x5A ((0x2C -shl 16) -bor 1); Wait 80
    P 0x0101 0x5A ((0x2C -shl 16) -bor 0xC0000001); Wait 80
    P 0x0101 0x11 ((0x1D -shl 16) -bor 0xC0000001); Wait 900
    $r.undo = State
    # drag on an empty part of the view
    Goto-Xy 1100 100; Wait 80
    P 0x0201 1 (XY 1100 100); Wait 120
    Goto-Xy 1060 130 1; Wait 200
    P 0x0202 0 (XY 1060 130); Wait 300
    $r.drag = State
    # the wheel in, at the cursor
    P 0x020A (120 -shl 16) (XY 1060 130); Wait 300
    $r.wheelIn = State
    $r
}

