# Builds dist/pob-rust/: the exe of the `dist` profile without a console, the converted data and the files of upstream the application reads.
# Upstream and data/ are only read. Nothing is published or downloaded.
#   pwsh tools/package.ps1 [-SkipBuild]
#
# What of upstream's src/ goes in (a journal of the files read on all the tabs, plus every reader in crates/):
#   Assets/ as it is. TreeData/ with ALL the versions (a build of an old version opens with that version's tree), one file per content: the other paths are in the table of the copies.
#   Every Lua file the application reads becomes a table of src/src.cache (crates/pob-ui/src/src_cache.rs); no .lua is in the package, and nothing there parses Lua text.
#   Not read by anything: Classes/, Modules/, Export/, the rest of Data/ (TimelessJewelData/ is the converter's input: the app reads data/timeless-luts, data/node-id-list.json and
#   data-patches/legion-passives.json instead).
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$up = Get-Content (Join-Path $repo 'tools/upstream.json') -Raw | ConvertFrom-Json
$pob = if ([IO.Path]::IsPathRooted($up.pobRoot)) { $up.pobRoot } else { Join-Path $repo $up.pobRoot }
$out = Join-Path $repo 'dist/pob-rust'
$exeName = 'Path of Building (Rust).exe'

foreach ($need in 'manifest.xml', 'changelog.txt', 'help.txt', 'LICENSE.md', 'src/GameVersions.lua', 'src/TreeData', 'src/Data', 'src/Assets', 'runtime/SimpleGraphic/Fonts') {
    if (-not (Test-Path (Join-Path $pob $need))) { throw "upstream has no $need" }
}

# the data must be what data.lock says (the application refuses it otherwise)
$lock = Get-Content (Join-Path $repo 'data.lock') -Raw | ConvertFrom-Json
$sha = (Get-FileHash (Join-Path $repo 'data/manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sha -ne $lock.manifestSha256) { throw "data/manifest.json has sha256 $sha, data.lock says $($lock.manifestSha256): run tools/sync-data.ps1" }

$manifest = [xml](Get-Content (Join-Path $pob 'manifest.xml') -Raw)
$version = $manifest.PoBVersion.Version.number
if ($version -ne $up.pobVersion) { throw "upstream's manifest.xml says $version, tools/upstream.json says $($up.pobVersion)" }

if (-not $SkipBuild) {
    # No path of this machine in the exe: the sources of the dependencies and of the workspace appear in panic locations
    $home_ = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $home_ '.cargo' }
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $home_ '.rustup' }
    $flags = @("--remap-path-prefix=$repo=src", "--remap-path-prefix=$cargoHome=cargo", "--remap-path-prefix=$rustupHome=rustup")
    $saved = $env:CARGO_ENCODED_RUSTFLAGS
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1f
    try {
        Push-Location $repo
        cargo build --profile dist -p pob-app --features package
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    } finally {
        Pop-Location
        $env:CARGO_ENCODED_RUSTFLAGS = $saved
    }
}
$exe = Join-Path $repo 'target/dist/pob-app.exe'
if (-not (Test-Path $exe)) { throw "no $exe" }

