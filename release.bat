@echo off
REM ClipPro release manager: double-click for a menu. Does the same as release.ps1
REM (which this calls), but asks for everything step by step.
setlocal EnableDelayedExpansion
cd /d "%~dp0"
title ClipPro releases
set "PS=powershell -NoProfile -ExecutionPolicy Bypass"
set "REPO=OkataMiracleDev/ClipPro"

where gh >nul 2>&1
if errorlevel 1 (
    echo The GitHub CLI is not installed. Install it with:  winget install GitHub.cli
    pause
    exit /b 1
)

:menu
cls
for /f "usebackq delims=" %%v in (`%PS% -Command "(Get-Content app\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version"`) do set "CURRENT=%%v"
echo.
echo   ==============================================
echo     ClipPro releases           current: v!CURRENT!
echo   ==============================================
echo.
echo     1  Make a new release (builds a draft)
echo     2  Watch the build that is running
echo     3  Publish a draft  (users and the website get it)
echo     4  Status: releases, drafts and builds
echo     5  Unpublish a release (back to draft)
echo     6  Delete a draft
echo     7  Open the releases page in the browser
echo     0  Exit
echo.
set "CHOICE="
set /p "CHOICE=  Choose a number: "
if "!CHOICE!"=="1" goto new
if "!CHOICE!"=="2" goto watch
if "!CHOICE!"=="3" goto publish
if "!CHOICE!"=="4" goto status
if "!CHOICE!"=="5" goto unpublish
if "!CHOICE!"=="6" goto delete
if "!CHOICE!"=="7" goto open
if "!CHOICE!"=="0" exit /b 0
goto menu

REM ---------------------------------------------------------------- new release
:new
cls
echo.
echo   NEW RELEASE   (current version: v!CURRENT!)
echo.
REM A release must match what's on GitHub, so offer to commit leftover changes first.
set "DIRTY="
for /f "delims=" %%l in ('git status --porcelain') do set "DIRTY=1"
if defined DIRTY (
    echo   You have changes that are not committed yet:
    echo.
    git status --short
    echo.
    set "YN="
    set /p "YN=  Commit them all now so they are part of this release? (y/n): "
    if /i not "!YN!"=="y" (
        echo.
        echo   Commit or undo those changes first, then try again.
        goto done
    )
    set "MSG="
    set /p "MSG=  Short description of those changes: "
    if "!MSG!"=="" set "MSG=Update"
    git add -A
    git commit -q -m "!MSG!"
    if errorlevel 1 ( echo   Committing failed. & goto done )
    echo   Committed.
    echo.
)
echo   What kind of release is this?
echo     1  Patch  - small fixes             (e.g. 1.0.0 to 1.0.1)
echo     2  Minor  - new features            (e.g. 1.0.0 to 1.1.0)
echo     3  Major  - big changes             (e.g. 1.0.0 to 2.0.0)
echo     4  Type an exact version            (e.g. 1.0.0 for the first release)
echo.
set "KIND="
set /p "KIND=  Choose 1-4: "
set "RC_VER="
if "!KIND!"=="1" set "RC_VER=patch"
if "!KIND!"=="2" set "RC_VER=minor"
if "!KIND!"=="3" set "RC_VER=major"
if "!KIND!"=="4" set /p "RC_VER=  Version (like 1.2.0): "
if "!RC_VER!"=="" ( echo   No version chosen. & goto done )

