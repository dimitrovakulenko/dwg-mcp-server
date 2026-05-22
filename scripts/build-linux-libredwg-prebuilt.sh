#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:-x86_64-unknown-linux-gnu}"
platform="${DWG_MCP_PREBUILD_PLATFORM:-linux/amd64}"
image="dwg-libredwg-prebuilt:$target"
archive="$root/prebuilt/libredwg/$target.tar.gz"

cd "$root"
mkdir -p "$(dirname "$archive")"

docker build \
  --platform "$platform" \
  --build-arg "LIBREDWG_TARGET=$target" \
  -t "$image" \
  -f docker/Dockerfile.libredwg-prebuilt \
  .

container="$(docker create "$image")"
cleanup() { docker rm -f "$container" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker cp "$container:/out/$target.tar.gz" "$archive"
echo "Wrote $archive"
du -h "$archive" | awk '{print "Archive size: " $1}'
