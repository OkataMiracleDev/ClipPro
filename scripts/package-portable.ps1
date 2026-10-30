# Portable ZIP of a release build (PLAN P14.1): the app, the CLI, FFmpeg, DirectML and the
# licence notices in one folder that runs without installing. Build the installer first
# (`tauri build` in app/, which also builds the CLI), then:
#
#   powershell -File scripts\package-portable.ps1
#
# Writes target\release\bundle\portable\ClipPro_<version>_x64-portable.zip.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$conf = Get-Content (Join-Path $root 'app\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$rel = Join-Path $root 'target\release'
$out = Join-Path $rel 'bundle\portable'
$stage = Join-Path $out "ClipPro-$($conf.version)"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses') | Out-Null

Copy-Item (Join-Path $rel 'ClipPro.exe') $stage
# Every bundle resource, at the same relative place as in the installer.
foreach ($p in $conf.bundle.resources.PSObject.Properties) {
    $src = Join-Path (Join-Path $root 'app\src-tauri') $p.Name
    $dst = Join-Path $stage $p.Value
    if (-not (Test-Path $src)) { throw "missing $src (build the installer first)" }
    New-Item -ItemType Directory -Force (Split-Path $dst) | Out-Null
    Copy-Item $src $dst
}
$zip = Join-Path $out "ClipPro_$($conf.version)_x64-portable.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -Recurse -Force $stage
Write-Host "Wrote $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
