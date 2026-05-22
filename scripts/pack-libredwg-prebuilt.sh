#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-$("$root/scripts/detect-libredwg-target.sh")}"
source_root="${LIBREDWG_SOURCE_TREE:-$root/third_party/libredwg}"
archive="${LIBREDWG_PREBUILT_ARCHIVE:-$root/prebuilt/libredwg/$target.tar.gz}"

required=(
  "$source_root/src/.libs/libredwg.a"
  "$source_root/src/config.h"
  "$source_root/src/classes.c"
  "$source_root/src/dynapi.c"
  "$source_root/include/dwg_api.h"
)

for path in "${required[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "Missing required LibreDWG build file: $path" >&2
    echo "Build LibreDWG for $target first, then rerun this script." >&2
    exit 1
  fi
done

tmp="$(mktemp -d)"
cleanup() { rm -rf "$tmp"; }
trap cleanup EXIT

mkdir -p "$tmp/src/.libs" "$tmp/include" "$(dirname "$archive")"
cp "$source_root/src/.libs/libredwg.a" "$tmp/src/.libs/"
cp "$source_root/src/"*.h "$tmp/src/"
cp "$source_root/src/classes.c" "$tmp/src/"
cp "$source_root/src/dynapi.c" "$tmp/src/"
cp "$source_root/include/"*.h "$tmp/include/"

tar -czf "$archive" -C "$tmp" .

echo "Wrote $archive"
du -h "$archive" | awk '{print "Archive size: " $1}'
