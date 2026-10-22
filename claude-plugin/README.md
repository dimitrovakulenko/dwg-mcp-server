# DWG MCP Server

Gives Claude read access to AutoCAD DWG and DXF drawings stored on your own
computer. Point the plugin at a folder of drawings and ask questions about
them in plain language: which layers and block definitions a drawing contains,
how many entities of a given type there are, the exact coordinates of a
polyline, or what a layout looks like as an image.

## What it provides

One local MCP server, `dwg-mcp`, with tools to open a drawing, list and
describe its object types, fetch and query objects with their full numeric
geometry, render model-space and paper-space views to PNG, and close the
drawing again. Drawing parsing is done by [LibreDWG](https://www.gnu.org/software/libredwg/).

## Access and your files

The plugin reads drawings only from the folder you choose when you enable it,
plus any roots your Claude client already provides. Edits made through
`dwg.set_entity_properties` apply to the in-memory copy of the drawing and are
discarded when it closes, so your source files are never modified and nothing
is written back to disk.

Drawing contents are sent to Claude as part of your conversation, the same way
any other file you share is. The plugin itself opens no network connections and
sends your drawings nowhere else.

## What it downloads

The server is a native binary, too large to ship inside a plugin, so the
plugin starts it through its npm launcher:

```
npx -y @dmytro-prototypes/dwg-mcp-server@0.2.4
```

On first run that launcher downloads the platform build for your computer from
this project's GitHub releases, verifies it against the published SHA-256
checksum, and caches it under `~/.cache/dwg-mcp-server`. Later runs reuse the
cache and download nothing. Set `DWG_MCP_CACHE_DIR` to cache it elsewhere.
Node.js is required; Python and Docker are not.

Supported platforms: macOS on Apple Silicon, Windows x64, Linux x64, and Linux
ARM64. Intel macOS needs a release that publishes an Intel build; version
0.2.4 does not have one.

## Where it runs

The server runs as a local process, so it loads in Claude Code, where you are
prompted for the drawings folder when you enable the plugin.

Cowork runs local servers only in sessions on your own computer, and it never
prompts for configuration, so it starts the server with no configured folder
and the server reads whatever roots the session provides.

Chat on claude.ai, the desktop app's chat, and the mobile apps skip local MCP
servers entirely.

## Links

- Source and issues: https://github.com/dimitrovakulenko/dwg-mcp-server
- License: GPL-3.0-or-later (see `LICENSE`)
