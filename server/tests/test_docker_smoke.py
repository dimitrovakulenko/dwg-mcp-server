from __future__ import annotations

import subprocess
import unittest
from pathlib import Path

try:
    from .mcp_test_client import McpProcessClient
except ImportError:  # pragma: no cover
    from mcp_test_client import McpProcessClient


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def image_tag() -> str:
    return "dwg-mcp-server"


def house_plan() -> str:
    return str(repo_root() / "testData" / "house_plan.dwg")


def dyn_blocks() -> str:
    return str(repo_root() / "testData" / "dyn-blocks.dwg")


def docker_available() -> bool:
    try:
        completed = subprocess.run(
            ["docker", "info"],
            cwd=repo_root(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    except FileNotFoundError:
        return False
    return completed.returncode == 0


def require_docker() -> None:
    if docker_available():
        return
    raise AssertionError(
        "Docker must be installed and the daemon must be running for DockerSmokeTests. "
        "Start Docker Desktop or dockerd, then run: bash scripts/build-docker-mcp-server.sh"
    )


class DockerSmokeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        require_docker()
        subprocess.run(
            ["bash", str(repo_root() / "scripts" / "build-docker-mcp-server.sh")],
            cwd=repo_root(),
            check=True,
        )

    def setUp(self) -> None:
        self.client = McpProcessClient(
            [
                "bash",
                str(repo_root() / "scripts" / "run-docker-mcp-server.sh"),
            ],
            cwd=repo_root(),
            env={"DWG_MCP_HOST_FOLDERS": str(repo_root() / "testData")},
        )

    def tearDown(self) -> None:
        self.client.terminate()

    def test_docker_smoke(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": {"path": house_plan()},
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        queried = self.client.request(
            "tools/call",
            {
                "name": "dwg.query_objects",
                "arguments": {
                    "documentId": document_id,
                    "mode": "count",
                    "whereClauses": [
                        {"property": "kind", "op": "eq", "value": "entity"}
                    ],
                    "limit": 10,
                },
            },
        )
        self.assertEqual(queried["result"]["structuredContent"]["total"], 3891)

        closed = self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )
        self.assertTrue(closed["result"]["structuredContent"]["closed"])

    def test_docker_exposes_dynamic_block_history_xdata(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": {"path": dyn_blocks()},
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        fetched = self.client.request(
            "tools/call",
            {
                "name": "dwg.get_objects",
                "arguments": {
                    "documentId": document_id,
                    "handles": ["D14"],
                    "select": ["ownerhandle", "num_xdata", "xdata"],
                },
            },
        )["result"]["structuredContent"]["items"][0]["properties"]

        self.assertEqual(fetched["ownerhandle"], "D13")
        self.assertEqual(fetched["num_xdata"], 7)
        self.assertEqual(
            fetched["xdata"],
            [
                [1071, 18597260],
                [1071, 25303744],
                [70, 25],
                [70, 104],
                [10, [-16.450129447944846, -0.09901143873563002, 0]],
                [10, [1982.9324090756895, -0.09901143873566041, 0]],
                [10, [0, 0, -1]],
            ],
        )

        closed = self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )
        self.assertTrue(closed["result"]["structuredContent"]["closed"])
