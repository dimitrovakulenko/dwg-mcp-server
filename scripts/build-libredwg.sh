#!/usr/bin/env bash
set -euo pipefail

src="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/third_party/libredwg"
[[ -d "$src" ]] || { echo "LibreDWG not found. Run: git submodule update --init --recursive" >&2; exit 1; }
[[ -x "$src/autogen.sh" ]] || { echo "Invalid LibreDWG checkout: $src" >&2; exit 1; }

cd "$src"
./autogen.sh
./configure --disable-docs
make -j"$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"
