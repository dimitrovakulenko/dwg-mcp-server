#!/usr/bin/env bash
set -euo pipefail

# Host folders to mount read-only at their original absolute paths
# (semicolon-separated). If DWG_MCP_DOCKER_MOUNTS is omitted, configured
# allowed roots are mounted. If both are omitted, default to $HOME/Documents.
#
# Access is authorized by MCP client roots or explicit DWG_MCP_ALLOWED_ROOTS
# inside the server. Mounts only make those paths visible inside the container.
HOST_FOLDERS="${DWG_MCP_DOCKER_MOUNTS:-${DWG_MCP_ALLOWED_ROOTS:-$HOME/Documents}}"

MOUNTS=()
ENV_ARGS=()

if [[ -n "${DWG_MCP_ALLOWED_ROOTS:-}" ]]; then
  ENV_ARGS+=(-e "DWG_MCP_ALLOWED_ROOTS=$DWG_MCP_ALLOWED_ROOTS")
fi

IFS=';' read -r -a FOLDER_ITEMS <<< "$HOST_FOLDERS"
for raw_folder in "${FOLDER_ITEMS[@]}"; do
  # Trim leading/trailing whitespace
  folder="${raw_folder#"${raw_folder%%[![:space:]]*}"}"
  folder="${folder%"${folder##*[![:space:]]}"}"
  if [[ -z "$folder" ]]; then continue; fi
  if [[ -d "$folder" ]]; then
    MOUNTS+=(-v "${folder}:${folder}:ro")
  fi
done

if [[ ${#MOUNTS[@]} -eq 0 ]]; then
  echo "DWG_MCP_DOCKER_MOUNTS does not point to any existing directories: $HOST_FOLDERS" >&2
  exit 1
fi

# unique name per process for multi-instance
CONTAINER_NAME="dwg-mcp-server-$$"
cleanup() { docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
cleanup  # remove leftover from prior run

exec docker run --rm -i \
  --platform "${DWG_MCP_DOCKER_PLATFORM:-linux/amd64}" \
  --name "$CONTAINER_NAME" \
  "${ENV_ARGS[@]}" \
  "${MOUNTS[@]}" \
  dwg-mcp-server "$@"
