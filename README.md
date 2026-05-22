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
  --env DWG_MCP_ALLOWED_ROOTS="$HOME/Downloads" \
  -- npx -y @dmytro-prototypes/dwg-mcp-server
```

Codex does not currently provide MCP client roots to stdio MCP servers. Configure
explicit allowed roots for the folders that contain your drawings.

### Claude

```bash
claude mcp add --scope user --transport stdio dwg-mcp -- npx -y @dmytro-prototypes/dwg-mcp-server
```

### Cursor

```json
{
  "mcpServers": {
    "dwg-mcp": {
      "command": "npx",
      "args": ["-y", "@dmytro-prototypes/dwg-mcp-server"]
    }
  }
}
```

DWG files must be opened from roots listed by `dwg.list_roots`. The server first
asks the MCP client for `roots/list`. If the client does not support MCP roots,
configure explicit allowed roots with `--allowed-root` or
`DWG_MCP_ALLOWED_ROOTS`.

## Exposed Tools

| Tool | Purpose |
| --- | --- |
| `dwg.list_roots` | List MCP client roots that can be used with `dwg.open_file`. |
| `dwg.open_file` | Open a DWG from `rootUri` plus `relativePath` and return a `documentId`. |
| `dwg.close_file` | Close an opened document and release its worker process. |
| `dwg.list_types` | List the globally supported DWG types known to the backend. |
| `dwg.list_file_types` | List only the types that are present in a specific opened DWG. |
| `dwg.describe_type` | Describe a supported type, including its properties and default projection. |
| `dwg.get_objects` | Fetch specific objects by handle, preserving the requested order and reporting missing handles. |
| `dwg.query_objects` | Query objects with filters, scopes, relation traversal, sorting, projection, and pagination. |

A typical flow is:

1. Discover available roots with `dwg.list_roots`.
2. Open a file with `dwg.open_file`, passing a returned `rootUri` and a DWG path relative to that root.
3. Inspect supported or file-local types with `dwg.list_types`, `dwg.list_file_types`, or `dwg.describe_type`.
4. Fetch known handles with `dwg.get_objects` or search the drawing with `dwg.query_objects`.
5. Close the session with `dwg.close_file`.

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

When you request full object records, responses also include that derived membership under `extendedData`, including container block, layout, and model or paper space when known.

`dwg.get_objects` is direct lookup by handle.
`dwg.query_objects` runs over indices for handle, type, generic type, kind, exact property values, block, layout, and space, then applies filters, scopes, relation traversal, sorting, projection, and pagination.
This is what makes queries over blocks, layers, layouts, references, and related objects practical on an opened drawing.

### Access and packaging

The server opens files only through roots returned by `dwg.list_roots`.
`dwg.open_file` accepts a `rootUri` returned by `dwg.list_roots` and a
`relativePath` under that root. Absolute host paths and arbitrary `file://` file
URIs are not accepted by the MCP tool API.

Roots come from the MCP client when it supports `roots/list`. For clients that do
not support MCP roots, configure explicit allowed roots:

```bash
python3 -m dwg_mcp_server --allowed-root "$HOME/Downloads"
```

or:

```bash
DWG_MCP_ALLOWED_ROOTS="$HOME/Downloads;$HOME/Documents/dwg" \
python3 -m dwg_mcp_server
```

The Docker wrapper can mount host folders into the container read-only at their
original absolute paths so client root URIs still resolve inside the container.
The Docker image itself unpacks a prebuilt LibreDWG bundle, builds the Rust
worker against it, and copies only the static-linked worker plus schema files
into the final Python runtime image.

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

### Prebuilt LibreDWG bundles

CI and Docker builds use a Git LFS LibreDWG bundle instead of rebuilding
LibreDWG on every run. The current CI bundle target is:

```text
prebuilt/libredwg/x86_64-unknown-linux-gnu.tar.gz
```

Refresh it only when the LibreDWG submodule or native build inputs change:

```bash
bash scripts/build-linux-libredwg-prebuilt.sh x86_64-unknown-linux-gnu
```

For local macOS debugging, keep using `bash scripts/build-libredwg.sh` and the
local Rust build; the Linux prebuilt archive is for CI and Docker.

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
DWG_MCP_DOCKER_MOUNTS="$HOME/Documents;$HOME/Desktop/dwg" \
bash scripts/run-docker-mcp-server.sh
```

For clients without MCP roots, set `DWG_MCP_ALLOWED_ROOTS` instead:

```bash
DWG_MCP_ALLOWED_ROOTS="$HOME/Documents;$HOME/Desktop/dwg" \
bash scripts/run-docker-mcp-server.sh
```

By default, the Docker launcher mounts `DWG_MCP_ALLOWED_ROOTS`, then
`~/Documents` when no roots are configured. Access is still authorized by MCP
client roots or explicit allowed roots; Docker mounts only make those paths
visible inside the container.

### Clean rebuild

Remove local build artifacts, Python caches, and the local Docker image:

```bash
bash scripts/clean-build-artifacts.sh
```

To also wipe the host LibreDWG build under `third_party/libredwg`:

```bash
bash scripts/clean-build-artifacts.sh --with-libredwg
```

## Official MCP Registry

Registry metadata lives in `server.json` under the GitHub-authenticated name
`io.github.dimitrovakulenko/dwg-mcp-server`.

Before publishing a new registry version:

1. Publish the matching npm package version from `npm/`.
2. Confirm `npm/package.json` contains the same `mcpName` as `server.json`.
3. Run the manual `Publish MCP Registry` GitHub Actions workflow, or run:

```bash
mcp-publisher login github
mcp-publisher publish
```

The registry validates the published npm package, so `server.json` must point to
an npm version that already exists on the public npm registry.

## License

This project is licensed under the GNU General Public License v3.0.
See `LICENSE` for the full license text.
