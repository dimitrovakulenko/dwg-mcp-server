"""Package an extracted native release for Claude Desktop and ChatGPT desktop."""

from __future__ import annotations

import argparse
import json
import shutil
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = {
    "aarch64-apple-darwin": "darwin",
    "x86_64-apple-darwin": "darwin",
    "x86_64-pc-windows-gnu": "win32",
    "x86_64-unknown-linux-gnu": "linux",
    "aarch64-unknown-linux-gnu": "linux",
}


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def archive(directory: Path, destination: Path, *, enclosing_folder: bool = False) -> None:
    base = directory.parent if enclosing_folder else directory
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(directory.rglob("*")):
            if path.is_file():
                bundle.write(path, path.relative_to(base))
    print(destination)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native_bundle", type=Path, help="Extracted dwg-mcp-server directory")
    parser.add_argument("--target", required=True, choices=PLATFORMS)
    parser.add_argument("--output", type=Path, default=ROOT / "target" / "desktop-packages")
    args = parser.parse_args()
    executable = "dwg-mcp-server.exe" if PLATFORMS[args.target] == "win32" else "dwg-mcp-server"
    worker = "dwg-worker.exe" if PLATFORMS[args.target] == "win32" else "dwg-worker"
    for required in (executable, worker, "_internal", "libredwg/classes.c", "libredwg/dynapi.c"):
        if not (args.native_bundle / required).exists():
            parser.error(f"native bundle is missing {required}")
    version = json.loads((ROOT / "npm/package.json").read_text())["version"]
    args.output.mkdir(parents=True, exist_ok=True)
    name = f"dwg-mcp-server-{version}-{args.target}"

    with tempfile.TemporaryDirectory(prefix="dwg-desktop-", dir=args.output) as temporary:
        stage = Path(temporary)
        claude = stage / "claude"
        claude.mkdir()
        # Dereference framework symlinks so both formats contain ordinary files only.
        shutil.copytree(args.native_bundle, claude / "server")
        shutil.copy2(ROOT / "LICENSE", claude / "LICENSE")
        shutil.copy2(ROOT / "claude-extension/README.md", claude / "README.md")
        manifest = json.loads((ROOT / "claude-extension/manifest.json").read_text())
        manifest["version"] = version
        manifest["compatibility"] = {"platforms": [PLATFORMS[args.target]]}
        manifest["server"]["entry_point"] = f"server/{executable}"
        manifest["server"]["mcp_config"]["command"] = "${__dirname}/server/" + executable
        write_json(claude / "manifest.json", manifest)
        archive(claude, args.output / f"{name}.mcpb")

        chatgpt = stage / "dwg-mcp-server"
        shutil.copytree(ROOT / "chatgpt-plugin", chatgpt)
        shutil.move(str(claude / "server"), chatgpt / "server")
        shutil.copy2(ROOT / "LICENSE", chatgpt / "LICENSE")
        plugin = json.loads((chatgpt / "plugin.json").read_text())
        plugin["version"] = version
        write_json(chatgpt / "plugin.json", plugin)
        mcp = json.loads((chatgpt / "mcp.json").read_text())
        mcp["mcpServers"]["dwg-mcp"]["command"] = "${PLUGIN_ROOT}/server/" + executable
        write_json(chatgpt / "mcp.json", mcp)
        archive(chatgpt, args.output / f"{name}-chatgpt.zip", enclosing_folder=True)


if __name__ == "__main__":
    main()
