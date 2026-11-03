# U6: one input scenario for the real window of both sides on the Items tab, dot-sourced by items-rust-input.ps1 and inject-items-real.ps1. They give: P(msg, w, l) (PostMessage to the side's
# own HWND), Cursor(x, y) (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results.
function Run-ItemsRealInput {
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
    $b = $r.start.itemsReal.modeBtn
    Click $b.x $b.y; Wait 500
    $r.tab = State
    # a row of the item list
    $c = $r.tab.itemsReal.rowPos
    Click $c.x $c.y; Wait 500
    $r.row = State
    # posted messages make no double click on either side (the OS cursor is not over the window, GLFW ignores WM_LBUTTONDBLCLK): the editor opens through "Craft item..."
    $e = $r.row.itemsReal.craftPos
    Click $e.x $e.y; Wait 500
    $r.popup = State
    $e = $r.popup.itemsReal.popupSave
    Click $e.x $e.y; Wait 800
    $r.craft = State
    # the search of the unique list: click, two digits, and the filtering frame
    $e = $r.craft.itemsReal.searchPos
    Click $e.x $e.y; Wait 300
    Key 0x31 0x02
    Key 0x32 0x03
    Wait 800
    $r.typed = State
    # the focus away (a click on the empty page); three notches of the wheel over the page
    Click 1100 700; Wait 300
    foreach ($k in 1..3) { P 0x020A ((-120 -band 0xFFFF) -shl 16) (XY 1100 700); Wait 120 }
    Wait 400
    $r.wheel = State
    $r
}
