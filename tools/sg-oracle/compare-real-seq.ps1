# U8b2: one input scenario for the real window of both sides on the Compare tab, dot-sourced by compare-rust-input.ps1 and inject-compare-real.ps1. They give: P(msg, w, l) (PostMessage to the side's
# own HWND), Cursor(x, y) (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results.
function Run-CompareRealInput {
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
    $b = $r.start.compareReal.modeCompare
    Click $b.x $b.y; Wait 500
    $r.tab = State
    # the import popup; two digits in its field; a code that is no code
    $e = $r.tab.compareReal.importBtn
    Click $e.x $e.y; Wait 500
    $r.popup = State
    $e = $r.popup.compareReal.input
    Click $e.x $e.y; Wait 300
    Key 0x31 0x02
    Key 0x32 0x03
    Wait 400
    $r.typed = State
    $e = $r.typed.compareReal.go
    Click $e.x $e.y; Wait 500
    $r.bad = State
    # Import from Folder: the second row, its Open button
    $e = $r.bad.compareReal.importFolder
    Click $e.x $e.y; Wait 500
    $r.folder = State
    $e = $r.folder.compareReal.folderRow2
    Click $e.x $e.y; Wait 400
    $r.row = State
    $e = $r.row.compareReal.folderOpen
    Click $e.x $e.y; Wait 2500
    $r.imported = State
    # the wheel over the summary
    Goto-Xy 700 500; Wait 200
    foreach ($k in 1..3) { P 0x020A ((-120 -band 0xFFFF) -shl 16) (XY 700 500); Wait 120 }
    Wait 500
    $r.wheel = State
    # the Items view
    $e = $r.wheel.compareReal.subItems
    Click $e.x $e.y; Wait 800
    $r.items = State
    # the Config view: a digit in its search
    $e = $r.items.compareReal.subConfig
    Click $e.x $e.y; Wait 800
    $r.config = State
    $e = $r.config.compareReal.configSearch
    Click $e.x $e.y; Wait 300
    Key 0x33 0x04
    Wait 500
    $r.searched = State
    # the Tree view, then Remove
    $e = $r.searched.compareReal.subTree
    Click $e.x $e.y; Wait 1500
    $r.tree = State
    $e = $r.tree.compareReal.removeBtn
    Click $e.x $e.y; Wait 500
    $r.removed = State
    $r
}
