from __future__ import annotations

import os
import unittest
from pathlib import Path
from unittest.mock import patch

from dwg_mcp_server.app import DwgMcpApplication
from dwg_mcp_server.worker_client import SessionManager, UnknownDocumentError


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def house_plan() -> str:
    return str(repo_root() / "testData" / "house_plan.dwg")


def house_plan_uri() -> str:
    return (repo_root() / "testData" / "house_plan.dwg").resolve().as_uri()


class SessionManagerTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.manager = SessionManager(worker_cwd=repo_root())

    async def asyncTearDown(self) -> None:
        await self.manager.close_all()

    async def test_open_list_file_types_and_close(self) -> None:
        opened = await self.manager.open_file(house_plan())
        self.assertIn("documentId", opened)
        self.assertEqual(opened["backend"], "libredwg-native")

        file_types = await self.manager.list_file_types(
            opened["documentId"],
            regex="^AcDbBlock",
            limit=2,
        )
        self.assertEqual(file_types["total"], 4)
        self.assertEqual(file_types["nextCursor"], "2")
        self.assertEqual(
            [item["typeName"] for item in file_types["items"]],
            ["AcDbBlockBegin", "AcDbBlockEnd"],
        )

        closed = await self.manager.close_file(opened["documentId"])
        self.assertTrue(closed["closed"])
        self.assertEqual(closed["documentId"], opened["documentId"])

    async def test_list_types_supports_regex_and_cursor(self) -> None:
        first_page = await self.manager.list_types(
            regex="^AcDb3(PointAngularDimension|dPolyline)$",
            limit=1,
        )
        self.assertEqual(first_page["total"], 2)
        self.assertEqual(first_page["nextCursor"], "1")
        self.assertEqual(first_page["items"][0]["typeName"], "AcDb3PointAngularDimension")

        second_page = await self.manager.list_types(
            regex="^AcDb3(PointAngularDimension|dPolyline)$",
            limit=1,
            cursor=first_page["nextCursor"],
        )
        self.assertIsNone(second_page["nextCursor"])
        self.assertEqual(second_page["items"][0]["typeName"], "AcDb3dPolyline")

    async def test_unknown_document_id_is_rejected(self) -> None:
        with self.assertRaises(UnknownDocumentError):
            await self.manager.list_file_types("missing-document")


class ApplicationTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.manager = SessionManager(worker_cwd=repo_root())
        self.app = DwgMcpApplication(session_manager=self.manager)

    async def asyncTearDown(self) -> None:
        await self.manager.close_all()

    async def test_tool_catalog_and_tool_calls(self) -> None:
        tool_names = [tool.name for tool in self.app.tool_definitions()]
        self.assertEqual(
            tool_names,
            [
                "dwg.open_file",
                "dwg.close_file",
                "dwg.list_types",
                "dwg.list_file_types",
                "dwg.describe_type",
                "dwg.get_objects",
                "dwg.query_objects",
            ],
        )

        described = await self.app.call_tool(
            "dwg.describe_type",
            {"typeName": "AcDb3PointAngularDimension"},
        )
        self.assertEqual(described["typeName"], "AcDb3PointAngularDimension")
        property_names = {item["name"] for item in described["properties"]}
        self.assertIn("center_pt", property_names)

        opened = await self.app.call_tool("dwg.open_file", {"path": house_plan()})
        self.assertIn("documentId", opened)
        self.assertEqual(opened["path"], house_plan())
        self.assertEqual(opened["fileUri"], house_plan_uri())

        listed = await self.app.call_tool(
            "dwg.list_file_types",
            {
                "documentId": opened["documentId"],
                "regex": "^AcDbBlockReference$",
                "limit": 10,
            },
        )
        self.assertEqual(listed["total"], 1)
        self.assertEqual(listed["items"][0]["typeName"], "AcDbBlockReference")

        layer_query = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": opened["documentId"],
                "typeName": "AcDbLayerTableRecord",
                "whereClauses": [
                    {
                        "property": "name",
                        "op": "eq",
                        "value": "0",
                    }
                ],
                "mode": "handles",
                "limit": 1,
            },
        )
        layer_handle = layer_query["handles"][0]

        fetched = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": opened["documentId"],
                "handles": [layer_handle, "missing-handle"],
                "projection": "full",
            },
        )
        self.assertEqual(fetched["items"][0]["handle"], layer_handle)
        self.assertEqual(fetched["items"][0]["properties"]["name"], "0")
        self.assertEqual(fetched["missingHandles"], ["missing-handle"])

        queried = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": opened["documentId"],
                "mode": "handles",
                "whereClauses": [
                    {
                        "property": "kind",
                        "op": "eq",
                        "value": "entity",
                    }
                ],
                "limit": 2,
            },
        )
        self.assertEqual(queried["total"], 3891)
        self.assertEqual(len(queried["handles"]), 2)
        self.assertEqual(queried["nextCursor"], "2")

        closed = await self.app.call_tool(
            "dwg.close_file",
            {"documentId": opened["documentId"]},
        )
        self.assertTrue(closed["closed"])

    async def test_open_file_failure_mentions_configured_access_folders(self) -> None:
        missing = str(repo_root() / "testData" / "missing.dwg")
        with patch.dict(
            os.environ,
            {"DWG_MCP_HOST_FOLDERS": str((repo_root() / "testData").resolve())},
            clear=False,
        ):
            with self.assertRaisesRegex(ValueError, "Configured access folders"):
                await self.app.call_tool("dwg.open_file", {"path": missing})
            with self.assertRaisesRegex(ValueError, "copy or move the DWG"):
                await self.app.call_tool("dwg.open_file", {"path": missing})
