#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$root"
bash scripts/build-libredwg.sh

export LIBREDWG_SOURCE_ROOT="$root/third_party/libredwg/src"

cargo test --workspace
cargo build -p dwg-worker

PYTHONPATH="$root/server/src" \
DWG_WORKER_BIN="$root/target/debug/dwg-worker" \
python3 -m unittest \
  server.tests.test_file_access \
  server.tests.test_host \
  server.tests.test_mcp_stdio \
  -v
