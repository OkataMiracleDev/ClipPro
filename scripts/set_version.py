"""Sets ClipPro's version everywhere it is recorded (release.ps1 and the release workflow).

    python scripts/set_version.py 1.2.0

Cargo workspace (Cargo.toml), the app (app/src-tauri/tauri.conf.json) and the UI package
(app/ui/package.json and its lock file). Cargo.lock follows on the next cargo command.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        print("usage: set_version.py 1.2.0", file=sys.stderr)
        return 2
    v = sys.argv[1]

    cargo = ROOT / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    # Only the [workspace.package] version, the first `version =` in the file.
    new, n = re.subn(r'(?m)^version = "[^"]+"', f'version = "{v}"', text, count=1)
    if n != 1:
        print("Cargo.toml: no workspace version found", file=sys.stderr)
        return 1
    cargo.write_text(new, encoding="utf-8", newline="\n")

    for rel, keys in [("app/src-tauri/tauri.conf.json", [()]), ("app/ui/package.json", [()]), ("app/ui/package-lock.json", [(), ("packages", "")])]:
        p = ROOT / rel
        data = json.loads(p.read_text(encoding="utf-8"))
        for path in keys:
            node = data
            for k in path:
                node = node[k]
            node["version"] = v
        p.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    print(v)
    return 0


if __name__ == "__main__":
    sys.exit(main())
