from __future__ import annotations

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


def resolve_root_relative_path(root_path: Path, relative_path_text: str) -> Path:
    relative_path = Path(relative_path_text)
    if not relative_path_text.strip():
        raise ValueError("relativePath must not be empty")
    if relative_path.is_absolute():
        raise ValueError("relativePath must be relative to the selected MCP root")

    resolved_root = root_path.resolve(strict=True)
    resolved_file = (resolved_root / relative_path).resolve(strict=True)
    try:
        resolved_file.relative_to(resolved_root)
    except ValueError as error:
        raise ValueError("relativePath escapes the selected MCP root") from error

    if not resolved_file.is_file():
        raise ValueError("relativePath must point to a regular DWG file")
    if resolved_file.suffix.lower() != ".dwg":
        raise ValueError("relativePath must point to a .dwg file")

    return resolved_file