# the check of the exe: no path of this machine, no name of the repo folder
$bytes = [IO.File]::ReadAllBytes($exe)
$ascii = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
$utf16 = [Text.Encoding]::Unicode.GetString($bytes)
$userProfile = if ($env:USERPROFILE) { Split-Path $env:USERPROFILE -Leaf } else { $null }
$bad = @('Users\', 'Users/', (Split-Path $repo -Leaf), $userProfile) | Where-Object { $_ }
$found = @($bad | Where-Object { $ascii.Contains($_) -or $utf16.Contains($_) })
if ($found) { throw "the exe contains: $($found -join ', ')" }

if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null
function Copy-Tree([string]$from, [string]$to, [string[]]$excludeDirs = @()) {
    $a = @($from, $to, '/E', '/NFL', '/NDL', '/NJH', '/NJS', '/NP')
    if ($excludeDirs) { $a += '/XD'; $a += $excludeDirs }
    robocopy @a | Out-Null
    # robocopy: 0-7 are success
    if ($LASTEXITCODE -ge 8) { throw "robocopy $from -> $to failed: $LASTEXITCODE" }
    $global:LASTEXITCODE = 0
}

Copy-Item $exe (Join-Path $out $exeName)
Copy-Tree (Join-Path $repo 'data') (Join-Path $out 'data')
Copy-Tree (Join-Path $repo 'data-patches') (Join-Path $out 'data-patches')
Copy-Item (Join-Path $repo 'data.lock') $out
# the binary cache of data/ (without it the application reads the JSON, slower)
& (Join-Path $PSScriptRoot 'gen-data-cache.ps1')
if ($LASTEXITCODE -ne 0) { throw 'gen-data-cache failed' }
Copy-Item (Join-Path $repo 'data.cache') $out
foreach ($f in 'manifest.xml', 'changelog.txt', 'help.txt') { Copy-Item (Join-Path $pob $f) $out }
Copy-Tree (Join-Path $pob 'runtime/SimpleGraphic/Fonts') (Join-Path $out 'runtime/SimpleGraphic/Fonts')
New-Item -ItemType Directory -Force (Join-Path $out 'src') | Out-Null
Copy-Tree (Join-Path $pob 'src/Assets') (Join-Path $out 'src/Assets')
# TreeData/ and src.cache. It stops if a file through the table of the copies or a table differs from upstream (byte for byte, bit for bit) or a .lua is left.
Push-Location $repo
try {
    cargo run --release -p pob-ui --example gen_src_cache -- (Join-Path $pob 'src') (Join-Path $out 'src')
    if ($LASTEXITCODE -ne 0) { throw 'gen_src_cache failed' }
} finally {
    Pop-Location
}
$lua = @(Get-ChildItem $out -Recurse -File -Filter '*.lua')
if ($lua) { throw "the package has $($lua.Count) Lua files: $($lua[0].FullName)" }

Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $out 'LICENSE')
Copy-Item (Join-Path $pob 'LICENSE.md') (Join-Path $out 'LICENSE-PathOfBuilding.md')
$head = (git -C $repo rev-parse --short HEAD)
@"
Path of Building (Rust)

A Rust port of Path of Building Community, built from commit $head of the port.

The program is under the MIT license of the port: LICENSE.
Next to it are unchanged files of Path of Building Community $version (commit $($up.pobSha),
https://github.com/PathOfBuildingCommunity/PathOfBuilding), under its license: LICENSE-PathOfBuilding.md:
  manifest.xml, changelog.txt, help.txt, runtime/SimpleGraphic/Fonts, src/Assets, src/TreeData (one file for each content).
data/, data-patches/ and src/src.cache are converted from the same files (pob-sha $($lock.pobSha)).
Game data and art: (c) Grinding Gear Games. This port is not affiliated with Grinding Gear Games or Path of Building Community.

User data goes to Documents\Path of Building (Rust). It updates by a new build: Ctrl+U only shows a status.
"@ | Set-Content -Encoding utf8 (Join-Path $out 'NOTICE.txt')

$mb = { param($p) [math]::Round(((Get-ChildItem $p -Recurse -File | Measure-Object Length -Sum).Sum) / 1MB, 1) }
"exe     {0,8:N1} MB" -f ((Get-Item (Join-Path $out $exeName)).Length / 1MB)
foreach ($d in 'data', 'data-patches', 'runtime', 'src/TreeData', 'src/Assets') { "{0,-13} {1,8:N1} MB" -f $d, (& $mb (Join-Path $out $d)) }
"src.cache {0,6:N1} MB" -f ((Get-Item (Join-Path $out 'src/src.cache')).Length / 1MB)
"total   {0,8:N1} MB, {1} files" -f (& $mb $out), (Get-ChildItem $out -Recurse -File).Count
"version $version"
"out     $out"
