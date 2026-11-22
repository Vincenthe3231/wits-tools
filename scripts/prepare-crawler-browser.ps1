# Stages the pinned Obscura runtime for development and Windows installers.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT' -or ($env:PROCESSOR_ARCHITEW6432 -ne 'AMD64' -and $env:PROCESSOR_ARCHITECTURE -ne 'AMD64')) {
  throw 'The bundled crawler browser requires Windows x64.'
}

# The release archive and published SHA-256 are updated together after browser smoke tests.
$version = '0.2.7'
$sha256 = 'A2AC7D8B08E93E3ED31F6F14CC7057D822725D3F55D91FC7992A6F2D3E0AD1B6'
$native = Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri'
$target = Join-Path $native 'target'
$stage = Join-Path $native 'resources/crawler-browser'
$archive = Join-Path $target "obscura-v$version.zip"
$release = "https://github.com/h4ckf0r0day/obscura/releases/download/v$version/obscura-x86_64-windows-no-render.zip"
New-Item -ItemType Directory -Force -Path $target | Out-Null

# Use .NET directly so verification works without the PowerShell utility module.
function Get-Sha256([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  $algorithm = [Security.Cryptography.SHA256]::Create()
  try {
    return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '')
  } finally {
    $algorithm.Dispose()
    $stream.Dispose()
  }
}
# Verify before extraction, including when using an archive from the local build cache.
if (-not (Test-Path -LiteralPath $archive)) {
  $download = Join-Path $target ("obscura-" + [Guid]::NewGuid() + '.download')
  try {
    Invoke-WebRequest -UseBasicParsing -Uri $release -OutFile $download -TimeoutSec 120
    if ((Get-Sha256 $download) -ne $sha256) {
      throw 'The Obscura archive checksum did not match the pinned release.'
    }
    Move-Item -LiteralPath $download -Destination $archive -Force
  } finally {
    if (Test-Path -LiteralPath $download) { Remove-Item -LiteralPath $download }
  }
}
if ((Get-Sha256 $archive) -ne $sha256) {
  throw "The cached Obscura archive failed verification. Remove the archive at $archive and retry."
}

# Ship the CDP server and its required DLLs. The parallel scrape worker is unused.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
try {
  foreach ($file in @('obscura.exe', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
    $entry = $zip.GetEntry($file)
    if (-not $entry) { throw "Missing $file in verified Obscura archive." }
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $stage $file), $true)
  }
} finally {
  $zip.Dispose()
}
