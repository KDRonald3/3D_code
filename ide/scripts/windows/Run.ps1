# Launch the built Horizon IDE on Windows (forked Code-OSS via scripts\code.bat).
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

$upstream = Join-Path $Roots.CodeOssDir "product.json.upstream"
$codeProduct = Join-Path $Roots.CodeOssDir "product.json"
if (Test-Path $upstream) {
    Copy-Item $upstream $codeProduct -Force
}
Merge-HorizonProductOverlay -Roots $Roots
$mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
Sync-HorizonContrib -Roots $Roots -Mode $mode

Initialize-HorizonNode

$codeBat = Join-Path $Roots.CodeOssDir "scripts\code.bat"
Write-HorizonInfo "launching built Horizon IDE ($codeBat)"

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
