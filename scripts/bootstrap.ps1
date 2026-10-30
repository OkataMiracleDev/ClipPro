# ClipPro development bootstrap (PLAN P0.1): installs whatever scripts\doctor.ps1 reports
# missing. Everything except Rust goes into third_party\ (portable, per-user, no admin), so a
# fresh clone needs only this script, Node, Python and an internet connection. Items already
# present are skipped.
#
#   powershell -File scripts\bootstrap.ps1 [-AcceptMsvcLicense] [-Only ffmpeg,cmake,...]
#
# -AcceptMsvcLicense accepts the Visual Studio Build Tools licence for the portable MSVC
# download (portable-msvc.py); without it that step asks interactively.

param(
    [switch]$AcceptMsvcLicense,
    [string[]]$Only = @(),
    [string]$ThirdParty = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Split-Path -Parent $PSScriptRoot
$tp = if ($ThirdParty) { $ThirdParty } else { Join-Path $root 'third_party' }
New-Item -ItemType Directory -Force $tp | Out-Null
$tmp = Join-Path $root 'target\tmp-bootstrap'
New-Item -ItemType Directory -Force $tmp | Out-Null

# Pinned versions.
$FFMPEG = 'n9.0'
$CMAKE = '4.4.5'
$NINJA = '1.13.2'
$LLVM = '23.1.3'
$PYAAF2 = '1.7.1'

function Want([string]$name) { $Only.Count -eq 0 -or $Only -contains $name }

function Fetch([string]$url, [string]$file) {
    $out = Join-Path $tmp $file
    if (-not (Test-Path $out)) {
        Write-Host "  downloading $url"
        & curl.exe -sS -L --fail --retry 5 --ssl-no-revoke -C - -o "$out.part" $url
        if ($LASTEXITCODE -ne 0) { throw "download failed: $url" }
        Move-Item "$out.part" $out
    }
    $out
}

function Step([string]$name, [scriptblock]$present, [scriptblock]$install) {
    if (-not (Want $name)) { return }
    if (& $present) { Write-Host "[skip] $name (present)"; return }
    Write-Host "[get]  $name"
    & $install
    if (-not (& $present)) { throw "$name still missing after install" }
    Write-Host "[ok]   $name"
}

# Rust (per-user rustup; stable MSVC host)
Step 'rust' { [bool](Get-Command cargo -ErrorAction SilentlyContinue) } {
    $exe = Fetch 'https://win.rustup.rs/x86_64' 'rustup-init.exe'
    & $exe -y --default-toolchain stable --default-host x86_64-pc-windows-msvc --profile minimal
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# MSVC + Windows SDK, portable (mmozeiko's portable-msvc.py)
Step 'msvc' { Test-Path (Join-Path $tp 'msvc\VC\Tools\MSVC') } {
    $py = Join-Path $tp 'portable-msvc.py'
    if (-not (Test-Path $py)) {
        Copy-Item (Fetch 'https://gist.githubusercontent.com/mmozeiko/7f3162ec2988e81e56d5c4e22cde9977/raw/portable-msvc.py' 'portable-msvc.py') $py
    }
    Push-Location $tp
    try {
        $a = @($py, '--target', 'x64')
        if ($AcceptMsvcLicense) { $a += '--accept-license' }
        & python @a
        if ($LASTEXITCODE -ne 0) { throw 'portable-msvc.py failed' }
    } finally { Pop-Location }
}

# FFmpeg LGPL shared (BtbN)
Step 'ffmpeg' { Test-Path (Join-Path $tp 'ffmpeg\include\libavcodec\avcodec.h') } {
    $v = $FFMPEG.TrimStart('n')
    $zip = Fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-$FFMPEG-latest-win64-lgpl-shared-$v.zip" "ffmpeg-$FFMPEG.zip"
    $x = Join-Path $tmp 'ffmpeg-x'
    Remove-Item -Recurse -Force $x -ErrorAction SilentlyContinue
    Expand-Archive $zip $x
    Move-Item (Get-ChildItem $x -Directory | Select-Object -First 1).FullName (Join-Path $tp 'ffmpeg')
}

# libclang and clang's headers only (for bindgen), from the LLVM release tarball
Step 'llvm' { Test-Path (Join-Path $tp 'llvm\bin\libclang.dll') } {
    $name = "clang+llvm-$LLVM-x86_64-pc-windows-msvc"
    $tar = Fetch "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM/$name.tar.xz" "$name.tar.xz"
    $x = Join-Path $tmp 'llvm-x'
    New-Item -ItemType Directory -Force $x | Out-Null
    & tar.exe -xf $tar -C $x "$name/bin/libclang.dll" "$name/lib/clang"
    if ($LASTEXITCODE -ne 0) { throw 'extracting libclang failed' }
    $dst = Join-Path $tp 'llvm'
    New-Item -ItemType Directory -Force (Join-Path $dst 'bin'), (Join-Path $dst 'lib') | Out-Null
    Move-Item (Join-Path $x "$name\bin\libclang.dll") (Join-Path $dst 'bin')
    Move-Item (Join-Path $x "$name\lib\clang") (Join-Path $dst 'lib')
}

# CMake and Ninja (whisper.cpp build)
Step 'cmake' { Test-Path (Join-Path $tp 'cmake\bin\cmake.exe') } {
    $zip = Fetch "https://github.com/Kitware/CMake/releases/download/v$CMAKE/cmake-$CMAKE-windows-x86_64.zip" "cmake-$CMAKE.zip"
    $x = Join-Path $tmp 'cmake-x'
    Remove-Item -Recurse -Force $x -ErrorAction SilentlyContinue
    Expand-Archive $zip $x
    Move-Item (Get-ChildItem $x -Directory | Select-Object -First 1).FullName (Join-Path $tp 'cmake')
}
Step 'ninja' { Test-Path (Join-Path $tp 'tools\bin\ninja.exe') } {
    $zip = Fetch "https://github.com/ninja-build/ninja/releases/download/v$NINJA/ninja-win.zip" "ninja-$NINJA.zip"
    New-Item -ItemType Directory -Force (Join-Path $tp 'tools\bin') | Out-Null
    Expand-Archive $zip (Join-Path $tp 'tools\bin') -Force
}

# UI dependencies (includes tauri-cli v2)
Step 'npm' { Test-Path (Join-Path $root 'app\ui\node_modules\.bin\tauri.cmd') } {
    & npm ci --prefix (Join-Path $root 'app\ui')
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
}

# pyaaf2 (AAF validator for tests), vendored so the system Python stays untouched
Step 'pyaaf2' { Test-Path (Join-Path $tp 'pyaaf2\aaf2') } {
    & python -m pip install --target (Join-Path $tp 'pyaaf2') "pyaaf2==$PYAAF2"
    if ($LASTEXITCODE -ne 0) { throw 'pip install pyaaf2 failed' }
}

# E2E drivers: tauri-driver, and msedgedriver matching the installed Edge
Step 'e2e' { (Test-Path (Join-Path $tp 'tools\bin\tauri-driver.exe')) -and (Test-Path (Join-Path $tp 'tools\msedgedriver.exe')) } {
    if (-not (Test-Path (Join-Path $tp 'tools\bin\tauri-driver.exe'))) {
        & cargo install tauri-driver --locked --root (Join-Path $tp 'tools')
    }
    $edge = (Get-Item "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe").VersionInfo.ProductVersion
    $zip = Fetch "https://msedgedriver.microsoft.com/$edge/edgedriver_win64.zip" "edgedriver-$edge.zip"
    $x = Join-Path $tmp 'edgedriver-x'
    Remove-Item -Recurse -Force $x -ErrorAction SilentlyContinue
    Expand-Archive $zip $x
    Copy-Item (Join-Path $x 'msedgedriver.exe') (Join-Path $tp 'tools\msedgedriver.exe') -Force
}

Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
Write-Host 'Bootstrap finished. Check with scripts\doctor.ps1.'
