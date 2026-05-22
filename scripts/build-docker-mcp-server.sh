#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
exec docker build \
  --platform "${DWG_MCP_DOCKER_PLATFORM:-linux/amd64}" \
  --build-arg "LIBREDWG_TARGET=${LIBREDWG_PREBUILT_TARGET:-x86_64-unknown-linux-gnu}" \
  -t dwg-mcp-server \
  -f docker/Dockerfile \
  .
