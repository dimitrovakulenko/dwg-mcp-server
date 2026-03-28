from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any

from mcp.server import NotificationOptions, Server
from mcp.server.models import InitializationOptions
from mcp.server.stdio import stdio_server
from mcp.types import Tool

from .file_access import (
    configured_access_folders,
    ensure_within_roots,
    file_uri_to_path,
    format_access_folders,
    normalize_local_path,
)
from .worker_client import SessionManager, WorkerClientError

SERVER_INSTRUCTIONS = (
    "Open a DWG with dwg.open_file before using file-scoped tools. "
    "Use dwg.list_file_types to discover valid type names for that file. "
    "open_file paths must be inside client roots and configured access folders."
)


class DwgMcpApplication:
    def __init__(self, session_manager: SessionManager | None = None) -> None:
        self.session_manager = session_manager or SessionManager()

        @asynccontextmanager
        async def lifespan(_: Server):
            try:
                yield None
            finally:
                await self.session_manager.close_all()

        self.server = Server(
            "dwg-mcp-server",
            version="0.1.0",
            instructions=SERVER_INSTRUCTIONS,
            lifespan=lifespan,
        )
        self._setup_handlers()

    def _setup_handlers(self) -> None:
        @self.server.list_tools()
        async def handle_list_tools() -> list[Tool]:
            return self.tool_definitions()

        @self.server.call_tool()
        async def handle_call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
            return await self.call_tool(name, arguments)

    def tool_definitions(self) -> list[Tool]:
        return [
            Tool(
                name="dwg.open_file",
                description=(
                    "Open a DWG and return documentId. Accepts an absolute path or file:// URI "
                    "within allowed roots/folders."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute local path to a DWG file.",
                        },
                        "fileUri": {
                            "type": "string",
                            "format": "uri",
                            "description": "file:// URI to a local DWG file.",
                        },
                    },
                    "oneOf": [
                        {"required": ["path"]},
                        {"required": ["fileUri"]},
                    ],
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.close_file",
                description="Close a previously opened document and release worker resources.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {
                            "type": "string",
                            "description": "documentId returned by dwg.open_file.",
                        }
                    },
                    "required": ["documentId"],
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.list_types",
                description=(
                    "List globally supported DWG types (not file-specific). Supports regex "
                    "filtering and cursor pagination."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "regex": {
                            "type": "string",
                            "description": "Optional regex over typeName, genericType, or aliases.",
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of items to return.",
                            "default": 100,
                        },
                        "cursor": {
                            "type": "string",
                            "description": "Opaque cursor from a previous response.",
                        },
                    },
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.list_file_types",
                description=(
                    "List types present in an opened DWG. Use this after open_file to discover "
                    "valid typeName values."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {
                            "type": "string",
                            "description": "documentId returned by dwg.open_file.",
                        },
                        "regex": {
                            "type": "string",
                            "description": "Optional regex over typeName, genericType, or aliases.",
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of items to return.",
                            "default": 100,
                        },
                        "cursor": {
                            "type": "string",
                            "description": "Opaque cursor from a previous response.",
                        },
                    },
                    "required": ["documentId"],
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.describe_type",
                description=(
                    "Describe a supported DWG type, including aliases, properties, and default "
                    "select fields."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "typeName": {
                            "type": "string",
                            "description": "Canonical type name or alias.",
                        }
                    },
                    "required": ["typeName"],
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.get_objects",
                description=(
                    "Fetch objects by handle from an opened DWG. Preserves input order and "
                    "reports missing handles."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {
                            "type": "string",
                            "description": "documentId returned by dwg.open_file.",
                        },
                        "handles": {
                            "type": "array",
                            "items": {"type": "string"},
                            "minItems": 1,
                            "description": "Object handles to fetch.",
                        },
                        "projection": {
                            "type": "string",
                            "enum": ["summary", "full"],
                        },
                        "select": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional property names to include.",
                        },
                    },
                    "required": ["documentId", "handles"],
                    "additionalProperties": False,
                },
            ),
            Tool(
                name="dwg.query_objects",
                description=(
                    "Query objects in an opened DWG using filters, scope, relations, sorting, "
                    "and pagination."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {
                            "type": "string",
                            "description": "documentId returned by dwg.open_file.",
                        },
                        "typeName": {"type": "string"},
                        "genericType": {"type": "string"},
                        "whereClauses": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "property": {"type": "string"},
                                    "op": {
                                        "type": "string",
                                        "enum": ["eq", "in", "contains", "gt", "gte", "lt", "lte"],
                                    },
                                    "value": {},
                                    "values": {"type": "array"},
                                },
                                "required": ["property", "op"],
                                "additionalProperties": False,
                            },
                        },
                        "scope": {
                            "type": "object",
                            "properties": {
                                "space": {
                                    "type": "string",
                                    "enum": ["modelSpace", "paperSpace"],
                                },
                                "layoutHandle": {"type": "string"},
                                "blockHandle": {"type": "string"},
                                "ownerHandle": {"type": "string"},
                            },
                            "additionalProperties": False,
                        },
                        "relations": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "property": {"type": "string"},
                                    "direction": {
                                        "type": "string",
                                        "enum": ["outgoing", "incoming"],
                                    },
                                    "targetTypeName": {"type": "string"},
                                    "targetGenericType": {"type": "string"},
                                    "whereClauses": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "property": {"type": "string"},
                                                "op": {
                                                    "type": "string",
                                                    "enum": ["eq", "in", "contains", "gt", "gte", "lt", "lte"],
                                                },
                                                "value": {},
                                                "values": {"type": "array"},
                                            },
                                            "required": ["property", "op"],
                                            "additionalProperties": False,
                                        },
                                    },
                                },
                                "required": ["property"],
                                "additionalProperties": False,
                            },
                        },
                        "sort": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "property": {"type": "string"},
                                    "direction": {
                                        "type": "string",
                                        "enum": ["asc", "desc"],
                                    },
                                },
                                "required": ["property"],
                                "additionalProperties": False,
                            },
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["count", "handles", "summary", "full"],
                        },
                        "projection": {
                            "type": "string",
                            "enum": ["summary", "full"],
                        },
                        "select": {
                            "type": "array",
                            "items": {"type": "string"},
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "default": 100,
                        },
                        "cursor": {"type": "string"},
                    },
                    "required": ["documentId"],
                    "additionalProperties": False,
                },
            ),
        ]

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        if name == "dwg.open_file":
            file_path = await self._resolve_open_file_path(arguments)
            try:
                opened = await self.session_manager.open_file(str(file_path))
            except WorkerClientError as error:
                raise ValueError(str(error)) from error
            return {
                "path": str(file_path),
                "fileUri": file_path.as_uri(),
                **opened,
            }
        if name == "dwg.close_file":
            return await self.session_manager.close_file(arguments["documentId"])
        if name == "dwg.list_types":
            return await self.session_manager.list_types(
                regex=arguments.get("regex"),
                limit=arguments.get("limit"),
                cursor=arguments.get("cursor"),
            )
        if name == "dwg.list_file_types":
            return await self.session_manager.list_file_types(
                arguments["documentId"],
                regex=arguments.get("regex"),
                limit=arguments.get("limit"),
                cursor=arguments.get("cursor"),
            )
        if name == "dwg.describe_type":
            return await self.session_manager.describe_type(arguments["typeName"])
        if name == "dwg.get_objects":
            return await self.session_manager.get_objects(
                arguments["documentId"],
                handles=arguments["handles"],
                projection=arguments.get("projection"),
                select=arguments.get("select"),
            )
        if name == "dwg.query_objects":
            return await self.session_manager.query_objects(arguments["documentId"], arguments)
        raise ValueError(f"unknown tool: {name}")

    async def _resolve_open_file_path(self, arguments: dict[str, Any]) -> Path:
        path_text = arguments.get("path")
        file_uri = arguments.get("fileUri")

        if sum(x is not None for x in (path_text, file_uri)) != 1:
            raise ValueError("Provide exactly one of `path` or `fileUri`.")

        if path_text is not None:
            file_path = normalize_local_path(path_text)
        else:
            file_path = file_uri_to_path(file_uri)

        root_paths = await self._list_client_root_paths()
        if root_paths is not None:
            ensure_within_roots(file_path, root_paths)

        configured_folders = configured_access_folders()
        if configured_folders:
            try:
                ensure_within_roots(
                    file_path,
                    configured_folders,
                    boundary_name="configured access folders",
                )
            except ValueError as error:
                raise ValueError(self._with_access_folders(str(error))) from error
        return file_path

    async def _list_client_root_paths(self) -> list[Path] | None:
        try:
            request_context = self.server.request_context
        except LookupError:
            return None

        client_params = request_context.session.client_params
        if client_params is None or client_params.capabilities.roots is None:
            return None

        roots_result = await request_context.session.list_roots()
        return [file_uri_to_path(str(root.uri)) for root in roots_result.roots]

    async def run_stdio(self) -> None:
        async with stdio_server() as (read_stream, write_stream):
            await self.server.run(
                read_stream,
                write_stream,
                self._initialization_options(),
            )

    def _initialization_options(self) -> InitializationOptions:
        return InitializationOptions(
            server_name="dwg-mcp-server",
            server_version="0.1.0",
            capabilities=self.server.get_capabilities(NotificationOptions(), {}),
            instructions=SERVER_INSTRUCTIONS,
        )

    def _with_access_folders(self, message: str) -> str:
        folders = configured_access_folders()
        if not folders:
            return message
        return (
            f"{message}\n"
            f"Allowed folders: {format_access_folders(folders)}\n"
            "Copy the DWG there, or restart with DWG_MCP_HOST_FOLDERS including its folder."
        )
