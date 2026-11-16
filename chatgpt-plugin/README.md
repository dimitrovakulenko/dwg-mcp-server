# ChatGPT desktop plugin

Use the ZIP matching your computer:

| Computer | Target |
| --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x64 | `x86_64-pc-windows-gnu` |
| Linux x64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |

The native server and its dependencies are included. Run the plugin in a local
desktop chat. The server also works with Codex CLI and other clients supporting
local stdio MCP servers. ChatGPT's Linux desktop app is available in
[preview](https://learn.chatgpt.com/docs/linux/linux-app).

## Install locally

1. Extract the ZIP to a permanent location. It contains `dwg-mcp-server/`.
2. If the client does not provide MCP roots, edit the extracted `mcp.json` and
   set the server's `args` to `["--allowed-root", "/absolute/path/to/drawings"]`.
   On Windows, use a JSON path such as `"C:/Users/you/Documents/Drawings"`.
   Repeat `--allowed-root` for additional folders. With neither client roots nor
   explicitly allowed folders, file access is denied. Explicit folders are added
   to any client-provided roots.
3. Add the directory containing `dwg-mcp-server/` as a local plugin marketplace.
   Create `.agents/plugins/marketplace.json` inside that directory with:

```json
{
  "name": "dwg-local",
  "plugins": [
    {
      "name": "dwg-mcp-server",
      "source": { "source": "local", "path": "./dwg-mcp-server" },
      "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
      "category": "Productivity"
    }
  ]
}
```

4. Register that marketplace directory with `codex plugin marketplace add
   /absolute/path/to/marketplace`, then restart the desktop app.
5. Open Plugins, select the `dwg-local` source, and install DWG MCP Server.
6. Start a local chat, select the plugin, and ask it to list the drawing roots.

If your desktop version does not expose local marketplaces, check its support
for [local plugin installation](https://developers.openai.com/plugins/build/plugins#install-a-local-plugin-manually).

For building the ZIP, see `claude-extension/README.md` in the source repository.
Source drawings remain unchanged. No HTTPS endpoint is needed.

## Public distribution

Local MCP submissions to the OpenAI public directory currently require an
approved partner organization. Local packaging and testing do not publish the
plugin. See [desktop-only plugins](https://developers.openai.com/plugins/deploy/desktop-only).

The server reads drawings only. If you report a problem with `dwg.report_feedback`, the report is appended to `~/.dwg-mcp/feedback.jsonl` on your own computer; nothing is sent anywhere.
