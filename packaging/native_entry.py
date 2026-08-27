from __future__ import annotations

import os
import sys
from pathlib import Path

from dwg_mcp_server.__main__ import main


if __name__ == "__main__":
    bundle = Path(sys.executable).resolve().parent
    worker = bundle / ("dwg-worker.exe" if os.name == "nt" else "dwg-worker")
    os.environ.setdefault("DWG_WORKER_BIN", str(worker))
    os.environ.setdefault("LIBREDWG_SOURCE_ROOT", str(bundle / "libredwg"))
    main()
