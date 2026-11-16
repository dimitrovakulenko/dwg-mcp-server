# Claude Desktop extension

Install the `.mcpb` matching your computer:

| Computer | Target |
| --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x64 | `x86_64-pc-windows-gnu` |
| Linux x64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |

1. Open Claude Desktop → Settings → Extensions → Advanced settings → Install Extension.
2. Select the `.mcpb` file.
3. Choose your drawings folder and enable the extension.
4. Ask Claude to list the available drawing roots and inspect a DWG or DXF.

The server and its dependencies are included. Python, Node.js, Docker, and
network downloads are not required to run it. Source drawings remain unchanged.
The selected folder is added to any MCP roots provided by Claude.

The server reads drawings only. If you report a problem with `dwg.report_feedback`, the report is appended to `~/.dwg-mcp/feedback.jsonl` on your own computer; nothing is sent anywhere.

## Build

Extract a native release, then run from the repository root:

```bash
python3 scripts/pack-desktop.py /path/to/dwg-mcp-server --target aarch64-apple-darwin
```

This creates both the Claude `.mcpb` and ChatGPT plugin ZIP in
`target/desktop-packages/`. Versions come from `npm/package.json`; use the matching
native release. Pass the corresponding `--target` from the table with each
platform's native bundle. The Native Release workflow builds and packages all
five targets. The Pack Desktop Extensions workflow repackages an existing GitHub
release; select `all` or one target. Intel macOS requires a release containing
the new Intel native build; older releases such as `0.2.3` have no Intel asset.

Format: [MCPB](https://github.com/modelcontextprotocol/mcpb).
