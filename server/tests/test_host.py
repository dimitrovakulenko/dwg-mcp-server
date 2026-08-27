from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import patch

from dwg_mcp_server.app import DwgMcpApplication
from dwg_mcp_server.worker_client import SessionManager, UnknownDocumentError


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def house_plan() -> str:
    return str(repo_root() / "testData" / "house_plan.dwg")


def test_data_root_uri() -> str:
    return (repo_root() / "testData").resolve().as_uri()


def house_plan_open_args() -> dict[str, str]:
    return {
        "rootUri": test_data_root_uri(),
        "relativePath": "house_plan.dwg",
    }


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

        async def list_client_roots() -> list[dict[str, str]]:
            return [{"uri": test_data_root_uri(), "name": "testData"}]

        self.app._list_client_roots = list_client_roots  # type: ignore[method-assign]

    async def asyncTearDown(self) -> None:
        await self.manager.close_all()

    async def test_tool_catalog_and_tool_calls(self) -> None:
        tool_names = [tool.name for tool in self.app.tool_definitions()]
        self.assertEqual(
            tool_names,
            [
                "dwg.list_roots",
                "dwg.open_file",
                "dwg.close_file",
                "dwg.list_types",
                "dwg.list_file_types",
                "dwg.describe_type",
                "dwg.get_objects",
                "dwg.query_objects",
                "dwg.set_entity_properties",
                "dwg.list_render_views",
                "dwg.render_view",
            ],
        )

        described = await self.app.call_tool(
            "dwg.describe_type",
            {"typeName": "AcDb3PointAngularDimension"},
        )
        self.assertEqual(described["typeName"], "AcDb3PointAngularDimension")
        property_names = {item["name"] for item in described["properties"]}
        self.assertIn("center_pt", property_names)

        block_reference_type = await self.app.call_tool(
            "dwg.describe_type", {"typeName": "AcDbBlockReference"}
        )
        writable = {
            item["name"]
            for item in block_reference_type["properties"]
            if item["writable"]
        }
        self.assertEqual(writable, {"ins_pt", "rotation", "scale"})

        roots = await self.app.call_tool("dwg.list_roots", {})
        self.assertEqual(roots["roots"][0]["uri"], test_data_root_uri())

        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        self.assertIn("documentId", opened)
        self.assertEqual(opened["rootUri"], test_data_root_uri())
        self.assertEqual(opened["relativePath"], "house_plan.dwg")

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

        updated = await self.app.call_tool(
            "dwg.set_entity_properties",
            {
                "documentId": opened["documentId"],
                "handle": "2AD",
                "properties": {
                    "ins_pt": [101.0, 202.0, 3.0],
                    "rotation": 1.25,
                    "scale": [-2.0, 3.0, 4.0],
                },
                "projection": "full",
                "select": ["ins_pt", "rotation", "scale"],
            },
        )
        self.assertTrue(updated["dirty"])
        self.assertEqual(updated["item"]["properties"]["ins_pt"], [101.0, 202.0, 3.0])
        self.assertEqual(updated["item"]["properties"]["rotation"], 1.25)
        self.assertEqual(updated["item"]["properties"]["scale"], [-2.0, 3.0, 4.0])

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

        views = await self.app.call_tool(
            "dwg.list_render_views",
            {"documentId": opened["documentId"]},
        )
        view_ids = {view["id"] for view in views["views"]}
        self.assertIn("model", view_ids)
        self.assertIn("layout:2F37", view_ids)
        self.assertIn("viewport:2F56", view_ids)

        rendered = await self.app.call_tool(
            "dwg.render_view",
            {
                "documentId": opened["documentId"],
                "target": {"kind": "layout", "layoutHandle": "2F37"},
                "width": 320,
                "height": 240,
                "format": "svg",
            },
        )
        self.assertEqual(rendered["mimeType"], "image/svg+xml")
        self.assertTrue(rendered["data"].startswith("PHN2Zy"))
        self.assertGreater(rendered["renderedEntities"], 1000)

        closed = await self.app.call_tool(
            "dwg.close_file",
            {"documentId": opened["documentId"]},
        )
        self.assertTrue(closed["closed"])

    async def test_header_settings_are_queryable(self) -> None:
        described = await self.app.call_tool(
            "dwg.describe_type",
            {"typeName": "HEADER"},
        )
        property_names = {item["name"] for item in described["properties"]}
        self.assertIn("DWGCODEPAGE", property_names)
        self.assertIn("HANDSEED", property_names)
        self.assertIn("MEASUREMENT", property_names)

        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        header = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": opened["documentId"],
                "typeName": "HEADER",
                "mode": "full",
                "limit": 1,
            },
        )

        self.assertEqual(header["total"], 1)
        self.assertEqual(header["items"][0]["handle"], "HEADER")
        self.assertEqual(header["items"][0]["kind"], "header")
        self.assertIn("HANDSEED", header["items"][0]["properties"])
        self.assertIn("CLAYER", header["items"][0]["properties"])
        self.assertIn("INSUNITS", header["items"][0]["properties"])

    async def test_get_objects_includes_insertion_points(self) -> None:
        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        document_id = opened["documentId"]

        block_reference = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": document_id,
                "handles": ["2AD"],
                "projection": "full",
                "select": ["ins_pt"],
            },
        )
        ins_pt = block_reference["items"][0]["properties"].get("ins_pt")
        self.assertIsNotNone(ins_pt)
        self.assertEqual(len(ins_pt), 3)

        text_handle_page = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": document_id,
                "typeName": "AcDbText",
                "mode": "handles",
                "limit": 1,
            },
        )
        self.assertTrue(text_handle_page["handles"])

        text = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": document_id,
                "handles": [text_handle_page["handles"][0]],
                "projection": "full",
                "select": ["ins_pt"],
            },
        )
        text_ins_pt = text["items"][0]["properties"].get("ins_pt")
        self.assertIsNotNone(text_ins_pt)
        self.assertEqual(len(text_ins_pt), 2)

    async def test_get_objects_includes_lwpolyline_points(self) -> None:
        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        document_id = opened["documentId"]

        polyline_handles = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": document_id,
                "typeName": "AcDbPolyline",
                "mode": "handles",
                "limit": 1,
            },
        )
        self.assertTrue(polyline_handles["handles"])

        polyline_without_select = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": document_id,
                "handles": [polyline_handles["handles"][0]],
                "projection": "full",
            },
        )
        self.assertNotIn("points", polyline_without_select["items"][0]["properties"])

        polyline = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": document_id,
                "handles": [polyline_handles["handles"][0]],
                "projection": "full",
                "select": ["num_points", "points"],
            },
        )
        properties = polyline["items"][0]["properties"]
        self.assertIn("points", properties)
        self.assertIn("num_points", properties)
        self.assertEqual(len(properties["points"]), properties["num_points"])
        self.assertTrue(all(len(point) == 2 for point in properties["points"]))

    async def test_full_object_queries_include_extended_data(self) -> None:
        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        document_id = opened["documentId"]

        queried = await self.app.call_tool(
            "dwg.query_objects",
            {
                "documentId": document_id,
                "typeName": "AcDbText",
                "scope": {"space": "modelSpace"},
                "mode": "full",
                "limit": 1,
            },
        )
        self.assertTrue(queried["items"])
        extended_data = queried["items"][0]["extendedData"]
        self.assertEqual(extended_data["space"], "modelSpace")
        self.assertIn("containerBlockHandle", extended_data)

        fetched = await self.app.call_tool(
            "dwg.get_objects",
            {
                "documentId": document_id,
                "handles": [queried["items"][0]["handle"]],
                "projection": "full",
            },
        )
        self.assertEqual(
            fetched["items"][0]["extendedData"]["space"],
            "modelSpace",
        )

    async def test_open_file_rejects_unknown_root_uri(self) -> None:
        with self.assertRaisesRegex(ValueError, "dwg.list_roots"):
            await self.app.call_tool(
                "dwg.open_file",
                {
                    "rootUri": (repo_root() / "server").resolve().as_uri(),
                    "relativePath": "house_plan.dwg",
                },
            )

    async def test_open_file_rejects_legacy_path_arguments(self) -> None:
        with self.assertRaisesRegex(ValueError, "rootUri"):
            await self.app.call_tool("dwg.open_file", {"path": house_plan()})

    async def test_open_file_explains_unmounted_docker_root(self) -> None:
        missing_root = Path("/tmp/dwg-missing-root").resolve()

        async def list_client_roots() -> list[dict[str, str]]:
            return [{"uri": missing_root.as_uri(), "name": "missing"}]

        self.app._list_client_roots = list_client_roots  # type: ignore[method-assign]
        with patch.dict(
            "os.environ",
            {
                "DWG_MCP_RUNNING_IN_DOCKER": "1",
                "DWG_MCP_DOCKER_MOUNTS": "/mounted",
            },
        ):
            with self.assertRaisesRegex(ValueError, "DWG_MCP_DOCKER_MOUNTS"):
                await self.app.call_tool(
                    "dwg.open_file",
                    {
                        "rootUri": missing_root.as_uri(),
                        "relativePath": "drawing.dwg",
                    },
                )

    async def test_configured_allowed_roots_are_added_to_client_roots(self) -> None:
        self.app.allowed_roots = ((repo_root() / "server").resolve(),)
        roots = await self.app.call_tool("dwg.list_roots", {})
        self.assertEqual(
            [root["uri"] for root in roots["roots"]],
            [
                test_data_root_uri(),
                (repo_root() / "server").resolve().as_uri(),
            ],
        )


class ApplicationAllowedRootTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.manager = SessionManager(worker_cwd=repo_root())
        self.app = DwgMcpApplication(
            session_manager=self.manager,
            allowed_roots=[repo_root() / "testData"],
        )

    async def asyncTearDown(self) -> None:
        await self.manager.close_all()

    async def test_configured_allowed_roots_work_without_client_roots(self) -> None:
        roots = await self.app.call_tool("dwg.list_roots", {})
        self.assertEqual(roots["roots"][0]["uri"], test_data_root_uri())
        self.assertEqual(roots["roots"][0]["name"], "testData")

        opened = await self.app.call_tool("dwg.open_file", house_plan_open_args())
        self.assertEqual(opened["rootUri"], test_data_root_uri())
        self.assertEqual(opened["relativePath"], "house_plan.dwg")

        closed = await self.app.call_tool(
            "dwg.close_file",
            {"documentId": opened["documentId"]},
        )
        self.assertTrue(closed["closed"])

    async def test_configured_allowed_roots_reject_unknown_root_uri(self) -> None:
        with self.assertRaisesRegex(ValueError, "dwg.list_roots"):
            await self.app.call_tool(
                "dwg.open_file",
                {
                    "rootUri": repo_root().resolve().as_uri(),
                    "relativePath": "README.md",
                },
            )

    def test_configured_allowed_roots_must_be_absolute(self) -> None:
        with self.assertRaisesRegex(ValueError, "absolute"):
            DwgMcpApplication(allowed_roots=["testData"])
