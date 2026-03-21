#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
PYTHONPATH="$root/server/src" python -m unittest discover -s server/tests -v
