#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WITH_LIBREDWG=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-libredwg)
      WITH_LIBREDWG=1
      ;;
    *)
      echo "usage: $0 [--with-libredwg]" >&2
      echo "  Removes Rust target/, Python caches, egg-info, and Docker image dwg-mcp-server:latest." >&2
      echo "  --with-libredwg  also runs make clean in third_party/libredwg (full LibreDWG rebuild next time)." >&2
      exit 1
      ;;
  esac
  shift
done

echo "== cargo clean (workspace) =="
cargo clean

echo "== Python __pycache__ and .egg-info =="
find "$ROOT" -type d -name __pycache__ -exec rm -rf {} + 2>/dev/null || true
find "$ROOT" -type d -name '*.egg-info' -exec rm -rf {} + 2>/dev/null || true

if [[ "$WITH_LIBREDWG" -eq 1 ]] && [[ -f "$ROOT/third_party/libredwg/Makefile" ]]; then
  echo "== make clean (third_party/libredwg) =="
  make -C "$ROOT/third_party/libredwg" clean
fi

echo "== Docker image dwg-mcp-server:latest =="
if docker image inspect dwg-mcp-server:latest >/dev/null 2>&1; then
  docker rmi dwg-mcp-server:latest
else
  echo "(no dwg-mcp-server:latest image)"
fi

echo "Done. Rebuild native: bash scripts/build-libredwg.sh && cargo build -p dwg-worker --release"
echo "Rebuild Docker: bash scripts/build-docker-mcp-server.sh"
