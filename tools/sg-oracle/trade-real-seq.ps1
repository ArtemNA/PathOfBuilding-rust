# U8d: one input scenario for the real window of both sides on the Trader popup, dot-sourced by trade-rust-input.ps1 and inject-trade-real.ps1. They give: P(msg, w, l) (PostMessage to the side's
# own HWND), Cursor(x, y) (what the side's GetCursorPos will read), State() (the state file) and Wait(ms). Returns the ordered results. No button here sends a request of its own but the popup's opening,
# which neither side delivers: the original's are replaced by the runner, pob-app runs with --no-network.
function Run-TradeRealInput {
    function XY([int]$x, [int]$y) { [int64]$x -bor ([int64]$y -shl 16) }
    function Goto-Xy([int]$x, [int]$y) { Cursor $x $y; P 0x0200 0 (XY $x $y) }
    function Click([int]$x, [int]$y) { Goto-Xy $x $y; Wait 150; P 0x0201 1 (XY $x $y); Wait 80; P 0x0202 0 (XY $x $y) }
    function Key([int]$vk, [int]$scan) {
        P 0x0100 $vk ((([int64]$scan) -shl 16) -bor 1); Wait 60
        P 0x0101 $vk ((([int64]$scan) -shl 16) -bor 0xC0000001); Wait 60
    }
    $r = [ordered]@{}
    $r.start = State
    # the tab
    $b = $r.start.tradeReal.modeItems
    Click $b.x $b.y; Wait 500
    $r.tab = State
    # the popup
    $b = $r.tab.tradeReal.price
    Click $b.x $b.y; Wait 800
    $r.popup = State
    # the field of the pages: click (the caret goes to the click), a digit
    $e = $r.popup.tradeReal.fetch
    Click $e.x $e.y; Wait 300
    Key 0x33 0x04
    Wait 400
    $r.typed = State
    # the weights: a row of the list, the slider, Cancel
    $b = $r.typed.tradeReal.weights
    Click $b.x $b.y; Wait 500
    $r.weights = State
    $l = $r.weights.tradeReal.list
    Click ($l.x - 100) ($l.y - 150); Wait 400
    $r.row = State
    $s = $r.row.tradeReal.slider
    Click $s.x $s.y; Wait 400
    $r.slider = State
    $b = $r.slider.tradeReal.cancel
    Click $b.x $b.y; Wait 500
    $r.back = State
    # Done
    $b = $r.back.tradeReal.close
    Click $b.x $b.y; Wait 500
    $r.closed = State
    $r
}
