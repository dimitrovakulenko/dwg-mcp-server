from __future__ import annotations

import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root))

from server.tests.mcp_test_client import McpProcessClient  # noqa: E402


client = McpProcessClient(sys.argv[1:], cwd=root)
try:
    client.initialize()
    response = client.request(
        "tools/call",
        {"name": "dwg.list_types", "arguments": {"limit": 1}},
    )
    assert response["result"]["structuredContent"]["total"] > 0
finally:
    client.terminate()
