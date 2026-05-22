#!/usr/bin/env bash
set -euo pipefail

HOST_FOLDERS="${DWG_MCP_DOCKER_MOUNTS:-${DWG_MCP_ALLOWED_ROOTS:-$HOME/Documents}}"
IMAGE="ghcr.io/dimitrovakulenko/dwg-mcp-server:latest"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required but was not found in PATH" >&2
  exit 1
fi

MOUNTS=()
ENV_ARGS=()

if [[ -n "${DWG_MCP_ALLOWED_ROOTS:-}" ]]; then
  ENV_ARGS+=(-e "DWG_MCP_ALLOWED_ROOTS=$DWG_MCP_ALLOWED_ROOTS")
fi

IFS=';' read -r -a FOLDER_ITEMS <<< "$HOST_FOLDERS"
for raw_folder in "${FOLDER_ITEMS[@]}"; do
  folder="${raw_folder#"${raw_folder%%[![:space:]]*}"}"
  folder="${folder%"${folder##*[![:space:]]}"}"
  if [[ -z "$folder" ]]; then
    continue
  fi
  if [[ -d "$folder" ]]; then
    MOUNTS+=(-v "${folder}:${folder}:ro")
  fi
done

if [[ ${#MOUNTS[@]} -eq 0 ]]; then
  echo "DWG_MCP_DOCKER_MOUNTS does not point to any existing directories: $HOST_FOLDERS" >&2
  exit 1
fi

CONTAINER_NAME="dwg-mcp-server-$$"

cleanup() {
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
}

trap cleanup EXIT INT TERM
cleanup

DOCKER_ARGS=(run --rm -i --platform linux/amd64 --name "$CONTAINER_NAME")
DOCKER_ARGS+=("${ENV_ARGS[@]}")
DOCKER_ARGS+=("${MOUNTS[@]}")
DOCKER_ARGS+=("$IMAGE")
DOCKER_ARGS+=("$@")

docker pull --platform linux/amd64 "$IMAGE" >/dev/null

exec docker "${DOCKER_ARGS[@]}"
