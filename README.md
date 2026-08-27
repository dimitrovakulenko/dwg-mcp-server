# DWG MCP Server

DWG and DXF file format access for AI via MCP.

Connect DWG and DXF files to Claude, ChatGPT, Codex, Cursor, and other MCP clients.
DWG MCP Server provides structured access to DWG data so AI can understand and
reason about drawings. Source files remain read-only; supported entity
properties can be changed in memory for inspection and rendering.

## Quick Start

Use DWG MCP Server from the MCP client of your choice.
The npm package downloads the matching native build from GitHub Releases on
first use, verifies its SHA-256 checksum, caches it, and runs it directly.
Docker and a separate Python installation are not required.

If your MCP client, AI agent, or test harness does not support MCP roots, set
`DWG_MCP_ALLOWED_ROOTS` to the folders that should be accessible.

### Codex

```bash
codex mcp add dwg-mcp \
  --env DWG_MCP_ALLOWED_ROOTS="$HOME" \
  -- npx -y @dmytro-prototypes/dwg-mcp-server
```

Codex does not currently provide MCP client roots to stdio MCP servers. Configure
explicit allowed roots for the folders that contain your drawings.

### Claude

```bash
claude mcp add --scope user --transport stdio dwg-mcp -- npx -y @dmytro-prototypes/dwg-mcp-server
```

Claude Code provides MCP roots, so no extra path variable is usually needed.

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

## Exposed Tools

| Tool | Purpose |
| --- | --- |
| `dwg.list_roots` | List folders available for DWG access. |
| `dwg.open_file` | Open a DWG or DXF from an available folder and return a `documentId`. |
| `dwg.close_file` | Close an opened document and release its worker process. |
| `dwg.list_types` | List the globally supported DWG types known to the backend. |
| `dwg.list_file_types` | List only the types that are present in a specific opened DWG. |
| `dwg.describe_type` | Describe a supported type, including readable and writable properties and its default projection. |
| `dwg.get_objects` | Fetch specific objects by handle, preserving the requested order and reporting missing handles. |
| `dwg.query_objects` | Query objects with filters, scopes, relation traversal, sorting, projection, and pagination. |
| `dwg.set_entity_properties` | Change properties marked writable on one entity in memory. Changes are discarded when the document closes. |
| `dwg.list_render_views` | List renderable model space, paper-space layouts, and layout viewports. |
| `dwg.render_view` | Render a complete view or selected drawing region as PNG or SVG. |

A typical flow is:

1. Discover available folders with `dwg.list_roots`.
2. Open a file from one of those folders with `dwg.open_file`.
3. Inspect supported or file-local types with `dwg.list_types`, `dwg.list_file_types`, or `dwg.describe_type`.
4. Fetch known handles with `dwg.get_objects` or search the drawing with `dwg.query_objects`.
5. Optionally change properties reported as writable with `dwg.set_entity_properties`.
6. Discover and render drawing views with `dwg.list_render_views` and `dwg.render_view`.
7. Close the session with `dwg.close_file`, discarding in-memory changes.

## Architecture

### Runtime model

In the packaged deployment, DWG MCP Server is a standalone native application
built for the host operating system.
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

`dwg.set_entity_properties` validates writes against the type catalog, updates
the native LibreDWG document, rebuilds the index, and invalidates the cached
rendering scene. Block-reference insertion points use OCS coordinates, block
scaling rules are enforced, and attributed block references are rejected until
their owned attributes can be transformed safely. The tool does not save or
overwrite the source file.

The worker lazily compiles a separate rendering scene on the first render call.
It expands block references and generated dimension blocks, draws common 2D
geometry, text, MTEXT, hatch contours, and approximated point-list entities,
and composes paper-space layouts through their viewports. SVG is the canonical
render output; PNG is rasterized from the same SVG in memory. Render responses
include coverage diagnostics for generated-block fallbacks and unsupported
entity types. Automatic fit rejects extreme sparse extents, WIPEOUT boundaries
mask earlier geometry, and explicit regions allow bounded rendering of dense
views. See [`docs/rendering.md`](docs/rendering.md) for the protocol and
rendering model.

### Access and packaging

DWG and DXF files must be opened from roots listed by `dwg.list_roots`. The server first
asks the MCP client for roots. If the client, AI agent, or test harness does not
support MCP roots, configure explicit allowed roots:

```bash
python3 -m dwg_mcp_server --allowed-root "$HOME/Downloads"
```

or:

```bash
DWG_MCP_ALLOWED_ROOTS="$HOME/Downloads;$HOME/Documents/dwg" \
python3 -m dwg_mcp_server
```

`DWG_MCP_ALLOWED_ROOTS` is an authorization fallback for clients without roots.
It should be a semicolon-separated list of absolute directories.

Official releases contain a standalone MCP host, the statically linked Rust
worker, and the LibreDWG schema files needed at runtime.

### Native platforms

GitHub Actions builds and tests these release artifacts on their native runners:

| System | CPU | Rust target |
| --- | --- | --- |
| macOS | Apple Silicon | `aarch64-apple-darwin` |
| Windows | Intel or AMD 64-bit | `x86_64-pc-windows-gnu` |
| Linux | Intel or AMD 64-bit | `x86_64-unknown-linux-gnu` |
| Linux | ARM 64-bit | `aarch64-unknown-linux-gnu` |

`x64`, `x86_64`, and `AMD64` name the same CPU architecture; the Windows and
Linux artifacts work on both Intel and AMD processors.

## Build and Test From Source

Local source builds use the vendored `third_party/libredwg` submodule by default.

### Prerequisites

- Rust toolchain
- Python 3.11 or newer
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

### Publish a native release

1. Set the same version in `npm/package.json`, `server.json`, and the Claude
   extension manifest when applicable.
2. Configure npm trusted publishing for `.github/workflows/native-release.yml`.
3. Push the matching `v<version>` tag.

The `Native Release` workflow builds and tests all four platforms, creates the
GitHub Release with checksum files, and then publishes the npm launcher. A
manual workflow run can build one selected platform or all four as downloadable
Actions artifacts without publishing.

### Clean rebuild

Remove local Rust and Python build artifacts:

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
