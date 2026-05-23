#!/usr/bin/env bash
set -euo pipefail

# Host folders to mount read-only at their original absolute paths
# (semicolon-separated). If DWG_MCP_DOCKER_MOUNTS is omitted, configured
# allowed roots are mounted. If both are omitted, default to $HOME.
#
# Access is authorized by MCP client roots or explicit DWG_MCP_ALLOWED_ROOTS
# inside the server. Mounts only make those paths visible inside the container.
if [[ -n "${DWG_MCP_DOCKER_MOUNTS:-}" ]]; then
  HOST_FOLDERS="$DWG_MCP_DOCKER_MOUNTS"
  MOUNT_SOURCE="DWG_MCP_DOCKER_MOUNTS"
elif [[ -n "${DWG_MCP_ALLOWED_ROOTS:-}" ]]; then
  HOST_FOLDERS="$DWG_MCP_ALLOWED_ROOTS"
  MOUNT_SOURCE="DWG_MCP_ALLOWED_ROOTS"
else
  HOST_FOLDERS="$HOME"
  MOUNT_SOURCE="\$HOME"
fi

MOUNTS=()
ENV_ARGS=()

if [[ -n "${DWG_MCP_ALLOWED_ROOTS:-}" ]]; then
  ENV_ARGS+=(-e "DWG_MCP_ALLOWED_ROOTS=$DWG_MCP_ALLOWED_ROOTS")
fi
ENV_ARGS+=(-e "DWG_MCP_RUNNING_IN_DOCKER=1")
ENV_ARGS+=(-e "DWG_MCP_DOCKER_MOUNTS=$HOST_FOLDERS")

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
  echo "$MOUNT_SOURCE does not point to any existing directories: $HOST_FOLDERS" >&2
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
