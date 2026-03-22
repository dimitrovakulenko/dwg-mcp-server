from __future__ import annotations

import asyncio

from .app import DwgMcpApplication


def main() -> None:
    application = DwgMcpApplication()
    asyncio.run(application.run_stdio())


if __name__ == "__main__":
    main()
