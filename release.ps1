<#
ClipPro release manager. Pushing code never makes a release; only this does.

    .\release.ps1 status                    current version, releases and drafts
    .\release.ps1 new patch "Fixed X"       build the next version as a DRAFT (patch/minor/major or 1.2.0)
    .\release.ps1 publish 1.2.0             publish the draft: users get the update, the website shows it
    .\release.ps1 unpublish 1.2.0           turn a published release back into a draft
    .\release.ps1 delete 1.2.0              delete a draft
    .\release.ps1 watch                     follow the build that is running
    .\release.ps1 notes patch               which version that is, and whether its notes file exists

Release notes: write release-notes\<version>.md (e.g. release-notes\1.4.0.md). "new" uses it for
the release and adds it to the in-app What's new; without one, pass a short note in quotes.

Flow: new -> (GitHub builds the installer, about 30-40 min) -> download the draft's installer and
test it -> publish. The app offers only published releases newer than itself, and okata.space
always shows the latest published one.
#>
param(
    [Parameter(Position = 0)][ValidateSet("status", "new", "publish", "unpublish", "delete", "watch", "notes")]
    [string]$Command = "status",
    [Parameter(Position = 1)][string]$Version = "",
    [Parameter(Position = 2)][string]$Notes = ""
)

# Not "Stop": Windows PowerShell turns any stderr from gh or git (progress, "release not found")
# into a terminating error. Native calls are checked through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"
$Repo = "OkataMiracleDev/ClipPro"
$Conf = Join-Path $PSScriptRoot "app\src-tauri\tauri.conf.json"
$WhatsNew = Join-Path $PSScriptRoot "docs\manual\whats-new.md"

function Fail($msg) { Write-Host $msg -ForegroundColor Red; exit 1 }
function Current { (Get-Content $Conf -Raw | ConvertFrom-Json).version }

function Next($kind) {
    $p = (Current).Split(".") | ForEach-Object { [int]$_ }
    switch ($kind) {
        "major" { "$($p[0] + 1).0.0" }
        "minor" { "$($p[0]).$($p[1] + 1).0" }
        "patch" { "$($p[0]).$($p[1]).$($p[2] + 1)" }
        default { $kind -replace '^v', '' }
    }
}

