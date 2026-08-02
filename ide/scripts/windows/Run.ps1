# Launch the built Horizon IDE on Windows (forked Code-OSS via scripts\code.bat).
#
# Before launch:
#   - Re-apply product overlay (branding)
#   - If contrib sources are newer than out\, auto sync + gulp compile-client
#   - Start / attach horizon-server sidecar (URL → ide\.cache\horizon-sidecar.url)
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Workspace = "",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$ExtraArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

if (-not (Test-HorizonCodeOssBuilt -Roots $Roots)) {
    Throw-Horizon @"
built Horizon IDE not ready.

Run:
  .\ide\scripts\windows\Bootstrap.ps1
  .\ide\scripts\windows\Build.ps1
Then re-run .\ide\scripts\windows\Run.ps1

Or use WSL2 — see ide\WINDOWS.md
"@
}

Ensure-HorizonProductOverlay -Roots $Roots

$mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
if (Test-HorizonContribOutStale -Roots $Roots) {
    Write-HorizonWarn "contrib newer than out\ — syncing and running gulp compile-client"
    Sync-HorizonContrib -Roots $Roots -Mode $mode
    Invoke-HorizonCompileClient -Roots $Roots
} else {
    Sync-HorizonContrib -Roots $Roots -Mode $mode
    if (-not (Test-Path (Get-HorizonContribOutJs -Roots $Roots))) {
        Write-HorizonWarn "Horizon contrib JS missing under out\ — compiling client"
        Invoke-HorizonCompileClient -Roots $Roots
    }
}

if (-not (Test-HorizonBuiltIn -Roots $Roots)) {
    Throw-Horizon "Horizon contrib is not compiled into out\. Run .\ide\scripts\windows\Build.ps1"
}

Ensure-HorizonSidecar -Roots $Roots

Initialize-HorizonNode

$codeBat = Join-Path $Roots.CodeOssDir "scripts\code.bat"
Write-HorizonInfo "launching built Horizon IDE ($codeBat)"
if ($env:HORIZON_SIDECAR_URL) {
    Write-HorizonInfo "HORIZON_SIDECAR_URL=$($env:HORIZON_SIDECAR_URL)"
}

$argList = @()
if ($Workspace) {
    $argList += (Resolve-Path $Workspace).Path
}
if ($ExtraArgs) {
    $argList += $ExtraArgs
}

Push-Location $Roots.CodeOssDir
try {
    & $codeBat @argList
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
