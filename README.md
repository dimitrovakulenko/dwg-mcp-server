# DWG MCP Server

DWG MCP Server is an MCP server for inspecting AutoCAD DWG files with AI agents and assistants.
It provides read-only access to drawing contents as queryable objects.
Agents can open a DWG, inspect available types, fetch objects by handle, and query properties, scopes, and references.

## Quick Start

Use DWG MCP Server from the MCP client of your choice.
The npm package launches the published Docker image.

### Codex

```bash
codex mcp add dwg-mcp -- npx -y @dmytro-prototypes/dwg-mcp-server
```

### Claude

```bash
claude mcp add --scope user --transport stdio --env DWG_MCP_HOST_FOLDERS=/absolute/path/to/your/dwg/folder dwg-mcp -- npx -y @dmytro-prototypes/dwg-mcp-server
```

### Cursor

```json
{
  "mcpServers": {
    "dwg-mcp": {
      "command": "npx",
      "args": ["-y", "@dmytro-prototypes/dwg-mcp-server"],
      "env": {
        "DWG_MCP_HOST_FOLDERS": "${workspaceFolder}"
      }
    }
  }
}
```

DWG files must be opened from allowed folders only.
If you run the Docker wrapper, configure those folders with `DWG_MCP_HOST_FOLDERS`.

## Exposed Tools

| Tool | Purpose |
| --- | --- |
| `dwg.open_file` | Open a DWG from an absolute path or `file://` URI and return a `documentId`. |
| `dwg.close_file` | Close an opened document and release its worker process. |
| `dwg.list_types` | List the globally supported DWG types known to the backend. |
| `dwg.list_file_types` | List only the types that are present in a specific opened DWG. |
| `dwg.describe_type` | Describe a supported type, including its properties and default projection. |
| `dwg.get_objects` | Fetch specific objects by handle, preserving the requested order and reporting missing handles. |
| `dwg.query_objects` | Query objects with filters, scopes, relation traversal, sorting, projection, and pagination. |

A typical flow is:

1. Open a file with `dwg.open_file`.
2. Inspect supported or file-local types with `dwg.list_types`, `dwg.list_file_types`, or `dwg.describe_type`.
3. Fetch known handles with `dwg.get_objects` or search the drawing with `dwg.query_objects`.
4. Close the session with `dwg.close_file`.

## How It Works

### Python host and worker lifecycle

The Python MCP host is intentionally thin.
It exposes the MCP tool surface, validates file access, starts worker processes, and keeps track of open document sessions.

Each `dwg.open_file` call creates a dedicated `dwg-worker` process and assigns it a host-side `documentId`.
That document id is what MCP clients use for later calls such as `dwg.list_file_types`, `dwg.get_objects`, and `dwg.query_objects`.
Closing the document terminates the worker for that session.

This means open documents are isolated from each other at the process level.
The host does not share mutable drawing state across sessions.

### Worker protocol and document model

The Rust worker speaks newline-delimited JSON over stdin and stdout.
Its request methods map directly to the MCP tool surface:
`openFile`, `closeFile`, `listTypes`, `listFileTypes`, `describeType`, `getObjects`, and `queryObjects`.

When a file is opened, the LibreDWG-backed backend reads the DWG and builds an in-memory indexed document.
Each indexed object keeps:

- its handle
- kind, type name, and generic type
- summary properties and full properties
- container block membership
- derived layout membership
- derived model-space or paper-space membership when available

Supported type metadata is also captured.
For each type, the backend exposes aliases, default projected properties, and property definitions, including whether a property is queryable and whether it points at another object type.

### Query engine and indexing

The worker does not execute every query by scanning the full drawing from scratch.
When a DWG is opened, it builds indices for:

- handle
- type name
- generic type
- object kind
- exact property values
- container block
- layout
- model space and paper space

`dwg.query_objects` uses those indices to reduce the candidate set before applying the remaining work.
It can combine:

- exact and range filters over properties
- scope filters by block, layout, owner, or space
- relation filters in outgoing or incoming direction
- sort order
- pagination
- summary or full projections

Relation filters matter because many useful DWG questions are graph questions.
For example, a block reference points to a block definition, dictionaries point to contained records, and objects may carry handles that refer to related objects elsewhere in the file.
The query engine can follow those handle-valued properties in either direction and then filter on the related objects.

`dwg.get_objects` is the direct lookup path.
It is useful when the client already knows the handles it wants and needs exact records back in the same order.
Missing handles are returned separately instead of failing the whole request.

### File access rules

The server accepts absolute local paths or `file://` URIs.
It does not open arbitrary paths by default.

If the MCP client provides roots, the requested DWG must stay inside those roots.
If `DWG_MCP_HOST_FOLDERS` is configured, the requested DWG must also stay inside one of those allowed folders.
The host returns the configured folders in the error message when access is denied.

The Docker wrapper keeps the same model.
It mounts the configured host folders into the container read-only and passes the allowed folder list into the server through `DWG_MCP_HOST_FOLDERS`.

### Docker packaging

The Docker image is built in three stages:

1. Build LibreDWG from the vendored submodule.
2. Build the Rust `dwg-worker` binary against that LibreDWG build.
3. Assemble a slim Python runtime image with the MCP host, the worker binary, and the required native runtime pieces.

This keeps the runtime image smaller than a full build environment while still shipping the native worker and the LibreDWG shared library it depends on.

## Build and Test From Source

Local source builds use the vendored `third_party/libredwg` submodule by default.

### Prerequisites

- Rust toolchain
- Python 3.11 or newer
- Docker, if you want to build or run the container image
- autotools for local LibreDWG builds on macOS or Linux (`autoreconf`, `aclocal`, `automake`, `autoconf`, `make`)

### Bootstrap

```bash
git submodule update --init --recursive
bash scripts/build-libredwg.sh
```

### Build and test

```bash
cargo test --workspace
bash scripts/run-e2e-tests.sh
```

### Run the MCP host locally

The Python host looks for `dwg-worker` under `target/release` or `target/debug`.
If you want a release build explicitly:

```bash
cargo build -p dwg-worker --release
```

Then run the MCP host:

```bash
PYTHONPATH=server/src python3 -m dwg_mcp_server
```

If the worker binary lives somewhere else, set `DWG_WORKER_BIN` to that executable.

### Build and run with Docker

Build the image:

```bash
bash scripts/build-docker-mcp-server.sh
```

Run the server and expose specific host folders read-only:

```bash
DWG_MCP_HOST_FOLDERS="$HOME/Documents;$HOME/Desktop/dwg" \
bash scripts/run-docker-mcp-server.sh
```

By default, the Docker launcher exposes `~/Documents`.

### Clean rebuild

Remove local build artifacts, Python caches, and the local Docker image:

```bash
bash scripts/clean-build-artifacts.sh
```

To also wipe the host LibreDWG build under `third_party/libredwg`:

```bash
bash scripts/clean-build-artifacts.sh --with-libredwg
```

## License

This project is licensed under the GNU General Public License v3.0.
See `LICENSE` for the full license text.
