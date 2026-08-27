import assert from "node:assert/strict"
import test from "node:test"

import { nativeTarget } from "../bin/cli.js"

test("maps supported native targets", () => {
  assert.equal(nativeTarget("darwin", "arm64"), "aarch64-apple-darwin")
  assert.equal(nativeTarget("linux", "arm64"), "aarch64-unknown-linux-gnu")
  assert.equal(nativeTarget("linux", "x64"), "x86_64-unknown-linux-gnu")
  assert.equal(nativeTarget("win32", "x64"), "x86_64-pc-windows-gnu")
})

test("rejects unsupported native targets", () => {
  assert.throws(() => nativeTarget("darwin", "x64"), /Unsupported platform/)
})
