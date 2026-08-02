# Build Horizon IDE on Windows (npm ci + npm run compile).
[CmdletBinding()]
param(
    [switch]$SkipPrereqCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

Write-HorizonInfo "Horizon IDE build (Windows)"

if (-not (Test-Path (Join-Path $Roots.CodeOssDir "package.json"))) {
    Throw-Horizon "Code-OSS missing; run .\ide\scripts\windows\Bootstrap.ps1 first"
}

Initialize-HorizonNode
if (-not $SkipPrereqCheck) {
    Assert-HorizonWindowsPrereqs
}

$upstream = Join-Path $Roots.CodeOssDir "product.json.upstream"
$codeProduct = Join-Path $Roots.CodeOssDir "product.json"
if (Test-Path $upstream) {
    Copy-Item $upstream $codeProduct -Force
}
Merge-HorizonProductOverlay -Roots $Roots
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
  - Missing Visual Studio 2022 C++ tools (Desktop development with C++)
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

    Write-HorizonInfo "compiling Code-OSS (npm run compile)…"
    npm run compile
    if ($LASTEXITCODE -ne 0) {
        Throw-Horizon "npm run compile failed. Fix errors above, then re-run Build.ps1. Prefer WSL2 if native Windows keeps failing — see ide\WINDOWS.md"
    }

    if (-not (Test-Path "scripts\code.bat")) {
        Throw-Horizon "scripts\code.bat missing from Code-OSS tree"
    }
    if (-not (Test-Path "out\main.js") -and -not (Test-Path "out\vs\code\electron-main\main.js")) {
        Throw-Horizon "compile finished but electron main entry is missing under out\"
    }
} finally {
    Pop-Location
}

Write-HorizonInfo "build complete"
Write-Host ""
Write-Host "Launch with:"
Write-Host "  .\ide\scripts\windows\Run.ps1 [workspace-path]"
