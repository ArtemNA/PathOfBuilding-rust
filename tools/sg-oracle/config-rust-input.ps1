# U7: the real input path of the Rust window on the Config tab, public golden: PostMessage to pob-app's own HWND (only) -> winit -> OnKeyDown / OnKeyUp / OnChar. A sandbox user folder in the temp dir; nothing touches the real mouse or keyboard.
#   pwsh tools/sg-oracle/config-rust-input.ps1 [-Exe target/release/pob-app.exe]
param([string]$Exe = "target/release/pob-app.exe")
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name R -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
$up = Get-Content (Join-Path $PSScriptRoot '../upstream.json') -Raw | ConvertFrom-Json
$root = Join-Path ([IO.Path]::GetTempPath()) ("pob-rust-config-input-" + $PID)
$builds = Join-Path $root 'Path of Building (Rust)/Builds'
New-Item -ItemType Directory -Force $builds | Out-Null
$name = 'OccVortex'
Copy-Item "$($up.pobRoot)/spec/TestBuilds/3.13/$name.xml" (Join-Path $builds "$name.xml")
$stateFile = Join-Path $root 'state.json'
$p = Start-Process -FilePath $Exe -ArgumentList @('--user-path', "`"$root`"", '--open', "`"$name`"", '--state-file', "`"$stateFile`"", '--secs', '40') -PassThru
function State { for ($i = 0; $i -lt 50; $i++) { try { return Get-Content $stateFile -Raw | ConvertFrom-Json } catch { Start-Sleep -Milliseconds 20 } } throw 'no state' }
function Wait($ms = 250) { Start-Sleep -Milliseconds $ms }
. (Join-Path $PSScriptRoot 'config-real-seq.ps1')
try {
    for ($i = 0; $i -lt 300; $i++) { if ((Test-Path $stateFile) -and (State).configReal) { break }; Start-Sleep -Milliseconds 100 }
    if (-not (State).configReal) { throw 'the build did not open' }
    Wait 1500
    $p.Refresh(); $h = $p.MainWindowHandle
    function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.R]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
    function Cursor([int]$x, [int]$y) { }
    $r = Run-ConfigRealInput
    $r | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $PSScriptRoot 'fixtures/config/real-input-rust.json')
} finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
