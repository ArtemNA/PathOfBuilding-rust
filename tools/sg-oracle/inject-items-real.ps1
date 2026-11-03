# U6: the scenario of items-real-seq.ps1 in the original's window (PostMessage to the sandbox exe's own HWND only; SimpleGraphic's GetCursorPos asks the OS, so the runner reads the position from sg/cursor.txt).
#   pwsh tools/sg-oracle/run.ps1 -Script items-runner.lua -ScriptArgs real-input -Inject inject-items-real.ps1 -Timeout 40
param($Process, [string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -Namespace SgOracle -Name I -MemberDefinition @"
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
"@
. (Join-Path $PSScriptRoot 'items-real-seq.ps1')
for ($i = 0; $i -lt 250 -and -not (Test-Path "$Out\ready"); $i++) { Start-Sleep -Milliseconds 100 }
if (-not (Test-Path "$Out\ready")) { throw 'the original did not get ready' }
Start-Sleep -Milliseconds 1500
$Process.Refresh(); $h = $Process.MainWindowHandle
function P([uint32]$msg, [int64]$w, [int64]$l) { [void][SgOracle.I]::PostMessageW($h, $msg, [IntPtr]$w, [IntPtr]$l) }
function Cursor([int]$x, [int]$y) { Set-Content "$Out\cursor.txt" "$x $y" -Encoding ascii }
function Wait($ms = 250) { Start-Sleep -Milliseconds $ms }
function State { for ($i = 0; $i -lt 50; $i++) { try { return Get-Content "$Out\state.json" -Raw | ConvertFrom-Json } catch { Start-Sleep -Milliseconds 20 } } }
$r = Run-ItemsRealInput
New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot 'fixtures/items') | Out-Null
$r | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $PSScriptRoot 'fixtures/items/real-input-oracle.json')
Set-Content "$Out\stop" '1'
