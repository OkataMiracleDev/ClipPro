"""Writes latest.json, the update manifest the app reads (tauri-plugin-updater format).

    python scripts/latest_json.py <version> <installer.exe> <notes.md> <out.json> [repo]

The installer's `.sig` (made by `tauri build` with the signing key set) must sit beside it.
The URL points at the versioned installer on the GitHub release, which exists once the
release is published.
"""

import datetime
import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) not in (5, 6):
        print(__doc__, file=sys.stderr)
        return 2
    version, exe, notes, out = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
    repo = sys.argv[5] if len(sys.argv) == 6 else "OkataMiracleDev/ClipPro"
    sig = exe.with_name(exe.name + ".sig")
    if not sig.is_file():
        print(f"missing signature {sig} (was TAURI_SIGNING_PRIVATE_KEY set for the build?)", file=sys.stderr)
        return 1
    manifest = {
        "version": version,
        "notes": notes.read_text(encoding="utf-8").strip() if notes.is_file() else f"ClipPro {version}",
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "windows-x86_64": {
                "signature": sig.read_text(encoding="utf-8").strip(),
                "url": f"https://github.com/{repo}/releases/download/v{version}/{exe.name}",
            }
        },
    }
    out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
