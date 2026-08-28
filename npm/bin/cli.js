#!/usr/bin/env node
import { createHash } from "node:crypto"
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from "node:fs"
import os from "node:os"
import path from "node:path"
import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const { version } = JSON.parse(readFileSync(path.join(packageRoot, "package.json"), "utf8"))

export function nativeTarget(platform = process.platform, arch = process.arch) {
  const targets = {
    "darwin-arm64": "aarch64-apple-darwin",
    "linux-arm64": "aarch64-unknown-linux-gnu",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "win32-x64": "x86_64-pc-windows-gnu"
  }
  const target = targets[`${platform}-${arch}`]
  if (!target) throw new Error(`Unsupported platform: ${platform} ${arch}`)
  return target
}

export function isMain(entry = process.argv[1]) {
  return Boolean(entry) && existsSync(entry) && realpathSync(entry) === realpathSync(fileURLToPath(import.meta.url))
}

async function download(url, destination) {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`Download failed (${response.status}): ${url}`)
  writeFileSync(destination, Buffer.from(await response.arrayBuffer()))
}

async function installNativeBundle(target) {
  const cacheRoot = process.env.DWG_MCP_CACHE_DIR || path.join(os.homedir(), ".cache", "dwg-mcp-server")
  const installDir = path.join(cacheRoot, version, target)
  const executable = path.join(installDir, `dwg-mcp-server${process.platform === "win32" ? ".exe" : ""}`)
  if (existsSync(executable)) return executable

  mkdirSync(path.dirname(installDir), { recursive: true })
  const temporaryDir = mkdtempSync(path.join(path.dirname(installDir), `${target}-`))
  const asset = `dwg-mcp-server-${version}-${target}.tar.gz`
  const baseUrl = `https://github.com/dimitrovakulenko/dwg-mcp-server/releases/download/v${version}`
  const archive = path.join(temporaryDir, asset)
  const checksum = `${archive}.sha256`

  try {
    console.error(`Downloading DWG MCP Server ${version} for ${target}...`)
    await download(`${baseUrl}/${asset}`, archive)
    await download(`${baseUrl}/${asset}.sha256`, checksum)
    const expected = readFileSync(checksum, "utf8").trim().split(/\s+/)[0]
    const actual = createHash("sha256").update(readFileSync(archive)).digest("hex")
    if (actual !== expected) throw new Error(`Checksum mismatch for ${asset}`)

    const extracted = spawnSync("tar", ["-xzf", archive, "-C", temporaryDir], { stdio: "inherit" })
    if (extracted.error) throw extracted.error
    if (extracted.status !== 0) throw new Error(`Failed to extract ${asset}`)

    try {
      renameSync(path.join(temporaryDir, "dwg-mcp-server"), installDir)
    } catch (error) {
      if (!existsSync(executable)) throw error
    }
    return executable
  } finally {
    rmSync(temporaryDir, { recursive: true, force: true })
  }
}

export async function main() {
  const executable = await installNativeBundle(nativeTarget())
  const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit", env: process.env })
  if (result.error) throw result.error
  process.exit(result.status ?? 1)
}

if (isMain()) {
  main().catch(error => {
    console.error(error.message)
    process.exit(1)
  })
}
