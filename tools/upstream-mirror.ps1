#requires -Version 7
# Lua-5.4-loadable mirror of the upstream PoB checkout for the WASM tools (pob-ts converter, pob-engine-wasm oracle).
#
# Why: desktop/docker LuaJIT of PoB is patched with compound assignment (`x += 1`, `s ..= "a"`). The pob-ts converter
# and the pob-engine-wasm oracle (both READ-ONLY for us) desugar only `+ - * / %` (pob-ts tools/convert/sandbox.ts
# COMPOUND_ASSIGN, pob-engine-wasm src/engine.mjs patchLua); upstream 3dbca4463 added `..=` (Classes/CompareBuySimilar.lua
# :199-200) and the boot dies with "syntax error near '..'". This script makes a `git clone --shared` of the upstream
# (objects via alternates: nothing is written into the upstream checkout) at the sha of tools/upstream.json and rewrites
# every `<lvalue> ..= <expr>` line into `<lvalue> = <lvalue> .. <expr>` -- the same textual desugaring the tools apply to
# `+=`. LuaJIT probes and Rust keep reading the REAL upstream (`pobRoot`); only WASM consumers get the mirror.
#
# Usage: tools/upstream-mirror.ps1 [-Force]      (idempotent; -Force re-clones)
param([switch]$Force)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$cfg = Get-Content -LiteralPath (Join-Path $root 'tools\upstream.json') -Raw | ConvertFrom-Json
function Resolve-Repo([string]$p) { if ([IO.Path]::IsPathRooted($p)) { $p } else { Join-Path $root $p } }
$pobRoot = if ($env:POB_ROOT) { $env:POB_ROOT } else { Resolve-Repo $cfg.pobRoot }
$mirror = if ($env:POB_WASM_ROOT) { $env:POB_WASM_ROOT } else { Resolve-Repo $cfg.wasmMirror }
$sha = $cfg.pobSha

$head = (& git -C $pobRoot rev-parse HEAD).Trim()
if ($head -ne $sha) { throw "upstream $pobRoot is at $head, tools/upstream.json pobSha is $sha" }

if ($Force -and (Test-Path -LiteralPath $mirror)) { Remove-Item -LiteralPath $mirror -Recurse -Force }
if (-not (Test-Path -LiteralPath (Join-Path $mirror '.git'))) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $mirror) | Out-Null
    & git clone --quiet --shared --no-checkout $pobRoot $mirror
    if ($LASTEXITCODE -ne 0) { throw 'git clone --shared failed' }
}
& git -C $mirror -c advice.detachedHead=false checkout --quiet --force $sha
if ($LASTEXITCODE -ne 0) { throw "checkout $sha failed (fetch it into $pobRoot first)" }
& git -C $mirror reset --quiet --hard $sha

# `..=` desugaring; [^\S\n] keeps the match on one line. Only whole-statement lvalues (name / a.b.c), like COMPOUND_ASSIGN.
$rx = [regex]'(?m)^([ \t]*)([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)[ \t]*\.\.=[ \t]*'
$patched = @()
foreach ($f in Get-ChildItem -LiteralPath (Join-Path $mirror 'src') -Recurse -File -Filter '*.lua') {
    $bytes = [IO.File]::ReadAllBytes($f.FullName)
    $text = [Text.Encoding]::Latin1.GetString($bytes)   # byte-preserving
    if (-not $text.Contains('..=')) { continue }
    $new = $rx.Replace($text, '$1$2 = $2 .. ')
    if ($new -ne $text) {
        [IO.File]::WriteAllBytes($f.FullName, [Text.Encoding]::Latin1.GetBytes($new))
        $patched += $f.FullName.Substring($mirror.Length + 1)
    }
}
# A `..=` the regex could not desugar (e.g. `t[k] ..= x`) would still break the WASM boot: fail loudly.
$left = Get-ChildItem -LiteralPath (Join-Path $mirror 'src') -Recurse -File -Filter '*.lua' |
    Select-String -Pattern '^[^-"'']*[^.]\.\.=' -List | ForEach-Object { "$($_.Path):$($_.LineNumber)" }
if ($left) { throw "un-desugared '..=' left: $($left -join ', ')" }
Write-Output "mirror: $mirror @ $($sha.Substring(0, 9)); desugared '..=' in $($patched.Count) file(s): $($patched -join ', ')"
