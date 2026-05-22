#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-$("$root/scripts/detect-libredwg-target.sh")}"
libredwg_root="$root/.cache/libredwg-root"

cd "$root"
bash scripts/use-libredwg-prebuilt.sh "$target" "$libredwg_root"

export LIBREDWG_ROOT_DIR="$libredwg_root"
export LIBREDWG_SOURCE_ROOT="$libredwg_root/src"

cargo test --workspace
cargo build -p dwg-worker

PYTHONPATH="$root/server/src" \
DWG_WORKER_BIN="$root/target/debug/dwg-worker" \
python3 -m unittest \
  server.tests.test_file_access \
  server.tests.test_host \
  server.tests.test_mcp_stdio \
  -v
