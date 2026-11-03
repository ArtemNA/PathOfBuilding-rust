# U8a: one input scenario for the real window of both sides on the Import and Party tabs, dot-sourced by import-rust-input.ps1 and inject-import-real.ps1. They give: P(msg, w, l) (PostMessage to the side's
# own HWND), Cursor(x, y) (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results.
function Run-ImportRealInput {
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
    $b = $r.start.importReal.modeImport
    Click $b.x $b.y; Wait 500
    $r.tab = State
    # the code of the build
    $e = $r.tab.importReal.generate
    Click $e.x $e.y; Wait 800
    $r.generate = State
    # an account name: click, two digits
    $e = $r.generate.importReal.account
    Click $e.x $e.y; Wait 300
    Key 0x31 0x02
    Key 0x32 0x03
    Wait 500
    $r.typed = State
    # the Party tab, its advanced view, a key in the focused buffer
    $b = $r.typed.importReal.modeParty
    Click $b.x $b.y; Wait 500
    $r.party = State
    $e = $r.party.importReal.advanced
    Click $e.x $e.y; Wait 500
    $r.advanced = State
    $e = $r.advanced.importReal.auras
    Click $e.x $e.y; Wait 300
    Key 0x33 0x04
    Wait 500
    $r.auras = State
    $r
}
