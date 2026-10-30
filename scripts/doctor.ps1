# ClipPro development doctor (PLAN P0.1): checks the toolchain and reports what's missing.
# Everything lives in third_party/ (portable, no admin) except Rust (per-user rustup), Node and
# Python. Exit code 0 when every required item is present; optional items only warn.
#
#   powershell -File scripts\doctor.ps1
#
# Install what's missing with scripts\bootstrap.ps1.

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
$tp = Join-Path $root 'third_party'
$script:failed = 0

function Report([string]$name, [bool]$ok, [string]$detail, [switch]$Optional) {
    if ($ok) {
        Write-Host ("  [ok]   {0,-34} {1}" -f $name, $detail) -ForegroundColor Green
    } elseif ($Optional) {
        Write-Host ("  [warn] {0,-34} {1}" -f $name, $detail) -ForegroundColor Yellow
    } else {
        Write-Host ("  [MISS] {0,-34} {1}" -f $name, $detail) -ForegroundColor Red
        $script:failed++
    }
}

function Version-Of([string]$exe, [string[]]$argv) {
    try { (& $exe @argv 2>$null | Select-Object -First 1) } catch { $null }
}

Write-Host "ClipPro doctor - $root"

# Rust
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
$cargoV = if ($cargo) { Version-Of 'cargo' @('--version') } else { $null }
Report 'cargo (stable)' ($null -ne $cargoV) ($(if ($cargoV) { $cargoV } else { 'install rustup: https://rustup.rs' }))
$host_ok = $false
if (Get-Command rustc -ErrorAction SilentlyContinue) { $host_ok = ((rustc -vV) -match 'host: x86_64-pc-windows-msvc').Count -gt 0 }
Report 'rust target x86_64-pc-windows-msvc' $host_ok 'default host toolchain'

# MSVC + Windows SDK (portable install via third_party/portable-msvc.py)
$msvc = Get-ChildItem (Join-Path $tp 'msvc\VC\Tools\MSVC') -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
Report 'MSVC build tools' ($null -ne $msvc) ($(if ($msvc) { "third_party/msvc ($($msvc.Name))" } else { 'run bootstrap.ps1 (portable-msvc.py)' }))
$fxc = Get-ChildItem (Join-Path $tp 'msvc\Windows Kits\10\bin') -Recurse -Filter fxc.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
Report 'Windows SDK (fxc.exe, rc.exe)' ($null -ne $fxc) ($(if ($fxc) { $fxc.Directory.Parent.Name } else { 'part of the portable MSVC install' }))

# LLVM (libclang for bindgen)
$clang = Join-Path $tp 'llvm\bin\libclang.dll'
Report 'libclang (bindgen)' (Test-Path $clang) 'third_party/llvm/bin/libclang.dll'

# CMake + Ninja (whisper.cpp)
$cmake = Join-Path $tp 'cmake\bin\cmake.exe'
$cmakeV = if (Test-Path $cmake) { Version-Of $cmake @('--version') } else { $null }
Report 'CMake' ($null -ne $cmakeV) ($(if ($cmakeV) { $cmakeV } else { 'third_party/cmake' }))
Report 'Ninja' (Test-Path (Join-Path $tp 'tools\bin\ninja.exe')) 'third_party/tools/bin/ninja.exe'

# FFmpeg LGPL shared
$ffmpeg = Join-Path $tp 'ffmpeg'
$ffOk = (Test-Path (Join-Path $ffmpeg 'include\libavcodec\avcodec.h')) -and (Test-Path (Join-Path $ffmpeg 'bin\ffmpeg.exe'))
$lgpl = (Test-Path (Join-Path $ffmpeg 'LICENSE.txt')) -and ((Get-Content (Join-Path $ffmpeg 'LICENSE.txt') -TotalCount 2) -match 'LESSER').Count -gt 0
$ffV = if ($ffOk) { Version-Of (Join-Path $ffmpeg 'bin\ffmpeg.exe') @('-version') } else { $null }
Report 'FFmpeg (LGPL shared)' ($ffOk -and $lgpl) ($(if ($ffV) { ($ffV -split ' ')[2] + $(if ($lgpl) { ', LGPL' } else { ', NOT LGPL' }) } else { 'third_party/ffmpeg (BtbN win64-lgpl-shared)' }))

# Node + UI dependencies
$nodeV = Version-Of 'node' @('--version')
$nodeOk = $nodeV -and ([int]($nodeV.TrimStart('v').Split('.')[0]) -ge 20)
Report 'Node >= 20' ([bool]$nodeOk) ($(if ($nodeV) { $nodeV } else { 'https://nodejs.org' }))
$tauriCli = Join-Path $root 'app\ui\node_modules\.bin\tauri.cmd'
Report 'tauri-cli v2 (npm)' (Test-Path $tauriCli) 'app/ui/node_modules (npm ci in app/ui)'

# Python validators (dev only)
$pyV = Version-Of 'python' @('--version')
Report 'Python 3' ([bool]($pyV -match 'Python 3')) ($(if ($pyV) { $pyV } else { 'https://python.org' }))
Report 'pyaaf2 (AAF validator)' (Test-Path (Join-Path $tp 'pyaaf2\aaf2')) 'third_party/pyaaf2' -Optional
$otio = $false
if ($pyV) { python -c "import opentimelineio" 2>$null; $otio = ($LASTEXITCODE -eq 0) }
Report 'opentimelineio (OTIO validator)' $otio 'pip install opentimelineio (no wheel for Python 3.14 yet)' -Optional

# E2E drivers, corpus, AI models
Report 'tauri-driver + msedgedriver (E2E)' ((Test-Path (Join-Path $tp 'tools\bin\tauri-driver.exe')) -and (Test-Path (Join-Path $tp 'tools\msedgedriver.exe'))) 'third_party/tools' -Optional
Report 'test corpus' (Test-Path (Join-Path $root 'testdata\gen\tripod_static.mp4')) 'clippro-testgen --out testdata/gen --mini' -Optional
$models = [Environment]::GetEnvironmentVariable('CLIPPRO_MODELS', 'User')
if ($env:CLIPPRO_MODELS) { $models = $env:CLIPPRO_MODELS }
if (-not $models) { $models = Join-Path $root 'target\models' }
$n = (Get-ChildItem $models -File -ErrorAction SilentlyContinue | Measure-Object).Count
Report 'AI models for tests' ($n -ge 5) "$models ($n files; AI tests skip without them)" -Optional

if ($script:failed -gt 0) {
    Write-Host "$($script:failed) required item(s) missing. Run scripts\bootstrap.ps1." -ForegroundColor Red
    exit 1
}
Write-Host 'All required tools are present.' -ForegroundColor Green
exit 0
