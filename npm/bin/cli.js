#!/usr/bin/env node
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import path from "node:path"

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const script = path.join(__dirname, "..", "scripts", "run-docker-mcp-server.sh")

const result = spawnSync("bash", [script, ...process.argv.slice(2)], {
  stdio: "inherit",
  env: process.env
})

if (result.error) {
  throw result.error
}

process.exit(result.status ?? 1)
