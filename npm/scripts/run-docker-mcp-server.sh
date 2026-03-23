#!/usr/bin/env bash
set -euo pipefail

HOST_FOLDERS="${DWG_MCP_HOST_FOLDERS:-$HOME/Documents}"
IMAGE="ghcr.io/dimitrovakulenko/dwg-mcp-server:latest"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required but was not found in PATH" >&2
  exit 1
fi

MOUNTS=()
EXPOSED_FOLDERS=()

IFS=';' read -r -a FOLDER_ITEMS <<< "$HOST_FOLDERS"
for raw_folder in "${FOLDER_ITEMS[@]}"; do
  folder="${raw_folder#"${raw_folder%%[![:space:]]*}"}"
  folder="${folder%"${folder##*[![:space:]]}"}"
  if [[ -z "$folder" ]]; then
    continue
  fi
  if [[ -d "$folder" ]]; then
    MOUNTS+=(-v "${folder}:${folder}:ro")
    EXPOSED_FOLDERS+=("$folder")
  fi
done

if [[ ${#MOUNTS[@]} -eq 0 ]]; then
  echo "DWG_MCP_HOST_FOLDERS does not point to any existing directories: $HOST_FOLDERS" >&2
  exit 1
fi

CONTAINER_FOLDERS="$(IFS=';'; printf '%s' "${EXPOSED_FOLDERS[*]}")"
CONTAINER_NAME="dwg-mcp-server-$$"

cleanup() {
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
}

trap cleanup EXIT INT TERM
cleanup

DOCKER_ARGS=(run --rm -i --platform linux/amd64 --name "$CONTAINER_NAME")
DOCKER_ARGS+=(-e "DWG_MCP_HOST_FOLDERS=${CONTAINER_FOLDERS}")
DOCKER_ARGS+=("${MOUNTS[@]}")
DOCKER_ARGS+=("$IMAGE")
DOCKER_ARGS+=("$@")

exec docker "${DOCKER_ARGS[@]}"
