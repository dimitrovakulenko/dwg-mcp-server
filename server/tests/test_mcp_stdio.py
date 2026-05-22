from __future__ import annotations

import sys
import unittest
from pathlib import Path

try:
    from .mcp_test_client import McpProcessClient
except ImportError:  # pragma: no cover
    from mcp_test_client import McpProcessClient


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def test_data_root_uri() -> str:
    return (repo_root() / "testData").resolve().as_uri()


def open_args(relative_path: str = "house_plan.dwg") -> dict[str, str]:
    return {
        "rootUri": test_data_root_uri(),
        "relativePath": relative_path,
    }


class McpStdioTests(unittest.TestCase):
    def setUp(self) -> None:
        self.client = McpProcessClient(
            [sys.executable, "-m", "dwg_mcp_server"],
            cwd=repo_root(),
            env={
                "PYTHONPATH": str(repo_root() / "server" / "src"),
            },
            root_uris=[test_data_root_uri()],
        )

    def tearDown(self) -> None:
        self.client.terminate()

    def test_mcp_stdio_smoke(self) -> None:
        init = self.client.initialize()
        self.assertEqual(init["result"]["serverInfo"]["name"], "dwg-mcp-server")

        listed_tools = self.client.request("tools/list", {})
        tool_names = [tool["name"] for tool in listed_tools["result"]["tools"]]
        self.assertIn("dwg.list_roots", tool_names)
        self.assertIn("dwg.open_file", tool_names)
        self.assertIn("dwg.describe_type", tool_names)
        self.assertIn("dwg.get_objects", tool_names)
        self.assertIn("dwg.query_objects", tool_names)

        roots = self.client.request("tools/call", {"name": "dwg.list_roots", "arguments": {}})
        self.assertEqual(
            roots["result"]["structuredContent"]["roots"][0]["uri"],
            test_data_root_uri(),
        )

        described = self.client.request(
            "tools/call",
            {
                "name": "dwg.describe_type",
                "arguments": {"typeName": "AcDb3dPolyline"},
            },
        )["result"]["structuredContent"]
        self.assertEqual(described["typeName"], "AcDb3dPolyline")
        property_names = {item["name"] for item in described["properties"]}
        self.assertIn("flag", property_names)

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        file_types = self.client.request(
            "tools/call",
            {
                "name": "dwg.list_file_types",
                "arguments": {
                    "documentId": document_id,
                    "regex": "^AcDbBlock",
                    "limit": 2,
                },
            },
        )
        structured = file_types["result"]["structuredContent"]
        self.assertEqual(structured["total"], 4)
        self.assertEqual(structured["nextCursor"], "2")
        self.assertEqual(
            [item["typeName"] for item in structured["items"]],
            ["AcDbBlockBegin", "AcDbBlockEnd"],
        )

        layer_handles = self.client.request(
            "tools/call",
            {
                "name": "dwg.query_objects",
                "arguments": {
                    "documentId": document_id,
                    "typeName": "AcDbLayerTableRecord",
                    "whereClauses": [
                        {"property": "name", "op": "eq", "value": "0"}
                    ],
                    "mode": "handles",
                    "limit": 1,
                },
            },
        )["result"]["structuredContent"]["handles"]

        fetched = self.client.request(
            "tools/call",
            {
                "name": "dwg.get_objects",
                "arguments": {
                    "documentId": document_id,
                    "handles": [layer_handles[0], "missing-handle"],
                    "projection": "full",
                },
            },
        )["result"]["structuredContent"]
        self.assertEqual(fetched["items"][0]["handle"], layer_handles[0])
        self.assertEqual(fetched["items"][0]["properties"]["name"], "0")
        self.assertEqual(fetched["missingHandles"], ["missing-handle"])

        closed = self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )
        self.assertTrue(closed["result"]["structuredContent"]["closed"])

    def test_query_objects_pagination_via_mcp(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        first_page = self.client.request(
            "tools/call",
            {
                "name": "dwg.query_objects",
                "arguments": {
                    "documentId": document_id,
                    "mode": "handles",
                    "whereClauses": [
                        {"property": "kind", "op": "eq", "value": "entity"}
                    ],
                    "limit": 2,
                },
            },
        )["result"]["structuredContent"]
        self.assertEqual(first_page["total"], 3891)
        self.assertEqual(len(first_page["handles"]), 2)
        self.assertEqual(first_page["nextCursor"], "2")

        second_page = self.client.request(
            "tools/call",
            {
                "name": "dwg.query_objects",
                "arguments": {
                    "documentId": document_id,
                    "mode": "handles",
                    "whereClauses": [
                        {"property": "kind", "op": "eq", "value": "entity"}
                    ],
                    "limit": 2,
                    "cursor": first_page["nextCursor"],
                },
            },
        )["result"]["structuredContent"]
        self.assertEqual(second_page["total"], 3891)
        self.assertEqual(len(second_page["handles"]), 2)
        self.assertEqual(second_page["nextCursor"], "4")
        self.assertNotEqual(first_page["handles"], second_page["handles"])

        self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )

    def test_open_file_accepts_relative_path_under_roots(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        structured = opened["result"]["structuredContent"]
        self.assertEqual(structured["rootUri"], test_data_root_uri())
        self.assertEqual(structured["relativePath"], "house_plan.dwg")

        self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": structured["documentId"]},
            },
        )

    def test_open_file_rejects_unknown_root_uri(self) -> None:
        restricted_client = McpProcessClient(
            [sys.executable, "-m", "dwg_mcp_server"],
            cwd=repo_root(),
            env={
                "PYTHONPATH": str(repo_root() / "server" / "src"),
            },
            root_uris=[(repo_root() / "server").resolve().as_uri()],
        )
        self.addCleanup(restricted_client.terminate)

        restricted_client.initialize()
        denied = restricted_client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        self.assertTrue(denied["result"]["isError"])
        self.assertIn("rootUri must exactly match", denied["result"]["content"][0]["text"])

    def test_open_file_rejects_relative_path_traversal(self) -> None:
        self.client.initialize()

        denied = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args("../README.md"),
            },
        )
        self.assertTrue(denied["result"]["isError"])
        self.assertIn("escapes", denied["result"]["content"][0]["text"])

    def test_open_file_requires_client_roots(self) -> None:
        client_without_roots = McpProcessClient(
            [sys.executable, "-m", "dwg_mcp_server"],
            cwd=repo_root(),
            env={
                "PYTHONPATH": str(repo_root() / "server" / "src"),
            },
        )
        self.addCleanup(client_without_roots.terminate)

        client_without_roots.initialize()
        denied = client_without_roots.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        self.assertTrue(denied["result"]["isError"])
        self.assertIn("MCP client roots are required", denied["result"]["content"][0]["text"])

    def test_allowed_root_args_work_without_client_roots(self) -> None:
        client_with_allowed_root = McpProcessClient(
            [
                sys.executable,
                "-m",
                "dwg_mcp_server",
                "--allowed-root",
                str(repo_root() / "testData"),
            ],
            cwd=repo_root(),
            env={
                "PYTHONPATH": str(repo_root() / "server" / "src"),
            },
        )
        self.addCleanup(client_with_allowed_root.terminate)

        client_with_allowed_root.initialize()
        roots = client_with_allowed_root.request(
            "tools/call",
            {"name": "dwg.list_roots", "arguments": {}},
        )
        self.assertEqual(
            roots["result"]["structuredContent"]["roots"][0]["uri"],
            test_data_root_uri(),
        )

        opened = client_with_allowed_root.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        structured = opened["result"]["structuredContent"]
        self.assertEqual(structured["rootUri"], test_data_root_uri())
        self.assertEqual(structured["relativePath"], "house_plan.dwg")

        client_with_allowed_root.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": structured["documentId"]},
            },
        )

    def test_dynamic_block_history_xrecord_is_exposed_via_mcp(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args("dyn-blocks.dwg"),
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        block_reference = self.client.request(
            "tools/call",
            {
                "name": "dwg.get_objects",
                "arguments": {
                    "documentId": document_id,
                    "handles": ["CBD"],
                    "select": [
                        "xdicobjhandle",
                        "block_representation_dict_handle",
                        "app_data_cache_handle",
                        "enhanced_block_data_handle",
                        "enhanced_block_data_xrecords",
                    ],
                },
            },
        )["result"]["structuredContent"]["items"][0]["properties"]
        self.assertEqual(block_reference["xdicobjhandle"], "CBE")
        self.assertEqual(block_reference["block_representation_dict_handle"], "CF2")
        self.assertEqual(block_reference["app_data_cache_handle"], "CF4")
        self.assertEqual(block_reference["enhanced_block_data_handle"], "D13")
        self.assertEqual(
            block_reference["enhanced_block_data_xrecords"],
            ["D14", "D17", "D18", "D15", "D16"],
        )

        history = self.client.request(
            "tools/call",
            {
                "name": "dwg.get_objects",
                "arguments": {
                    "documentId": document_id,
                    "handles": ["D14", "D15"],
                    "select": ["ownerhandle", "num_xdata", "xdata"],
                },
            },
        )["result"]["structuredContent"]["items"]

        self.assertEqual(history[0]["properties"]["ownerhandle"], "D13")
        self.assertEqual(history[0]["properties"]["num_xdata"], 7)
        self.assertEqual(
            history[0]["properties"]["xdata"],
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
        self.assertEqual(
            history[1]["properties"]["xdata"],
            [
                [1071, 6895636],
                [1071, 9291323],
                [70, 25],
                [70, 104],
                [40, 0],
            ],
        )

        self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )

    def test_full_object_queries_include_extended_data_via_mcp(self) -> None:
        self.client.initialize()

        opened = self.client.request(
            "tools/call",
            {
                "name": "dwg.open_file",
                "arguments": open_args(),
            },
        )
        document_id = opened["result"]["structuredContent"]["documentId"]

        queried = self.client.request(
            "tools/call",
            {
                "name": "dwg.query_objects",
                "arguments": {
                    "documentId": document_id,
                    "typeName": "AcDbText",
                    "scope": {"space": "modelSpace"},
                    "mode": "full",
                    "limit": 1,
                },
            },
        )["result"]["structuredContent"]
        self.assertTrue(queried["items"])
        self.assertEqual(queried["items"][0]["extendedData"]["space"], "modelSpace")
        self.assertIn("containerBlockHandle", queried["items"][0]["extendedData"])

        fetched = self.client.request(
            "tools/call",
            {
                "name": "dwg.get_objects",
                "arguments": {
                    "documentId": document_id,
                    "handles": [queried["items"][0]["handle"]],
                    "projection": "full",
                },
            },
        )["result"]["structuredContent"]
        self.assertEqual(fetched["items"][0]["extendedData"]["space"], "modelSpace")

        self.client.request(
            "tools/call",
            {
                "name": "dwg.close_file",
                "arguments": {"documentId": document_id},
            },
        )
