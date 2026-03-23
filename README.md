# DWG MCP Server

DWG MCP Server is an MCP server for inspecting AutoCAD DWG files with AI agents and assistants.
It provides read-only access to drawing contents as queryable objects.
Agents can open a DWG, inspect available types, fetch objects by handle, and query properties, scopes, and references.

## Quick Start

Use DWG MCP Server from the MCP client of your choice.
The npm package launches the published Docker image.

### Codex

```bash
codex mcp add dwg-mcp \
  --env DWG_MCP_HOST_FOLDERS=/absolute/path/to/your/dwg/folder \
  -- npx -y @dmytro-prototypes/dwg-mcp-server
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
Set `DWG_MCP_HOST_FOLDERS` to one or more absolute host folders separated by `;`.

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

## Architecture

### Runtime model

In the packaged deployment, DWG MCP Server is a stdio MCP server implemented in Python and typically run inside a Linux container.
The Python host exposes the MCP tools, validates file access, and manages document sessions.

Each `dwg.open_file` call starts a dedicated Rust `dwg-worker` process for that DWG and returns a host-side `documentId`.
All later file-scoped calls use that document id.
`dwg.close_file` terminates the worker for that session.

### Worker and query model

The Rust worker speaks newline-delimited JSON over stdin and stdout.
When it opens a DWG through LibreDWG, it first builds an in-memory indexed document.
That upfront indexing step is central to the design: the server pays the cost once when the file is opened, then answers later requests against the index instead of rescanning the DWG each time.

The indexed model stores object handles, kinds, type names, generic types, summary and full properties, and derived block, layout, and space membership.
It also stores supported type metadata such as aliases, default projections, and property definitions.

`dwg.get_objects` is direct lookup by handle.
`dwg.query_objects` runs over indices for handle, type, generic type, kind, exact property values, block, layout, and space, then applies filters, scopes, relation traversal, sorting, projection, and pagination.
This is what makes queries over blocks, layers, layouts, references, and related objects practical on an opened drawing.

### Access and packaging

The server accepts absolute local paths or `file://` URIs only.
If the MCP client exposes roots, opened DWGs must stay inside those roots.
If `DWG_MCP_HOST_FOLDERS` is configured, opened DWGs must also stay inside one of those allowed folders.

The Docker wrapper mounts those host folders into the container read-only and forwards the same folder list to the Python host.
The Docker image itself is built in three stages: LibreDWG, the Rust worker, and the final Python runtime image.

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

By default, the Docker launcher exposes the current working directory.

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
