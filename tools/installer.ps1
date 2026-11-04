# Builds dist/pob-rust-<version>-windows-x64-setup.exe from dist/pob-rust/ (tools/package.ps1 first). Needs Inno Setup 6 on this machine, the users of the installer do not.
#   pwsh tools/installer.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not (Test-Path (Join-Path $repo 'dist/pob-rust/Path of Building (Rust).exe'))) { throw 'no dist/pob-rust: run tools/package.ps1' }

$iscc = @(
    (Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe')
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'no Inno Setup 6: winget install --id JRSoftware.InnoSetup --scope user' }

$cargo = Get-Content (Join-Path $repo 'Cargo.toml') -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'no version in Cargo.toml' }
$version = $Matches[1]

# the icon of the application as an .ico of PNG frames
$icon = Join-Path $repo 'dist/installer-icon.ico'
$src = [Drawing.Image]::FromFile((Join-Path $repo 'crates/pob-app/assets/icon.png'))
$frames = foreach ($size in 256, 64, 48, 32, 16) {
    $bmp = [Drawing.Bitmap]::new($size, $size)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.DrawImage($src, 0, 0, $size, $size)
    $g.Dispose()
    $ms = [IO.MemoryStream]::new()
    $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , @($size, $ms.ToArray())
}
$src.Dispose()
$out = [IO.MemoryStream]::new()
$w = [IO.BinaryWriter]::new($out)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
    $s = if ($f[0] -ge 256) { 0 } else { $f[0] }
    $w.Write([byte]$s); $w.Write([byte]$s); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$f[1].Length); $w.Write([uint32]$offset)
    $offset += $f[1].Length
}
foreach ($f in $frames) { $w.Write([byte[]]$f[1]) }
$w.Flush()
[IO.File]::WriteAllBytes($icon, $out.ToArray())

& $iscc /Q "/DMyVersion=$version" "/DRepoRoot=$repo" "/DIconFile=$icon" (Join-Path $repo 'tools/installer.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC failed: $LASTEXITCODE" }
Remove-Item $icon
$setup = Get-Item (Join-Path $repo "dist/pob-rust-$version-windows-x64-setup.exe")
"{0}  {1:N0} MB" -f $setup.FullName, ($setup.Length / 1MB)
"sha256 {0}" -f (Get-FileHash $setup.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
