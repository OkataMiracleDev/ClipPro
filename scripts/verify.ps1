# ClipPro verification (PLAN P0.2/P0.3): everything a change must pass before it is committed
# or released. Runs through Git Bash so the portable toolchain from scripts/env.sh applies.
#
#   powershell -File scripts\verify.ps1 [-Quick]
#
# -Quick skips the release test run (the slowest step) and the licence inventory.

param([switch]$Quick)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$bash = (Get-Command bash -ErrorAction SilentlyContinue).Source
if (-not $bash) { $bash = "$env:ProgramFiles\Git\bin\bash.exe" }
if (-not (Test-Path $bash)) { throw 'Git Bash not found (needed for scripts/env.sh)' }
$script:results = @()

function Run([string]$name, [string]$cmd) {
    Write-Host "== $name" -ForegroundColor Cyan
    $t = [Diagnostics.Stopwatch]::StartNew()
    # Native stderr (npm notices, cargo progress) must not stop the script in PowerShell 5.1.
    $ErrorActionPreference = 'Continue'
    & $bash -lc "cd '$($root -replace '\\','/')' && source scripts/env.sh >/dev/null 2>&1 && $cmd" 2>&1 | ForEach-Object { "$_" }
    $ok = $LASTEXITCODE -eq 0
    $script:results += [pscustomobject]@{ Step = $name; Result = $(if ($ok) { 'pass' } else { 'FAIL' }); Seconds = [math]::Round($t.Elapsed.TotalSeconds) }
    if (-not $ok) { Write-Host "   $name failed" -ForegroundColor Red }
}

& (Join-Path $PSScriptRoot 'doctor.ps1') *> $null
if ($LASTEXITCODE -ne 0) { & (Join-Path $PSScriptRoot 'doctor.ps1'); throw 'doctor found missing tools' }

Run 'clippy (deny-level lints)' 'cargo clippy --workspace --release --all-targets --quiet'
Run 'UI type-check' 'cd app/ui && npx tsc --noEmit -p .'
Run 'UI tests' 'npm --prefix app/ui test --silent'
Run 'bindings drift' 'timeout 900 bash scripts/check-bindings.sh < /dev/null'
if (-not $Quick) {
    Run 'Rust tests (release)' 'cargo test --workspace --release --quiet'
    Run 'licences' 'python scripts/licenses.py'
}
# Tests clean up after themselves; anything left is a bug.
Run 'no test residue' '! ls target | grep -q ^tmp-'

$script:results | Format-Table -AutoSize
if ($script:results.Result -contains 'FAIL') { exit 1 }
Write-Host 'verify: all steps passed' -ForegroundColor Green
exit 0
