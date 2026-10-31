# One-time setup for releases (see RELEASING.md): uploads the update-signing key to the GitHub
# repository's Actions secrets, where the release workflow signs each installer with it.
#
#   powershell -File scripts\setup-release-secrets.ps1 [-KeyDir "D:\ClipPro Data\keys"]
#
# Needs the GitHub CLI, signed in as an owner of the repository (gh auth status).

param([string]$KeyDir = "D:\ClipPro Data\keys")

$ErrorActionPreference = "Stop"
$Repo = "OkataMiracleDev/ClipPro"
$key = Join-Path $KeyDir "clippro-updater.key"
$pw = Join-Path $KeyDir "clippro-updater.password.txt"
if (-not (Test-Path $key)) { throw "No signing key at $key." }
if (-not (Test-Path $pw)) { throw "No key password at $pw." }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "Install the GitHub CLI first: winget install GitHub.cli" }

# Redirected from the files, so the key never appears on a command line or in the shell history.
# cmd's < passes the bytes as they are; a PowerShell pipe would add a newline to the password.
cmd /c "gh secret set TAURI_SIGNING_PRIVATE_KEY --repo $Repo < `"$key`""
if ($LASTEXITCODE -ne 0) { throw "Setting TAURI_SIGNING_PRIVATE_KEY failed." }
cmd /c "gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo $Repo < `"$pw`""
if ($LASTEXITCODE -ne 0) { throw "Setting TAURI_SIGNING_PRIVATE_KEY_PASSWORD failed." }

gh secret list --repo $Repo
Write-Host "Done. Releases can now be signed. Keep $KeyDir backed up: without that key, installed copies can't be updated." -ForegroundColor Green
