#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-${LIBREDWG_PREBUILT_TARGET:-$("$root/scripts/detect-libredwg-target.sh")}}"
dest="${2:-${LIBREDWG_ROOT_DIR:-$root/.cache/libredwg-root}}"
archive="${LIBREDWG_PREBUILT_ARCHIVE:-$root/prebuilt/libredwg/$target.tar.gz}"

if [[ ! -f "$archive" ]]; then
  echo "Missing prebuilt LibreDWG archive: $archive" >&2
  echo "Create it with: bash scripts/pack-libredwg-prebuilt.sh $target" >&2
  exit 1
fi

rm -rf "$dest"
mkdir -p "$dest"
tar -xzf "$archive" -C "$dest"

required=(
  "$dest/src/.libs/libredwg.a"
  "$dest/src/config.h"
  "$dest/src/classes.c"
  "$dest/src/dynapi.c"
  "$dest/include/dwg_api.h"
)

for path in "${required[@]}"; do
  if [[ ! -f "$path" ]]; then
    echo "Invalid LibreDWG prebuilt archive $archive; missing $path" >&2
    exit 1
  fi
done

echo "$dest"
