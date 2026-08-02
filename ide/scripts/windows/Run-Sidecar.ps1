# Start horizon-server on loopback for Map analyse (Windows).
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

Write-HorizonInfo "Horizon sidecar (loopback only, --no-open)"

function Resolve-HorizonServer {
    if ($env:HORIZON_SERVER_PATH -and (Test-Path $env:HORIZON_SERVER_PATH)) {
        return (Resolve-Path $env:HORIZON_SERVER_PATH).Path
    }
    $candidates = @(
        (Join-Path $Roots.RepoRoot "target\release\horizon-server.exe"),
        (Join-Path $Roots.RepoRoot "target\debug\horizon-server.exe")
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    if (Test-HorizonCommand "horizon-server") {
        return (Get-Command horizon-server).Source
    }
    return $null
}

$bin = Resolve-HorizonServer
if ($bin) {
    Write-HorizonInfo "using binary: $bin"
    & $bin --no-open
    exit $LASTEXITCODE
}

if (-not (Test-HorizonCommand "cargo")) {
    Throw-Horizon @"
horizon-server binary not found and cargo is not on PATH.

Build it first:
  cargo build -p horizon-server --release

Or set `$env:HORIZON_SERVER_PATH to the .exe path.
"@
}

Write-HorizonInfo "no binary found; using cargo run -p horizon-server -- --no-open"
Push-Location $Roots.RepoRoot
try {
    cargo run -p horizon-server --release -- --no-open
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
