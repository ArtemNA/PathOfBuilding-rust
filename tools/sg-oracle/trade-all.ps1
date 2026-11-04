# U8d: the whole oracle of tests/trade.rs, scene groups one after another (a long -ScriptArgs fails, MINEFIELD R61). Stops on the first failed group.
#   pwsh tools/sg-oracle/trade-all.ps1 [-Only 'a,b;c,d']
param([string]$Only = '')
$here = $PSScriptRoot
$groups = if ($Only) { $Only -split ';' } else {
    @('open,weights,new-build,open-errors,open-bad-leagues',
        'query-ring,query-body,query-amulet,query-weapon',
        'query-standard,query-selectors,query-cancel,query-flask',
        'query-megalomaniac,query-watchers,query-pearl,query-socket',
        'search,limits,search-errors,search-adjust',
        'price-url,buy-rare,buy-unique,buy-compare',
        'scroll,hover,row-megalomaniac,row-megalomaniac2,row-watchers,row-pearl',
        'row-socket,row-amulet-copy,row-helmet-remove,login,login-timeout,expired')
}
$err = Join-Path $here 'fixtures\trade\startup-error.json'
foreach ($g in $groups) {
    Remove-Item $err -ErrorAction SilentlyContinue
    & pwsh (Join-Path $here 'run.ps1') -Script trade-runner.lua -ScriptArgs $g -Timeout 200 2>&1 | Select-Object -Last 1
    if (Test-Path $err) { "FAILED group: $g"; Get-Content $err; exit 1 }
}
'all groups done'
