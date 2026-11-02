# U7: one input scenario for the real window of both sides on the Config tab, dot-sourced by config-rust-input.ps1 and inject-config-real.ps1. They give: P(msg, w, l) (PostMessage to the side's
# own HWND), Cursor(x, y) (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results.
function Run-ConfigRealInput {
    function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
    function Goto-Xy([int]$x, [int]$y) { Cursor $x $y; P 0x0200 0 (XY $x $y) }
    function Click([int]$x, [int]$y) { Goto-Xy $x $y; Wait 150; P 0x0201 1 (XY $x $y); Wait 80; P 0x0202 0 (XY $x $y) }
    # the message loops translate WM_KEYDOWN into WM_CHAR themselves
    function Key([int]$vk, [int]$scan) {
        P 0x0100 $vk ((([int64]$scan) -shl 16) -bor 1); Wait 60
        P 0x0101 $vk ((([int64]$scan) -shl 16) -bor 0xC0000001); Wait 60
    }
    $r = [ordered]@{}
    $r.start = State
    # the tab
    $b = $r.start.configReal.modeBtn
    Click $b.x $b.y; Wait 500
    $r.tab = State
    # a check option, and its recalculation frame
    $c = $r.tab.configReal.checkPos
    Click $c.x $c.y; Wait 900
    $r.check = State
    # a number option: click, two digits
    $e = $r.check.configReal.editPos
    Click $e.x $e.y; Wait 200
    Key 0x32 0x03
    Key 0x35 0x06
    Wait 900
    $r.typed = State
    # the focus away; three notches of the wheel over the page
    Key 0x1B 0x01
    Wait 200
    Goto-Xy 800 400; Wait 150
    foreach ($k in 1..3) { P 0x020A ((-120 -band 0xFFFF) -shl 16) (XY 800 400); Wait 120 }
    Wait 400
    $r.wheel = State
    $r
}
