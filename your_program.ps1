# Use this script to run your program LOCALLY.
#
# Note: Changing this script WILL NOT affect how CodeCrafters runs your program.
#
# Learn more: https://codecrafters.io/program-interface

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Paths
$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$targetDir = Join-Path ([System.IO.Path]::GetTempPath()) "codecrafters-build-bittorrent-rust"
$manifestPath = Join-Path $repoRoot "Cargo.toml"
$binaryPath = Join-Path (Join-Path $targetDir "release") "codecrafters-bittorrent.exe"

# Copied from .codecrafters/compile.sh
Push-Location $repoRoot
try {
  cargo build --release --target-dir $targetDir --manifest-path $manifestPath | Out-Default
}
finally {
  Pop-Location
}

# Copied from .codecrafters/run.sh
& $binaryPath @args