function NotesFile($v) { Join-Path $PSScriptRoot ("release-notes\" + ($v -replace '^v', '') + ".md") }
function HasNotes($v) { $f = NotesFile $v; (Test-Path $f) -and ((Get-Content $f -Raw) -match '\S') }
function Tag($v) { if ($v.StartsWith("v")) { $v } else { "v$v" } }

# Adds "## <version>" with the notes to the in-app What's new, unless it is already there.
function AddToWhatsNew($v, $text) {
    $doc = Get-Content $WhatsNew -Raw
    if ($doc -match "(?m)^## $([regex]::Escape($v))\b") { return }
    # The notes file's own "# ClipPro x.y.z" title is replaced by the section heading, and its
    # "## New" style headings become "###" under it.
    $body = ($text -replace '(?m)^# .*\r?\n', '').Trim()
    $body = $body -replace '(?m)^## ', '### '
    $section = "## $v`n`n$body`n`n"
    $i = $doc.IndexOf("`n## ")
    $doc = if ($i -ge 0) { $doc.Substring(0, $i + 1) + $section + $doc.Substring($i + 1) } else { $doc.TrimEnd() + "`n`n" + $section }
    [IO.File]::WriteAllText($WhatsNew, $doc)
}

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { Fail "Install the GitHub CLI first: winget install GitHub.cli" }
Push-Location $PSScriptRoot
try {
    switch ($Command) {
        "status" {
            Write-Host "Version in the source: $(Current)"
            Write-Host "`nReleases in $Repo (users only see published ones):"
            gh release list --repo $Repo --limit 10
            Write-Host "`nRecent release builds:"
            gh run list --repo $Repo --workflow release.yml --limit 5
        }
        "notes" {
            # For release.bat: "<version>|<notes file>|yes" or "...|no".
            $v = Next $Version
            Write-Output ("{0}|{1}|{2}" -f $v, (NotesFile $v), $(if (HasNotes $v) { "yes" } else { "no" }))
        }
        "new" {
            if (-not $Version) { Fail "Usage: .\release.ps1 new patch|minor|major|1.2.0 `"What changed`"" }
            $v = Next $Version
            if ($v -notmatch '^\d+\.\d+\.\d+$') { Fail "Version must look like 1.2.0 (or use patch/minor/major)." }
            $notesFile = NotesFile $v
            if (HasNotes $v) {
                Write-Host "Release notes: $notesFile" -ForegroundColor Green
                $text = Get-Content $notesFile -Raw
            } elseif ($Notes) {
                Write-Host "No release-notes\$v.md: using the short note `"$Notes`"." -ForegroundColor Yellow
                $text = "# ClipPro $v`n`n$Notes`n"
            } else {
                Fail "No release notes for $v. Write release-notes\$v.md, or add a short note: .\release.ps1 new $Version `"What changed`""
            }
            if ([version]$v -le [version](Current) -and $Version -match '^\d') {
                Write-Host "Note: $v is not higher than $(Current); users would not be offered it." -ForegroundColor Yellow
            }
            gh release view (Tag $v) --repo $Repo 2>$null | Out-Null
            if ($LASTEXITCODE -eq 0) { Fail "$(Tag $v) already exists. Pick a higher version (.\release.ps1 status)." }
            $global:LASTEXITCODE = 0
            if (git status --porcelain) { Fail "You have uncommitted changes. Commit (or stash) them first, so the release is exactly what's on GitHub." }
            git fetch -q origin
            $branch = git rev-parse --abbrev-ref HEAD
            if (git rev-list "origin/$branch..HEAD") {
                Write-Host "Pushing your commits first..."
                git push -q origin $branch
                if ($LASTEXITCODE -ne 0) { Fail "Could not push $branch." }
            }

            # Record the version (and its notes in the in-app What's new), then ask GitHub to build it.
            python scripts\set_version.py $v | Out-Null
            if ($LASTEXITCODE -ne 0) { Fail "Could not set the version." }
            if (Get-Command cargo -ErrorAction SilentlyContinue) { cargo update --workspace --offline -q 2>$null }
            $global:LASTEXITCODE = 0
            AddToWhatsNew $v $text
            git add -A Cargo.toml Cargo.lock app/src-tauri/tauri.conf.json app/ui/package.json app/ui/package-lock.json docs/manual/whats-new.md
            if (Test-Path release-notes) { git add -A release-notes }
            git commit -q -m "Release $v"
            if ($LASTEXITCODE -ne 0) { Fail "Could not commit the version change." }
            git push -q origin $branch
            if ($LASTEXITCODE -ne 0) { Fail "Could not push the version change." }
            $tmp = Join-Path $env:TEMP "clippro-notes-$v.md"
            [IO.File]::WriteAllText($tmp, $text)
            gh workflow run release.yml --repo $Repo --ref $branch -f version=$v -F notes=@$tmp
            if ($LASTEXITCODE -ne 0) { Fail "Could not start the build." }
            Write-Host "Building ClipPro $v as a draft (about 30-40 minutes)." -ForegroundColor Green
            Write-Host "Follow it with:  .\release.ps1 watch"
            Write-Host "When it's done, test the installer from https://github.com/$Repo/releases"
            Write-Host "then:            .\release.ps1 publish $v"
        }
        "watch" {
            Start-Sleep 3
            $id = gh run list --repo $Repo --workflow release.yml --limit 1 --json databaseId --jq ".[0].databaseId"
            gh run watch $id --repo $Repo --exit-status
        }
        "publish" {
            if (-not $Version) { Fail "Usage: .\release.ps1 publish 1.2.0" }
            # release-notes\<version>.md, if it's there, goes on the release again (it may have been
            # edited since the build). The app's update message reads latest.json, made at build time.
            $notesFile = NotesFile $Version
            if (HasNotes $Version) {
                gh release edit (Tag $Version) --repo $Repo --notes-file $notesFile
                if ($LASTEXITCODE -ne 0) { Fail "Could not set the release notes from $notesFile." }
                Write-Host "Release notes: $notesFile"
            }
            gh release edit (Tag $Version) --repo $Repo --draft=false --latest
            if ($LASTEXITCODE -ne 0) { Fail "Publishing failed (does the draft exist? .\release.ps1 status)" }
            Write-Host "Published $(Tag $Version)." -ForegroundColor Green
            Write-Host "ClipPro offers it at its next start; okata.space shows it within about 10 minutes."
        }
        "unpublish" {
            if (-not $Version) { Fail "Usage: .\release.ps1 unpublish 1.2.0" }
            gh release edit (Tag $Version) --repo $Repo --draft=true
            if ($LASTEXITCODE -ne 0) { Fail "Unpublishing failed (.elease.ps1 status)." }
            Write-Host "$(Tag $Version) is a draft again; users who haven't updated won't be offered it."
        }
        "delete" {
            if (-not $Version) { Fail "Usage: .\release.ps1 delete 1.2.0" }
            $draft = gh release view (Tag $Version) --repo $Repo --json isDraft --jq ".isDraft"
            if ($draft -ne "true") { Fail "$(Tag $Version) is published. Unpublish it first if you really want to delete it." }
            gh release delete (Tag $Version) --repo $Repo --yes --cleanup-tag
            if ($LASTEXITCODE -ne 0) { Fail "Deleting failed (.elease.ps1 status)." }
            Write-Host "Deleted draft $(Tag $Version)."
        }
    }
} finally { Pop-Location }
