# Build Horizon IDE on Windows (npm ci + npm run compile).
[CmdletBinding()]
param(
    [switch]$SkipPrereqCheck
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
# npm writes warnings to stderr; do not treat native stderr as terminating errors (PS 7+).
if (Test-Path variable:/PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

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
    # Cursor/sandbox sometimes injects npm_config_devdir; strip unknown configs that spam stderr.
    if ($env:npm_config_devdir) { Remove-Item Env:npm_config_devdir -ErrorAction SilentlyContinue }
    if (-not $env:NODE_OPTIONS) {
        $env:NODE_OPTIONS = "--max-old-space-size=8192"
    }
    if (-not $env:PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD) {
        $env:PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1"
    }
    if (-not $env:GYP_MSVS_VERSION -and $env:npm_config_msvs_version) {
        $env:GYP_MSVS_VERSION = $env:npm_config_msvs_version
    }
    # npm's bundled node-gyp (<= 11.x) only knows Visual Studio up to 2022; VS 2026 needs node-gyp 12+.
    if (-not $env:npm_config_node_gyp) {
        $modernGyp = Resolve-HorizonNodeGyp
        if ($modernGyp) {
            $env:npm_config_node_gyp = $modernGyp
            Write-HorizonInfo "npm_config_node_gyp -> $modernGyp"
        }
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
        Write-HorizonInfo "installing npm dependencies (this takes a while)..."
        if ((Test-Path "node_modules") -and -not (Test-Path "node_modules\gulp") -and -not (Test-Path "node_modules\.bin\gulp.cmd")) {
            Remove-Item -Recurse -Force "node_modules"
        }
        if (Test-Path "package-lock.json") {
            npm.cmd ci --no-fund --no-audit
            if ($LASTEXITCODE -ne 0) {
                Throw-Horizon "npm ci failed. Common causes: missing VS C++ tools, Node < 22.15.1, yarn, path length/antivirus. See ide\WINDOWS.md"
            }
        } else {
            npm.cmd install --no-fund --no-audit
            if ($LASTEXITCODE -ne 0) {
                Throw-Horizon "npm install failed - see errors above and ide\WINDOWS.md"
            }
        }
    } else {
        Write-HorizonInfo "node_modules present; skipping npm ci (set `$env:HORIZON_FORCE_NPM_CI=1 to reinstall)"
    }

    Write-HorizonInfo "compiling Code-OSS (npm run compile)..."
    npm.cmd run compile
    if ($LASTEXITCODE -ne 0) {
        Throw-Horizon "npm run compile failed. Fix errors above, then re-run Build.ps1. Prefer WSL2 if native Windows keeps failing - see ide\WINDOWS.md"
    }

    if (-not (Test-Path "scripts\code.bat")) {
        Throw-Horizon "scripts\code.bat missing from Code-OSS tree"
    }
    if (-not (Test-Path "out\main.js") -and -not (Test-Path "out\vs\code\electron-main\main.js")) {
        Throw-Horizon "compile finished but electron main entry is missing under out/"
    }
} finally {
    Pop-Location
}

Write-HorizonInfo "build complete"
Write-Host ""
Write-Host "Launch with:"
Write-Host "  .\ide\scripts\windows\Run.ps1 [workspace-path]"
