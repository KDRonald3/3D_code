# Bootstrap Horizon IDE on Windows:
# shallow-clone microsoft/vscode into ide\code-oss, brand, sync contrib\horizon.
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Import-Module "$PSScriptRoot\HorizonIde.psm1" -Force
$Roots = Get-HorizonRoots

Write-HorizonInfo "Horizon IDE bootstrap (Windows)"
Write-HorizonInfo "vscode ref: $($Roots.VscodeRef)"
Write-HorizonInfo "code-oss dir: $($Roots.CodeOssDir)"

if (-not (Test-HorizonCommand "git")) {
    Throw-Horizon "Git is required. Install from https://git-scm.com/download/win"
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Roots.CodeOssDir) | Out-Null

$gitDir = Join-Path $Roots.CodeOssDir ".git"
if (Test-Path $gitDir) {
    Write-HorizonInfo "Code-OSS checkout already present"
    Push-Location $Roots.CodeOssDir
    try {
        $describe = git describe --tags --exact-match 2>$null
        if (-not $describe) { $describe = git rev-parse --short HEAD }
        Write-HorizonInfo "current checkout: $describe"
        $exact = git describe --tags --exact-match 2>$null
        if ($exact -ne $Roots.VscodeRef) {
            Write-HorizonInfo "fetching pinned ref $($Roots.VscodeRef)"
            git fetch --depth 1 origin "refs/tags/$($Roots.VscodeRef):refs/tags/$($Roots.VscodeRef)" 2>$null
            if ($LASTEXITCODE -ne 0) {
                git fetch --depth 1 origin $Roots.VscodeRef
            }
            if ((git rev-parse --verify "refs/tags/$($Roots.VscodeRef)" 2>$null)) {
                git checkout -q "refs/tags/$($Roots.VscodeRef)"
            } else {
                git checkout -q $Roots.VscodeRef
            }
        }
    } finally {
        Pop-Location
    }
} else {
    if (Test-Path $Roots.CodeOssDir) {
        Throw-Horizon "$($Roots.CodeOssDir) exists but is not a git checkout; remove it and re-run"
    }
    Write-HorizonInfo "shallow-cloning $($Roots.VscodeRepo) @ $($Roots.VscodeRef)"
    git clone --depth 1 --branch $Roots.VscodeRef $Roots.VscodeRepo $Roots.CodeOssDir
    if ($LASTEXITCODE -ne 0) {
        Write-HorizonInfo "branch clone failed; trying commit fetch"
        git clone --depth 1 $Roots.VscodeRepo $Roots.CodeOssDir
        Push-Location $Roots.CodeOssDir
        try {
            git fetch --depth 1 origin $Roots.VscodeRef
            git checkout -q FETCH_HEAD
        } finally {
            Pop-Location
        }
    }
}

if (-not (Test-Path (Join-Path $Roots.CodeOssDir "product.json"))) {
    Throw-Horizon "clone succeeded but product.json missing"
}

$upstream = Join-Path $Roots.CodeOssDir "product.json.upstream"
$codeProduct = Join-Path $Roots.CodeOssDir "product.json"
if (-not (Test-Path $upstream)) {
    Copy-Item $codeProduct $upstream
}
Copy-Item $upstream $codeProduct -Force
Merge-HorizonProductOverlay -Roots $Roots
Repair-HorizonPreinstallVs2026 -Roots $Roots

$mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
Sync-HorizonContrib -Roots $Roots -Mode $mode

Write-HorizonInfo "bootstrap complete"
Write-Host ""
Write-Host "Next (one path → full product with Horizon built in):"
Write-Host "  .\ide\scripts\windows\Build.ps1      # sync contrib + gulp compile-client → out\"
Write-Host "  .\ide\scripts\windows\Run.ps1 [workspace]"
Write-Host "  .\ide\scripts\windows\Dev.ps1 [workspace]   # fast: sync + compile-client + run"
Write-Host ""
Write-Host "If native Windows compile is too heavy, use WSL2 — see ide\WINDOWS.md"
