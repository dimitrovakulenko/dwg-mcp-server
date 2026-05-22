from __future__ import annotations

import argparse
import asyncio
import os
from pathlib import Path

from .app import DwgMcpApplication


def _env_allowed_roots() -> list[str]:
    raw_value = os.environ.get("DWG_MCP_ALLOWED_ROOTS", "")
    return [item.strip() for item in raw_value.split(";") if item.strip()]


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="dwg-mcp-server",
        description="Read-only MCP server for inspecting AutoCAD DWG files.",
    )
    parser.add_argument(
        "--allowed-root",
        action="append",
        default=[],
        metavar="PATH",
        help=(
            "Explicit root directory allowed for DWG access when the MCP client does not "
            "provide roots. Repeat for multiple roots."
        ),
    )
    return parser.parse_args()


def main() -> None:
    args = _parse_args()
    allowed_roots = [Path(root) for root in [*_env_allowed_roots(), *args.allowed_root]]
    application = DwgMcpApplication(allowed_roots=allowed_roots)
    asyncio.run(application.run_stdio())


if __name__ == "__main__":
    main()
