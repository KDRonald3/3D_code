# Start horizon-server on loopback for Map analyse (Windows).
# Writes the listen URL to ide\.cache\sidecar.url for Run.ps1 / IDE attach.
[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

Write-HorizonInfo "Horizon sidecar (loopback only, --no-open)"
Write-HorizonInfo "URL file: $($Roots.SidecarUrlFile)"

function Write-UrlFromLine([string]$Line) {
    if ($Line -match '^http://(127\.0\.0\.1|localhost|\[::1\]):\d+/?$') {
        Write-HorizonSidecarUrlFile -Roots $Roots -Url $Line
        Write-Host "set `$env:HORIZON_SIDECAR_URL = '$($Line.TrimEnd('/'))'"
    }
}

$bin = Resolve-HorizonServerBinary -Roots $Roots
$argList = @("--no-open")
if ($ExtraArgs) { $argList += $ExtraArgs }

if ($bin) {
    Write-HorizonInfo "using binary: $bin"
    New-Item -ItemType Directory -Force -Path $Roots.CacheDir | Out-Null
    & $bin @argList 2>&1 | ForEach-Object {
        $line = "$_"
        Write-Host $line
        Write-UrlFromLine $line
    }
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
    New-Item -ItemType Directory -Force -Path $Roots.CacheDir | Out-Null
    cargo run -p horizon-server --release -- --no-open @ExtraArgs 2>&1 | ForEach-Object {
        $line = "$_"
        Write-Host $line
        Write-UrlFromLine $line
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
