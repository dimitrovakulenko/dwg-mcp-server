#!/bin/sh
set -eu

repository="dimitrovakulenko/dwg-mcp-server"
target="aarch64-apple-darwin"

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "DWG MCP Server supports this installer on macOS Apple Silicon only." >&2
  exit 1
fi

release_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$repository/releases/latest")
tag=${release_url##*/}
version=${tag#v}
asset="dwg-mcp-server-$version-$target.tar.gz"
download_url="https://github.com/$repository/releases/download/$tag/$asset"
temporary_dir=$(mktemp -d)
trap 'rm -rf "$temporary_dir"' EXIT HUP INT TERM

echo "Downloading DWG MCP Server $version for macOS Apple Silicon..."
curl -fsSL "$download_url" -o "$temporary_dir/$asset"
curl -fsSL "$download_url.sha256" -o "$temporary_dir/$asset.sha256"

expected=$(awk '{print $1}' "$temporary_dir/$asset.sha256")
actual=$(shasum -a 256 "$temporary_dir/$asset" | awk '{print $1}')
if [ "$actual" != "$expected" ]; then
  echo "Checksum verification failed for $asset." >&2
  exit 1
fi

install_root=${DWG_MCP_INSTALL_ROOT:-"$HOME/.local/share/dwg-mcp-server"}
install_dir="$install_root/$version"
bin_dir=${DWG_MCP_BIN_DIR:-"$HOME/.local/bin"}
mkdir -p "$install_root" "$bin_dir"
tar -xzf "$temporary_dir/$asset" -C "$temporary_dir"
rm -rf "$install_dir"
mv "$temporary_dir/dwg-mcp-server" "$install_dir"
ln -sfn "$install_dir/dwg-mcp-server" "$bin_dir/dwg-mcp-server"

echo "Installed DWG MCP Server $version to $install_dir"
echo "Executable: $bin_dir/dwg-mcp-server"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "Add $bin_dir to PATH before using dwg-mcp-server." ;;
esac
