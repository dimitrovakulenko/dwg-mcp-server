from __future__ import annotations

import os
from pathlib import Path
from typing import Sequence
from urllib.parse import unquote, urlparse
from urllib.request import url2pathname


def normalize_local_path(path_text: str) -> Path:
    path = Path(path_text).expanduser()
    if not path.is_absolute():
        raise ValueError("path must be an absolute local path")
    return path.resolve(strict=False)


def file_uri_to_path(file_uri: str) -> Path:
    parsed = urlparse(file_uri)
    if parsed.scheme != "file":
        raise ValueError("fileUri must use the file:// scheme")
    if parsed.netloc not in ("", "localhost"):
        raise ValueError("fileUri must not include a remote host")

    path_text = url2pathname(unquote(parsed.path or ""))
    return normalize_local_path(path_text)


def ensure_within_roots(
    file_path: Path,
    root_paths: Sequence[Path],
    *,
    boundary_name: str = "client roots",
) -> None:
    resolved_file = file_path.resolve(strict=False)
    resolved_roots = [root.resolve(strict=False) for root in root_paths]

    if not resolved_roots:
        raise ValueError(f"no {boundary_name} were provided")

    for root in resolved_roots:
        try:
            resolved_file.relative_to(root)
            return
        except ValueError:
            continue

    allowed = ", ".join(str(root) for root in resolved_roots[:3])
    if len(resolved_roots) > 3:
        allowed = f"{allowed}, ..."
    raise ValueError(
        f"path is outside the {boundary_name}: {resolved_file}. Allowed roots: {allowed}"
    )


def configured_access_folders() -> list[Path]:
    raw_value = os.getenv("DWG_MCP_HOST_FOLDERS", "").strip()
    if not raw_value:
        return []

    folders: list[Path] = []
    for chunk in raw_value.split(";"):
        candidate = chunk.strip()
        if not candidate:
            continue
        folders.append(normalize_local_path(candidate))
    return folders


def format_access_folders(folders: Sequence[Path]) -> str:
    if not folders:
        return "none configured"
    return ", ".join(str(folder) for folder in folders)