REM Which version that is, and whether release-notes\<version>.md exists.
:notes_check
set "RC_V=" & set "RC_FILE=" & set "RC_HAS="
for /f "usebackq tokens=1-3 delims=|" %%a in (`%PS% -Command "& '%~dp0release.ps1' notes $env:RC_VER"`) do (
    set "RC_V=%%a" & set "RC_FILE=%%b" & set "RC_HAS=%%c"
)
if "!RC_V!"=="" ( echo   Could not work out the version. & goto done )
set "RC_NOTES="
echo.
if "!RC_HAS!"=="yes" (
    echo   Release notes found: release-notes\!RC_V!.md
    echo   They go on the release, in the app's update message and in What's new.
    echo   To change them, edit that file first.
) else (
    echo   There are no release notes for !RC_V! yet: release-notes\!RC_V!.md doesn't exist or is empty.
    echo.
    echo     1  Write them now  ^(opens Notepad; save and close it to continue^)
    echo     2  Use a one-line note instead
    echo     3  Cancel
    echo.
    set "NC="
    set /p "NC=  Choose 1-3: "
    if "!NC!"=="1" (
        if not exist "%~dp0release-notes" mkdir "%~dp0release-notes"
        if not exist "!RC_FILE!" (
            > "!RC_FILE!" echo # ClipPro !RC_V!
            >> "!RC_FILE!" echo.
            >> "!RC_FILE!" echo One or two sentences on what's new. Users see this first in the update message.
            >> "!RC_FILE!" echo.
            >> "!RC_FILE!" echo ## New
            >> "!RC_FILE!" echo.
            >> "!RC_FILE!" echo -
            >> "!RC_FILE!" echo.
            >> "!RC_FILE!" echo ## Fixes
            >> "!RC_FILE!" echo.
            >> "!RC_FILE!" echo -
        )
        start /wait notepad "!RC_FILE!"
        goto notes_check
    )
    if "!NC!"=="2" (
        echo   What changed? ^(one line; avoid double quotes^)
        set /p "RC_NOTES=  Note: "
        if "!RC_NOTES!"=="" ( echo   No note given. & goto done )
    )
    if not "!NC!"=="1" if not "!NC!"=="2" goto menu
)
echo.
set "YN="
set /p "YN=  Build release !RC_V! now? (y/n): "
if /i not "!YN!"=="y" goto menu
echo.
set "RC_VER=!RC_V!"
%PS% -Command "& '%~dp0release.ps1' new $env:RC_VER $env:RC_NOTES"
if errorlevel 1 goto done
echo.
set "YN="
set /p "YN=  Follow the build now? It takes about 30-40 minutes. (y/n): "
if /i "!YN!"=="y" goto watch
goto done

REM ---------------------------------------------------------------- watch
:watch
cls
echo.
echo   Following the latest build (Ctrl+C stops watching; the build keeps going)...
echo.
%PS% -File release.ps1 watch
echo.
echo   When it says completed, test the draft from option 7, then publish with option 3.
goto done

REM ---------------------------------------------------------------- publish
:publish
cls
echo.
echo   PUBLISH A DRAFT
echo.
call :drafts
set "LATEST="
for /f "usebackq delims=" %%t in (`%PS% -Command "$d = gh release list --repo $env:REPO --limit 20 --json tagName,isDraft | ConvertFrom-Json | Where-Object { $_.isDraft } | Select-Object -First 1; if ($d) { $d.tagName }"`) do set "LATEST=%%t"
if "!LATEST!"=="" ( echo   There are no drafts to publish. & goto done )
set "RC_VER="
set /p "RC_VER=  Version to publish [Enter = !LATEST!]: "
if "!RC_VER!"=="" set "RC_VER=!LATEST!"
set "YN="
set /p "YN=  Publish !RC_VER! to everyone now? (y/n): "
if /i not "!YN!"=="y" goto menu
%PS% -Command "& '%~dp0release.ps1' publish $env:RC_VER"
goto done

REM ---------------------------------------------------------------- status
:status
cls
echo.
%PS% -File release.ps1 status
goto done

REM ---------------------------------------------------------------- unpublish
:unpublish
cls
echo.
echo   UNPUBLISH (turns a published release back into a draft)
echo.
gh release list --repo %REPO% --limit 10
echo.
set "RC_VER="
set /p "RC_VER=  Version to unpublish (like 1.2.0): "
if "!RC_VER!"=="" goto menu
set "YN="
set /p "YN=  Unpublish !RC_VER!? Users who already updated keep it. (y/n): "
if /i not "!YN!"=="y" goto menu
%PS% -Command "& '%~dp0release.ps1' unpublish $env:RC_VER"
goto done

REM ---------------------------------------------------------------- delete
:delete
cls
echo.
echo   DELETE A DRAFT
echo.
call :drafts
set "RC_VER="
set /p "RC_VER=  Draft version to delete (like 1.2.0): "
if "!RC_VER!"=="" goto menu
set "YN="
set /p "YN=  Delete draft !RC_VER! for good? (y/n): "
if /i not "!YN!"=="y" goto menu
%PS% -Command "& '%~dp0release.ps1' delete $env:RC_VER"
goto done

REM ---------------------------------------------------------------- open page
:open
start "" "https://github.com/%REPO%/releases"
goto menu

REM ---------------------------------------------------------------- helpers
:drafts
echo   Drafts (only you can see these):
%PS% -Command "$d = @(gh release list --repo $env:REPO --limit 20 --json tagName,isDraft,createdAt | ConvertFrom-Json | Where-Object { $_.isDraft }); if ($d.Count) { $d | ForEach-Object { '    ' + $_.tagName + '   made ' + ([string]$_.createdAt).Substring(0, 10) } } else { '    (none)' }"
echo.
exit /b 0

:done
echo.
pause
goto menu
