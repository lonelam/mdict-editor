# Package the Windows release executable after `tauri build`.
# Keep this script compatible with Windows PowerShell 5.1 and PowerShell 7.
[CmdletBinding()]
param(
    [string]$BinaryPath,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $BinaryPath) {
    $BinaryPath = Join-Path $repoRoot 'src-tauri/target/release/mdict-editor.exe'
}
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $repoRoot 'src-tauri/target/portable'
}
if (-not (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) {
    throw "Release executable missing: $BinaryPath. Run 'npm run tauri build -- --no-bundle' first."
}
$binary = (Resolve-Path -LiteralPath $BinaryPath).Path
$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)

# Read the executable's PE machine type so cross-built binaries get the
# correct archive name instead of inheriting the packaging host's architecture.
$reader = [System.IO.BinaryReader]::new([System.IO.File]::OpenRead($binary))
try {
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Not a Windows executable.' }
    $reader.BaseStream.Position = 0x3C
    $peOffset = $reader.ReadInt32()
    if ($peOffset -lt 0 -or $peOffset -gt ($reader.BaseStream.Length - 6)) {
        throw 'Invalid PE header.'
    }
    $reader.BaseStream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Invalid PE signature.' }
    $architecture = switch ($reader.ReadUInt16()) {
        0x8664 { 'x64' }
        0xAA64 { 'arm64' }
        0x014C { 'x86' }
        default { throw 'Unsupported Windows executable architecture.' }
    }
} finally {
    $reader.Dispose()
}

$config = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri/tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $config.version
if ($version -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$') {
    throw "Invalid application version: $version"
}
$name = "mdict-editor-$version-windows-$architecture-portable"
$null = New-Item -ItemType Directory -Path $output -Force
$stage = Join-Path $output ('portable-stage-' + [Guid]::NewGuid().ToString('N'))
$content = Join-Path $stage $name
$archive = Join-Path $output "$name.zip"
$checksum = "$archive.sha256"
$null = New-Item -ItemType Directory -Path $content
try {
    Copy-Item -LiteralPath $binary -Destination (Join-Path $content 'mdict-editor.exe')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'portable-README.txt') -Destination (Join-Path $content 'README.txt')
    # MSVC statically links the WebView2 loader; GNU builds may emit this DLL.
    $loader = Join-Path (Split-Path -Parent $binary) 'WebView2Loader.dll'
    if (Test-Path -LiteralPath $loader -PathType Leaf) {
        Copy-Item -LiteralPath $loader -Destination (Join-Path $content 'WebView2Loader.dll')
    }
    Compress-Archive -LiteralPath $content -DestinationPath $archive -CompressionLevel Optimal -Force
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    $archiveStream = [System.IO.File]::OpenRead($archive)
    try {
        $hash = [BitConverter]::ToString($sha256.ComputeHash($archiveStream)).Replace('-', '').ToLowerInvariant()
    } finally {
        $archiveStream.Dispose()
        $sha256.Dispose()
    }
    [System.IO.File]::WriteAllText($checksum, "$hash  $name.zip`n", [System.Text.Encoding]::ASCII)
    Write-Output "Portable archive: $archive"
    Write-Output "SHA-256: $checksum"
} finally {
    # Only remove files created in this invocation's staging directory.
    # No recursive cleanup or deletion of the output directory is needed.
    foreach ($file in @('mdict-editor.exe', 'README.txt', 'WebView2Loader.dll')) {
        $stagedFile = Join-Path $content $file
        if (Test-Path -LiteralPath $stagedFile -PathType Leaf) {
            Remove-Item -LiteralPath $stagedFile -Force
        }
    }
    Remove-Item -LiteralPath $content
    Remove-Item -LiteralPath $stage
}
