#!/usr/bin/env bash
set -euo pipefail

# Host folders to mount read-only (semicolon-separated); default current working directory
DEFAULT_HOST_FOLDER="$(pwd -P)"
HOST_FOLDERS="${DWG_MCP_HOST_FOLDERS:-$DEFAULT_HOST_FOLDER}"

MOUNTS=()
EXPOSED_FOLDERS=()

IFS=';' read -r -a FOLDER_ITEMS <<< "$HOST_FOLDERS"
for raw_folder in "${FOLDER_ITEMS[@]}"; do
  # Trim leading/trailing whitespace
  folder="${raw_folder#"${raw_folder%%[![:space:]]*}"}"
  folder="${folder%"${folder##*[![:space:]]}"}"
  if [[ -z "$folder" ]]; then continue; fi
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

# unique name per process for multi-instance
CONTAINER_NAME="dwg-mcp-server-$$"
cleanup() { docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
cleanup  # remove leftover from prior run

exec docker run --rm -i \
  --name "$CONTAINER_NAME" \
  -e "DWG_MCP_HOST_FOLDERS=${CONTAINER_FOLDERS}" \
  "${MOUNTS[@]}" \
  dwg-mcp-server "$@"
