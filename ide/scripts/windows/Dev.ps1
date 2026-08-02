# Fast iteration on Windows: sync contrib -> gulp compile-client -> run.
# Use after editing ide\contrib\horizon\ when a full Build already exists.
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
no prior build.

Run once:
  .\ide\scripts\windows\Bootstrap.ps1
  .\ide\scripts\windows\Build.ps1
Then use .\ide\scripts\windows\Dev.ps1 for sync + compile-client + run.
"@
}

Write-HorizonInfo "Horizon IDE fast path (Sync-Contrib + compile-client + run)"
Ensure-HorizonProductOverlay -Roots $Roots
Repair-HorizonPreinstallVs2026 -Roots $Roots
$mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
Sync-HorizonContrib -Roots $Roots -Mode $mode
Invoke-HorizonCompileClient -Roots $Roots
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
