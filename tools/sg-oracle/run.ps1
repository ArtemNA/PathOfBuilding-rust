# Runs one oracle Lua script inside the sandbox copy of the original PoB exe, with a file audit before/after.
#   pwsh tools/sg-oracle/run.ps1 -Script text-corpus.lua [-Timeout 120] [-Inject inject.ps1] [-Deep]
# - lua/*.lua and scenes/*.json are copied to runtime/sg/ and the exe runs runtime/sg/<Script> with cwd = runtime/.
# - Watched outside dirs: path/size/mtime (+sha256 for user data and config files) before/after; plus a sweep for any file
#   written since launch under Documents, %APPDATA% (and %LOCALAPPDATA% with -Deep, ~50 s). Any outside change -> exit 2 (STOP).
# - runtime/sg/out/** is copied to fixtures/ only when the audit is clean. Screenshots are moved by the Lua manifest (see scene-runner).
param(
    [Parameter(Mandatory)][string]$Script,
    [int]$Timeout = 120,
    [string]$Inject,
    [switch]$Deep,
    [string]$ScriptArgs = ''
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$rt = Join-Path $here 'runtime'
$exe = Join-Path $rt 'Path{space}of{space}Building.exe'
if (-not (Test-Path $exe)) { throw 'sandbox missing: run setup.ps1' }
# The upstream checkout (tools/upstream.json via upstream.ps1): the Lua runners read its src/ in place (read-only) and the
# sandbox must be a copy of ITS runtime (else: setup.ps1 -Force).
. (Join-Path $here 'upstream.ps1')
$up = Get-SgUpstream
$upWin = $up.Root -replace '/', '\'
if ((Get-FileHash "$rt\SimpleGraphic.dll").Hash -ne (Get-FileHash "$upWin\runtime\SimpleGraphic.dll").Hash) {
    throw "sandbox SimpleGraphic.dll differs from $($up.Root)/runtime: run setup.ps1 -Force"
}
$docs = [Environment]::GetFolderPath('MyDocuments')
$watch = @(
    @{ p = "$docs\Path of Building"; hash = $true },
    @{ p = "$docs\Path of Building (PoE2)"; hash = $true },
    @{ p = "$env:APPDATA\Path of Building Community"; hash = $false },
    @{ p = "$env:APPDATA\Path of Building Community (PoE2)"; hash = $false },
    @{ p = "$upWin\runtime"; hash = $false },
    @{ p = "$upWin\src"; hash = $false },
    @{ p = 'C:\pob-work\project_15\PathOfBuilding\runtime'; hash = $false },
    @{ p = 'C:\pob-work\project_15\PathOfBuilding\src'; hash = $false }
)
function Snap {
    $m = @{}
    foreach ($w in $watch) {
        if (-not (Test-Path -LiteralPath $w.p)) { $m[$w.p] = 'absent'; continue }
        Get-ChildItem -LiteralPath $w.p -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object {
            $v = "$($_.Length)|$($_.LastWriteTimeUtc.Ticks)"
            if (-not $_.PSIsContainer -and ($w.hash -or $_.Extension -in '.cfg', '.ini', '.xml')) {
                $v += '|' + (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
            }
            $m[$_.FullName] = $v
        }
    }
    $m
}
function SnapSandbox { $m = @{}; Get-ChildItem $rt -Recurse -File -Force | ForEach-Object { $m[$_.FullName] = "$($_.Length)|$($_.LastWriteTimeUtc.Ticks)" }; $m }
function Compare-Snap($a, $b) {
    $out = @()
    foreach ($k in $b.Keys) { if (-not $a.ContainsKey($k)) { $out += "NEW  $k" } elseif ($a[$k] -ne $b[$k]) { $out += "CHG  $k" } }
    foreach ($k in $a.Keys) { if (-not $b.ContainsKey($k)) { $out += "DEL  $k" } }
    $out | Sort-Object
}

if (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) { throw 'a sandbox exe is already running' }
# Stage scripts + scenes into the sandbox; clear previous outputs.
$sg = Join-Path $rt 'sg'
if (Test-Path $sg) { Remove-Item -Recurse -Force $sg }
New-Item -ItemType Directory "$sg\out" | Out-Null
Copy-Item "$here\lua\*.lua" $sg
# The upstream root for the runners (`dofile(base .. "/upstream.lua")`) and scene image paths (`{POB}/...`).
$oracleRoot = (Get-Content (Join-Path $here '..\upstream.json') -Raw | ConvertFrom-Json).oracleRoot
"return { root = `"$($up.Root)/`", sha = `"$($up.Sha)`", oracle = `"$oracleRoot/`" }" | Set-Content "$sg\upstream.lua" -Encoding ascii
if (Test-Path "$here\scenes") { Copy-Item "$here\scenes" "$sg\scenes" -Recurse }
$shots = Join-Path $rt 'SimpleGraphic\Screenshots'
if (Test-Path $shots) { Remove-Item -Recurse -Force $shots }

$before = Snap
$sbBefore = SnapSandbox
$t0 = Get-Date
$p = Start-Process -FilePath $exe -ArgumentList "`"$sg\$Script`" $ScriptArgs" -WorkingDirectory $rt -PassThru
"launched pid $($p.Id) at $($t0.ToString('HH:mm:ss.fff'))"
if ($Inject) { & (Join-Path $here $Inject) -Process $p -Out $sg }
if (-not $p.WaitForExit($Timeout * 1000)) {
    Write-Warning "timeout ${Timeout}s: killing own pid $($p.Id)"
    Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null
}
$t1 = Get-Date
"exit code $($p.ExitCode), $([int]($t1 - $t0).TotalMilliseconds) ms"
if (Get-Process -Id $p.Id -ErrorAction SilentlyContinue) { Write-Warning "pid $($p.Id) still alive" }

$after = Snap
$outside = @(Compare-Snap $before $after)
# Documents\AI (this repo and other agents' work) is skipped: the sandbox lives there and parallel sessions write there.
$roots = @(Get-ChildItem $docs -Force | Where-Object { $_.Name -ne 'AI' } | ForEach-Object FullName) + @($env:APPDATA) +
    $(if ($Deep) { @($env:LOCALAPPDATA) } else { @() })
$recent = foreach ($r in $roots) {
    Get-ChildItem -LiteralPath $r -Recurse -File -Force -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -ge $t0 } | ForEach-Object { "RECENT $($_.FullName)" }
}
$sbChanges = @(Compare-Snap $sbBefore (SnapSandbox) | Where-Object { $_ -notlike "*\sg\*" } | ForEach-Object { $_.Replace($rt, '<rt>') })
$suspect = @($recent | Where-Object { $_ -match 'Path of Building|SimpleGraphic|imgui\.ini|Screenshots' })
"--- sandbox changes"
$sbChanges
"--- outside watched changes: $($outside.Count)"
$outside
"--- files written since launch outside sandbox (incl. other apps' noise): $(@($recent).Count), PoB-like names: $($suspect.Count)"
$recent
# Audit record (no foreign paths: other apps' noise is only counted).
New-Item -ItemType Directory -Force "$sg\out\audit" | Out-Null
[ordered]@{ script = $Script; args = $ScriptArgs; exit = $p.ExitCode; ms = [int]($t1 - $t0).TotalMilliseconds
    upstream_sha = $up.Sha; simplegraphic_sha256 = (Get-FileHash "$rt\SimpleGraphic.dll").Hash
    sandbox_changes = $sbChanges; outside_watched_changes = $outside; recent_writes_elsewhere = @($recent).Count; pob_like_recent = $suspect
} | ConvertTo-Json | Set-Content ("$sg\out\audit\" + ($Script -replace '\.lua$', '') + $(if ($ScriptArgs) { "-$($ScriptArgs -replace '[^\w-]', '_')" }) + '.json')
if ($outside.Count -or $suspect.Count) { Write-Warning 'STOP: original exe changed files outside the sandbox'; exit 2 }
# Scenes: crop full-screen shots to the scene's top-left size*dpi/100 physical px (fixtures independent of monitor size).
$man = "$sg\out\scenes\manifest.json"
if (Test-Path $man) {
    Add-Type -AssemblyName System.Drawing
    foreach ($m in (Get-Content $man -Raw | ConvertFrom-Json)) {
        $png = "$sg\out\scenes\$($m.name).png"
        if (-not (Test-Path $png)) { Write-Warning "no shot for $($m.name)"; continue }
        New-Item -ItemType Directory -Force "$sg\full" | Out-Null; Copy-Item $png "$sg\full\"  # uncropped copy stays in the sandbox
        $k = $m.dpi / 100
        $src = [System.Drawing.Bitmap]::new($png)
        $rect = [System.Drawing.Rectangle]::new(0, 0, [math]::Ceiling($m.size[0] * $k), [math]::Ceiling($m.size[1] * $k))
        $crop = $src.Clone($rect, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb); $src.Dispose()
        $crop.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $crop.Dispose()
        "scene $($m.name): $($rect.Width)x$($rect.Height) from $($m.screen -join 'x') errors=$(@($m.errors).Count)"
    }
}
if (Test-Path "$sg\out\*") { New-Item -ItemType Directory -Force (Join-Path $here 'fixtures') | Out-Null; Copy-Item "$sg\out\*" (Join-Path $here 'fixtures') -Recurse -Force; 'outputs copied to fixtures/' }
