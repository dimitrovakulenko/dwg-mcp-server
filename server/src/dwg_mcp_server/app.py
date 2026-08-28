from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any, Sequence

from mcp.server import NotificationOptions, Server
from mcp.server.models import InitializationOptions
from mcp.server.stdio import stdio_server
from mcp.types import CallToolResult, ImageContent, Tool, ToolAnnotations

from .file_access import (
    file_uri_to_path,
    resolve_root_relative_path,
)
from .worker_client import SessionManager, WorkerClientError

SERVER_INSTRUCTIONS = (
    "Use dwg.list_roots to discover roots available for DWG access. "
    "Open a DWG with dwg.open_file before using file-scoped tools. "
    "Use dwg.list_file_types to discover valid type names for that file. "
    "Use dwg.describe_type to discover properties marked writable before editing. "
    "open_file accepts only rootUri plus a path relative to that root."
)

READ_ONLY_TOOL = ToolAnnotations(readOnlyHint=True)
MUTATING_TOOL = ToolAnnotations(
    readOnlyHint=False,
    destructiveHint=True,
    idempotentHint=True,
    openWorldHint=False,
)


class DwgMcpApplication:
    def __init__(
        self,
        session_manager: SessionManager | None = None,
        *,
        allowed_roots: Sequence[str | Path] = (),
    ) -> None:
        self.session_manager = session_manager or SessionManager()
        self.allowed_roots = tuple(self._normalize_allowed_root(root) for root in allowed_roots)

        @asynccontextmanager
        async def lifespan(_: Server):
            try:
                yield None
            finally:
                await self.session_manager.close_all()

        self.server = Server(
            "dwg-mcp-server",
            version="0.2.1",
            instructions=SERVER_INSTRUCTIONS,
            lifespan=lifespan,
        )
        self._setup_handlers()

    def _setup_handlers(self) -> None:
        @self.server.list_tools()
        async def handle_list_tools() -> list[Tool]:
            return self.tool_definitions()

        @self.server.call_tool()
        async def handle_call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any] | CallToolResult:
            result = await self.call_tool(name, arguments)
            if name != "dwg.render_view":
                return result
            data = result.pop("data")
            return CallToolResult(
                content=[ImageContent(type="image", data=data, mimeType=result["mimeType"])],
                structuredContent=result,
            )

    def tool_definitions(self) -> list[Tool]:
        return [
            Tool(
                name="dwg.list_roots",
                description=(
                    "List roots that can be used with dwg.open_file. Uses MCP client roots when "
                    "the client provides them, otherwise uses server-configured allowed roots."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {},
                    "additionalProperties": False,
                },
                annotations=READ_ONLY_TOOL,
            ),
            Tool(
                name="dwg.open_file",
                description=(
                    "Open a DWG or DXF from a listed root and return documentId. "
                    "Call dwg.list_roots first, then provide one returned rootUri and a "
                    "relativePath under that root."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "rootUri": {
                            "type": "string",
                            "format": "uri",
                            "description": "A file:// root URI returned by dwg.list_roots.",
                        },
                        "relativePath": {
                            "type": "string",
                            "description": "Path to the DWG or DXF file relative to rootUri.",
                        },
                    },
                    "required": ["rootUri", "relativePath"],
                    "additionalProperties": False,
                },
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
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
                annotations=READ_ONLY_TOOL,
            ),
            Tool(
                name="dwg.set_entity_properties",
                description=(
                    "Set properties marked writable by dwg.describe_type on one existing entity. "
                    "The change affects only the opened in-memory document and is discarded on "
                    "close; this tool does not write the source file."
                ),
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {
                            "type": "string",
                            "description": "documentId returned by dwg.open_file.",
                        },
                        "handle": {
                            "type": "string",
                            "description": "Handle of the entity to update.",
                        },
                        "properties": {
                            "type": "object",
                            "minProperties": 1,
                            "description": (
                                "Property names and values accepted for the entity's type. "
                                "Discover writable properties with dwg.describe_type."
                            ),
                            "additionalProperties": True,
                        },
                        "projection": {
                            "type": "string",
                            "enum": ["summary", "full"],
                        },
                        "select": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Optional properties to return after the update.",
                        },
                    },
                    "required": ["documentId", "handle", "properties"],
                    "additionalProperties": False,
                },
                annotations=MUTATING_TOOL,
            ),
            Tool(
                name="dwg.list_render_views",
                description="List model space, paper-space layouts, and layout viewports that can be rendered.",
                inputSchema={
                    "type": "object",
                    "properties": {"documentId": {"type": "string"}},
                    "required": ["documentId"],
                    "additionalProperties": False,
                },
                annotations=READ_ONLY_TOOL,
            ),
            Tool(
                name="dwg.render_view",
                description="Render model space, a paper-space layout, or one layout viewport as PNG or SVG.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "documentId": {"type": "string"},
                        "target": {
                            "oneOf": [
                                {"type": "object", "properties": {"kind": {"const": "model"}}, "required": ["kind"], "additionalProperties": False},
                                {"type": "object", "properties": {"kind": {"const": "layout"}, "layoutHandle": {"type": "string"}}, "required": ["kind", "layoutHandle"], "additionalProperties": False},
                                {"type": "object", "properties": {"kind": {"const": "viewport"}, "viewportHandle": {"type": "string"}}, "required": ["kind", "viewportHandle"], "additionalProperties": False},
                            ]
                        },
                        "region": {
                            "type": "object",
                            "properties": {
                                "min": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
                                "max": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
                            },
                            "required": ["min", "max"],
                            "additionalProperties": False,
                        },
                        "width": {"type": "integer", "minimum": 1, "maximum": 4096, "default": 1600},
                        "height": {"type": "integer", "minimum": 1, "maximum": 4096, "default": 1200},
                        "format": {"type": "string", "enum": ["png", "svg"], "default": "png"},
                        "background": {"type": "string", "enum": ["model", "paper", "transparent", "white", "black"], "default": "paper"},
                        "padding": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.02},
                    },
                    "required": ["documentId", "target"],
                    "additionalProperties": False,
                },
                annotations=READ_ONLY_TOOL,
            ),
        ]

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        if name == "dwg.list_roots":
            return {"roots": await self._list_available_roots()}
        if name == "dwg.open_file":
            file_path, root = await self._resolve_open_file_path(arguments)
            try:
                opened = await self.session_manager.open_file(str(file_path))
            except WorkerClientError as error:
                raise ValueError(str(error)) from error
            return {
                "rootUri": root["uri"],
                "rootName": root.get("name"),
                "relativePath": arguments["relativePath"],
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
        if name == "dwg.set_entity_properties":
            return await self.session_manager.set_entity_properties(
                arguments["documentId"], arguments
            )
        if name == "dwg.list_render_views":
            return await self.session_manager.list_render_views(arguments["documentId"])
        if name == "dwg.render_view":
            return await self.session_manager.render_view(arguments["documentId"], arguments)
        raise ValueError(f"unknown tool: {name}")

    async def _resolve_open_file_path(self, arguments: dict[str, Any]) -> tuple[Path, dict[str, str | None]]:
        if set(arguments) != {"rootUri", "relativePath"}:
            raise ValueError("Provide exactly rootUri and relativePath from dwg.list_roots.")

        root_uri = arguments["rootUri"]
        relative_path = arguments["relativePath"]
        roots = await self._list_available_roots()
        root = next((candidate for candidate in roots if candidate["uri"] == root_uri), None)
        if root is None:
            raise ValueError("rootUri must exactly match a URI returned by dwg.list_roots")

        root_path = file_uri_to_path(root_uri)
        try:
            return resolve_root_relative_path(root_path, relative_path), root
        except OSError as error:
            raise ValueError(f"failed to resolve relativePath under MCP root: {error}") from error

    async def _list_client_roots(self) -> list[dict[str, str | None]]:
        try:
            request_context = self.server.request_context
        except LookupError:
            raise ValueError("MCP client roots are not available in this context")

        client_params = request_context.session.client_params
        if client_params is None or client_params.capabilities.roots is None:
            raise ValueError("MCP client roots are not advertised by this client")

        roots_result = await request_context.session.list_roots()
        roots: list[dict[str, str | None]] = []
        for root in roots_result.roots:
            root_uri = str(root.uri)
            file_uri_to_path(root_uri)
            roots.append(
                {
                    "uri": root_uri,
                    "name": root.name,
                }
            )
        return roots

    async def _list_available_roots(self) -> list[dict[str, str | None]]:
        configured_roots = self._list_configured_roots()
        try:
            client_roots = await self._list_client_roots()
        except Exception as error:
            if configured_roots:
                return configured_roots
            raise ValueError(
                "MCP client roots are required to open DWG files unless "
                "--allowed-root or DWG_MCP_ALLOWED_ROOTS is configured"
            ) from error

        return self._merge_roots([*client_roots, *configured_roots])

    def _list_configured_roots(self) -> list[dict[str, str | None]]:
        return [
            {
                "uri": root.as_uri(),
                "name": root.name or str(root),
            }
            for root in self.allowed_roots
        ]

    @staticmethod
    def _merge_roots(roots: list[dict[str, str | None]]) -> list[dict[str, str | None]]:
        merged: list[dict[str, str | None]] = []
        seen: set[str] = set()
        for root in roots:
            root_uri = root["uri"]
            if root_uri in seen:
                continue
            seen.add(root_uri)
            merged.append(root)
        return merged

    @staticmethod
    def _normalize_allowed_root(root: str | Path) -> Path:
        root_path = Path(root).expanduser()
        if not root_path.is_absolute():
            raise ValueError("allowed roots must be absolute local paths")

        resolved = root_path.resolve(strict=True)
        if not resolved.is_dir():
            raise ValueError("allowed roots must point to existing directories")
        return resolved

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
