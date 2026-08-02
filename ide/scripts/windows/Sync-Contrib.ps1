# Re-sync ide\contrib\horizon into the Code-OSS workbench tree.
[CmdletBinding()]
param(
    [ValidateSet("copy", "link")]
    [string]$Mode = "copy"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots
Sync-HorizonContrib -Roots $Roots -Mode $Mode
Write-HorizonInfo "done - run Build.ps1 / Dev.ps1 (gulp compile-client) so out\ picks up changes"

