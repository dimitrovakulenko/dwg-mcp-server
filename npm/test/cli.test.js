import assert from "node:assert/strict"
import { mkdtempSync, rmSync, symlinkSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"

import { isMain, nativeTarget } from "../bin/cli.js"

test("maps supported native targets", () => {
  assert.equal(nativeTarget("darwin", "arm64"), "aarch64-apple-darwin")
  assert.equal(nativeTarget("linux", "arm64"), "aarch64-unknown-linux-gnu")
  assert.equal(nativeTarget("linux", "x64"), "x86_64-unknown-linux-gnu")
  assert.equal(nativeTarget("win32", "x64"), "x86_64-pc-windows-gnu")
})

test("rejects unsupported native targets", () => {
  assert.throws(() => nativeTarget("darwin", "x64"), /Unsupported platform/)
})

test("recognizes npm bin symlink as the main script", { skip: process.platform === "win32" }, () => {
  const temporaryDir = mkdtempSync(path.join(os.tmpdir(), "dwg-mcp-cli-"))
  const symlink = path.join(temporaryDir, "dwg-mcp-server")
  try {
    symlinkSync(fileURLToPath(new URL("../bin/cli.js", import.meta.url)), symlink)
    assert.equal(isMain(symlink), true)
  } finally {
    rmSync(temporaryDir, { recursive: true, force: true })
  }
})
