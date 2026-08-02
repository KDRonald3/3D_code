# Build Horizon IDE on Windows.
#
# Always:
#   1. Re-apply product.json overlay (branding)
#   2. Patch VS 2026 into Code-OSS preinstall.js
#   3. Sync ide\contrib\horizon → code-oss\src\vs\workbench\contrib\horizon
#   4. npm ci when needed
#   5. Compile so Horizon TS lands in out\ (Map buttons, analyse, folder picker)
#
# Compile mode ($env:HORIZON_COMPILE_MODE):
#   client (default) — npx gulp compile-client (workbench src → out/, includes contrib)
#   full — npm run compile (client + extensions; heavier / flakier)
#
# After a successful build: .\ide\scripts\windows\Run.ps1
[CmdletBinding()]
param(
    [switch]$SkipPrereqCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

$compileMode = if ($env:HORIZON_COMPILE_MODE) { $env:HORIZON_COMPILE_MODE } else { "client" }
Write-HorizonInfo "Horizon IDE build (Windows)"
Write-HorizonInfo "compile mode: $compileMode"

if (-not (Test-Path (Join-Path $Roots.CodeOssDir "package.json"))) {
    Throw-Horizon "Code-OSS missing; run .\ide\scripts\windows\Bootstrap.ps1 first"
}

Initialize-HorizonNode
if (-not $SkipPrereqCheck) {
    Assert-HorizonWindowsPrereqs
}

# --- Product surface: brand + toolchain patch + always sync contrib -----------
Ensure-HorizonProductOverlay -Roots $Roots
Repair-HorizonPreinstallVs2026 -Roots $Roots
$mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
Sync-HorizonContrib -Roots $Roots -Mode $mode

Push-Location $Roots.CodeOssDir
try {
    $env:npm_config_fund = "false"
    $env:npm_config_audit = "false"
    if (-not $env:NODE_OPTIONS) {
        $env:NODE_OPTIONS = "--max-old-space-size=8192"
    }

    $needInstall = $false
    if ($env:HORIZON_FORCE_NPM_CI -eq "1") {
        $needInstall = $true
    } elseif (-not (Test-Path "node_modules")) {
        $needInstall = $true
    } elseif (-not (Test-Path "node_modules\gulp") -and -not (Test-Path "node_modules\.bin\gulp.cmd")) {
        Write-HorizonWarn "node_modules looks incomplete (gulp missing); reinstalling"
        $needInstall = $true
    }

    if ($needInstall) {
        Write-HorizonInfo "installing npm dependencies (this takes a while)…"
        if ((Test-Path "node_modules") -and -not (Test-Path "node_modules\gulp") -and -not (Test-Path "node_modules\.bin\gulp.cmd")) {
            Remove-Item -Recurse -Force "node_modules"
        }
        if (Test-Path "package-lock.json") {
            npm ci --no-fund --no-audit
            if ($LASTEXITCODE -ne 0) {
                Throw-Horizon @"
npm ci failed.

Common Windows causes:
  - Missing Visual Studio 2026/2022 C++ tools (Desktop development with C++)
  - Node < 22.15.1
  - Using yarn (unsupported — use npm)
  - Path too long / antivirus locking node_modules

See ide\WINDOWS.md
"@
            }
        } else {
            npm install --no-fund --no-audit
            if ($LASTEXITCODE -ne 0) {
                Throw-Horizon "npm install failed — see errors above and ide\WINDOWS.md"
            }
        }
    } else {
        Write-HorizonInfo "node_modules present; skipping npm ci (set `$env:HORIZON_FORCE_NPM_CI=1 to reinstall)"
    }
} finally {
    Pop-Location
}

# Default: compile-client so contrib/horizon TypeScript is emitted under out\.
Invoke-HorizonCompileCodeOss -Roots $Roots

if (-not (Test-Path (Join-Path $Roots.CodeOssDir "scripts\code.bat"))) {
    Throw-Horizon "scripts\code.bat missing from Code-OSS tree"
}
if (-not (Test-HorizonBuiltIn -Roots $Roots)) {
    Throw-Horizon "build finished but Horizon is not present under out\ (missing electron main or contrib JS)"
}

Write-HorizonInfo "build complete — Horizon Map contrib is compiled into out\"
Write-Host ""
Write-Host "Launch with:"
Write-Host "  .\ide\scripts\windows\Run.ps1 [workspace-path]"
Write-Host ""
Write-Host "Fast iteration after editing ide\contrib\horizon:"
Write-Host "  .\ide\scripts\windows\Dev.ps1 [workspace-path]"
Write-Host "  # or: `$env:HORIZON_COMPILE_MODE='full'; .\ide\scripts\windows\Build.ps1"
