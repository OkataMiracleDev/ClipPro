#!/usr/bin/env bash
# Fails if app/ui/src/bindings.ts is stale. Usage: scripts/check-bindings.sh [--write]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cargo build -q -p clippro-app
exe="$root/target/debug/ClipPro.exe"
if [ "${1:-}" = "--write" ]; then
  "$exe" --export-bindings "$root/app/ui/src/bindings.ts"
  echo "bindings written"
  exit 0
fi
tmp="$root/target/bindings-check.ts"
"$exe" --export-bindings "$tmp"
if diff -q <(tr -d '\r' < "$tmp") <(tr -d '\r' < "$root/app/ui/src/bindings.ts") >/dev/null; then
  echo "bindings up to date"; rm -f "$tmp"
else
  echo "app/ui/src/bindings.ts is stale: run scripts/check-bindings.sh --write" >&2; rm -f "$tmp"; exit 1
fi
