#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dwgread="$root/third_party/libredwg/programs/dwgread"

[[ $# -ge 1 && $# -le 2 ]] || { echo "Usage: $0 <input.dwg> [output.json]" >&2; exit 1; }
[[ -f "$1" ]] || { echo "DWG file not found: $1" >&2; exit 1; }
[[ -x "$dwgread" ]] || { echo "dwgread not found. Run scripts/build-libredwg.sh first." >&2; exit 1; }

if [[ $# -eq 2 ]]; then
  out="$2"
else
  out="$(dirname "$1")/$(basename "$1" .dwg).libredwg.json"
fi

"$dwgread" -O JSON -o "$out" "$1"
