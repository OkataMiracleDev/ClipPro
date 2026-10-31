# Releasing ClipPro

Pushing code never makes a release. A release happens only when you start one, and users only see it once you publish it. Publishing does two things:

- ClipPro offers the update at its next start-up, with your release notes.
- okata.space shows the new version, its notes and its download within about 10 minutes.

## One-time setup

1. **Signing key → GitHub.** Every installer is signed so installed copies can verify updates. The key is in `D:\ClipPro Data\keys` (keep a backup somewhere safe: lose it and existing installs can't be updated). Upload it once:

   ```powershell
   powershell -File scripts\setup-release-secrets.ps1
   ```

2. **GitHub CLI** signed in as the repository owner: `winget install GitHub.cli`, then `gh auth login`.

## Every release

Double-click **`release.bat`** in the repository folder and follow the menu. Or use `release.ps1` directly.

| Step | release.bat | release.ps1 |
|---|---|---|
| Write the notes | option 1 offers to open Notepad | `release-notes\<version>.md` |
| Build a draft (about 30–40 min) | 1 | `.\release.ps1 new patch` (or `minor`, `major`, `1.2.0`) |
| Follow the build | 2 | `.\release.ps1 watch` |
| Test the draft | 7 opens the releases page; download the installer from the draft and try it | |
| Publish | 3 | `.\release.ps1 publish 1.2.0` |
| See everything | 4 | `.\release.ps1 status` |
| Take a release back | 5 (back to draft) or 6 (delete a draft) | `unpublish` / `delete` |

What "new" does:

1. It commits any changes you confirm.
2. It writes the version into the source and adds your notes to the in-app What's new.
3. It commits "Release x.y.z", pushes, and starts **Actions → Release** on GitHub.

The workflow builds the installer and portable ZIP, signs the update, and creates a **draft** release containing:

- `ClipPro_x.y.z_x64-setup.exe` and its `.sig`;
- `ClipPro-setup-x64.exe`, the fixed name the website's download button uses;
- the portable ZIPs;
- `latest.json`, the update manifest the app reads.

## Release notes

`release-notes\<version>.md`: the first paragraph is the summary users see first in the update message; then `## New` / `## Fixes` sections. They appear in four places:

- on the GitHub release;
- in the app's update message;
- in Help → What's new;
- on okata.space/build/clippro.

## Versions

Patch for fixes (1.0.0 → 1.0.1), minor for features (1.0.0 → 1.1.0), major for big changes. The app only offers a published release that is newer than itself. Drafts and pre-releases are never offered.
