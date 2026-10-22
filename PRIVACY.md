# Privacy Policy

_Last updated: 2026-10-22_

**Short version: DWG MCP Server collects nothing about you and sends nothing to
its author.**

DWG MCP Server ("the Software") is free, open-source software published at
https://github.com/dimitrovakulenko/dwg-mcp-server. It runs on your own
computer. There is no account, no hosted service, and no operator-side database
in which anything about you could be stored.

## What the Software collects

Nothing. The Software has no analytics, no telemetry, no crash reporting, and
no usage counters. It does not phone home, and it opens no network connections
of its own.

## What it reads on your computer

The Software reads DWG and DXF files from the folder you choose when you
configure it, plus any roots your AI client provides. It does not scan the rest
of your disk, and it does not modify, move, or delete the drawings it opens.
Property edits apply to an in-memory copy of the drawing and are discarded when
the drawing closes.

## Where your drawing contents go

When you ask about a drawing, the contents the Software returns go to the AI
client you are running, as part of your conversation with it, exactly like any
other file you share with that client. Your client's own privacy policy governs
what happens to them from there. They are not sent anywhere else, and they are
never sent to the author of the Software.

## Network connections

The Software itself makes none. One related component does: the npm launcher
downloads the native build for your platform from this project's GitHub
releases on first run, verifies it against a published checksum, and caches it
locally so later runs download nothing. That request goes to GitHub and the npm
registry, which receive the ordinary technical information any download
involves, such as your IP address and the time of the request, under their own
privacy policies. The author receives none of it and has no access to it.

You can avoid even that request by installing a release yourself and running
the binary directly.

## Cookies, tracking, and children

The Software has no web interface, so it sets no cookies and uses no tracking
technologies. It is a developer tool and is not directed at children.

## Changes

This policy may change as the Software develops. The version in the repository
at the time you obtain a given release is the version that applies to it, and
the file's history is public.

## Contact

Questions: https://github.com/dimitrovakulenko/dwg-mcp-server/issues
