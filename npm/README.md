# DWG MCP Server

DWG file format access for AI via MCP.

Connect DWG files to Claude, ChatGPT, Codex, Cursor, and other MCP clients.
DWG MCP Server provides read-only, structured access to DWG data.

## Run

This package downloads the matching checksum-verified native build from GitHub
Releases on first use, caches it locally, and runs it directly. Docker and a
separate Python installation are not required.

```bash
npx -y @dmytro-prototypes/dwg-mcp-server
```

Supported platforms are macOS Apple Silicon, Windows x64/AMD64, Linux
x64/AMD64, and Linux ARM64.

See the [full setup guide](https://github.com/dimitrovakulenko/dwg-mcp-server#quick-start)
for client configuration and file-access options.
