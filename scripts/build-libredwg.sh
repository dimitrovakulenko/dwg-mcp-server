#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$root/third_party/libredwg"
patch="$root/patches/libredwg-binary-dxf.patch"
[[ -d "$src" ]] || { echo "LibreDWG not found. Run: git submodule update --init --recursive" >&2; exit 1; }
[[ -x "$src/autogen.sh" ]] || { echo "Invalid LibreDWG checkout: $src" >&2; exit 1; }

cd "$src"
git apply --reverse --check "$patch" 2>/dev/null || git apply "$patch"
./autogen.sh --force
./configure --disable-docs --disable-shared --enable-static --disable-bindings --disable-python
make -j"$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"
