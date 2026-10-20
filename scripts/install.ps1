$ErrorActionPreference = "Stop"

$repository = "dimitrovakulenko/dwg-mcp-server"
if (-not [Environment]::Is64BitOperatingSystem) {
    throw "DWG MCP Server supports this installer on 64-bit Windows only."
}

$release = Invoke-RestMethod "https://api.github.com/repos/$repository/releases/latest"
$tag = $release.tag_name
$version = $tag.TrimStart("v")
$target = "x86_64-pc-windows-gnu"
$asset = "dwg-mcp-server-$version-$target.tar.gz"
$downloadUrl = "https://github.com/$repository/releases/download/$tag/$asset"
$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("dwg-mcp-server-" + [Guid]::NewGuid())
$archive = Join-Path $temporaryDirectory $asset
$checksum = "$archive.sha256"

New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null
try {
    Write-Host "Downloading DWG MCP Server $version for Windows x64..."
    Invoke-WebRequest $downloadUrl -OutFile $archive
    Invoke-WebRequest "$downloadUrl.sha256" -OutFile $checksum

    $expected = (Get-Content $checksum -Raw).Trim().Split()[0]
    $actual = (Get-FileHash $archive -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        throw "Checksum verification failed for $asset."
    }

    tar.exe -xzf $archive -C $temporaryDirectory
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to extract $asset."
    }

    $installRoot = if ($env:DWG_MCP_INSTALL_ROOT) { $env:DWG_MCP_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "DWGMCPServer" }
    $installDirectory = Join-Path $installRoot "current"
    New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
    if (Test-Path $installDirectory) {
        Remove-Item -Recurse -Force $installDirectory
    }
    Move-Item (Join-Path $temporaryDirectory "dwg-mcp-server") $installDirectory

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ";" | Where-Object { $_ })
    if ($installDirectory -notin $pathEntries) {
        [Environment]::SetEnvironmentVariable("Path", (($pathEntries + $installDirectory) -join ";"), "User")
    }
    $env:Path = "$installDirectory;$env:Path"

    Write-Host "Installed DWG MCP Server $version to $installDirectory"
    Write-Host "Restart open terminals and MCP clients before using dwg-mcp-server."
}
finally {
    Remove-Item -Recurse -Force $temporaryDirectory -ErrorAction SilentlyContinue
}
