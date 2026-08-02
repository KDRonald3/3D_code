# Shared helpers for Horizon IDE Windows scripts.
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-HorizonRoots {
    $scriptsWin = $PSScriptRoot
    $scripts = Split-Path -Parent $scriptsWin
    $ide = Split-Path -Parent $scripts
    $repo = Split-Path -Parent $ide
    $refFile = Join-Path $ide "product\vscode-ref.txt"
    $ref = if ($env:HORIZON_VSCODE_REF) {
        $env:HORIZON_VSCODE_REF.Trim()
    } elseif (Test-Path $refFile) {
        (Get-Content -Raw $refFile).Trim()
    } else {
        "1.105.1"
    }
    [pscustomobject]@{
        RepoRoot      = $repo
        IdeRoot       = $ide
        ScriptsRoot   = $scripts
        ProductJson   = Join-Path $ide "product\product.json"
        CodeOssDir    = Join-Path $ide "code-oss"
        ContribSrc    = Join-Path $ide "contrib\horizon"
        ContribDst    = Join-Path $ide "code-oss\src\vs\workbench\contrib\horizon"
        WorkbenchMain = Join-Path $ide "code-oss\src\vs\workbench\workbench.common.main.ts"
        VscodeRef     = $ref
        VscodeRepo    = if ($env:HORIZON_VSCODE_REPO) { $env:HORIZON_VSCODE_REPO } else { "https://github.com/microsoft/vscode.git" }
    }
}

function Write-HorizonInfo([string]$Message) {
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-HorizonWarn([string]$Message) {
    Write-Host "warning: $Message" -ForegroundColor Yellow
}

function Throw-Horizon([string]$Message) {
    throw "error: $Message"
}

function Test-HorizonCommand([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Initialize-HorizonNode {
    if (-not (Test-HorizonCommand "node")) {
        Throw-Horizon "Node.js is required (v22.15.1+). Install from https://nodejs.org/ and reopen PowerShell."
    }
    if (-not (Test-HorizonCommand "npm")) {
        Throw-Horizon "npm is required (bundled with Node.js)."
    }

    $ver = (node -v).TrimStart("v")
    $parts = $ver.Split(".")
    $major = [int]$parts[0]
    $minor = if ($parts.Length -gt 1) { [int]$parts[1] } else { 0 }
    $patch = if ($parts.Length -gt 2) { [int]$parts[2] } else { 0 }
    $ok = ($major -gt 22) -or ($major -eq 22 -and $minor -gt 15) -or ($major -eq 22 -and $minor -eq 15 -and $patch -ge 1)
    if (-not $ok) {
        if (-not $env:VSCODE_SKIP_NODE_VERSION_CHECK) {
            Throw-Horizon "Node.js >= 22.15.1 required (found v$ver). Upgrade Node, or set `$env:VSCODE_SKIP_NODE_VERSION_CHECK=1 to bypass."
        }
        Write-HorizonWarn "Node.js v$ver is below 22.15.1; continuing because VSCODE_SKIP_NODE_VERSION_CHECK is set"
    }
    Write-HorizonInfo "Node v$ver ($((Get-Command node).Source)), npm $(npm -v)"
}

function Test-HorizonWindowsBuildTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        return $false
    }
    $found = & $vswhere -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath `
        -latest 2>$null
    return [bool]$found
}

function Assert-HorizonWindowsPrereqs {
    if (-not (Test-HorizonCommand "git")) {
        Throw-Horizon "Git is required. Install from https://git-scm.com/download/win"
    }
    if (-not (Test-HorizonCommand "python") -and -not (Test-HorizonCommand "python3")) {
        Throw-Horizon "Python 3 is required (on PATH as python). Install from https://www.python.org/downloads/ and enable 'Add python.exe to PATH'."
    }
    if (-not (Test-HorizonWindowsBuildTools)) {
        Throw-Horizon @"
Visual Studio 2022 C++ build tools are required to compile Code-OSS on Windows.

Install 'Build Tools for Visual Studio 2022' with workload:
  Desktop development with C++

Or see: https://github.com/microsoft/vscode/wiki/How-to-Contribute#prerequisites

If this is too heavy, use WSL2 instead — see ide\WINDOWS.md
"@
    }
    Write-HorizonInfo "Windows build prerequisites look present (Git, Python, MSVC)"
}

function Merge-HorizonProductOverlay {
    param($Roots)
    $codeProduct = Join-Path $Roots.CodeOssDir "product.json"
    $overlay = $Roots.ProductJson
    if (-not (Test-Path $codeProduct)) { Throw-Horizon "missing $codeProduct (run Bootstrap.ps1 first)" }
    if (-not (Test-Path $overlay)) { Throw-Horizon "missing $overlay" }

    # Prefer Python for a faithful JSON merge (PowerShell ConvertTo-Json mangles nested shapes).
    $python = $null
    if (Test-HorizonCommand "python") { $python = "python" }
    elseif (Test-HorizonCommand "python3") { $python = "python3" }
    if (-not $python) {
        Throw-Horizon "Python 3 is required to merge product.json"
    }

    $py = @'
import json, sys
vendor_path, overlay_path = sys.argv[1], sys.argv[2]
with open(vendor_path, encoding="utf-8") as f:
    product = json.load(f)
with open(overlay_path, encoding="utf-8") as f:
    overlay = json.load(f)
for key, value in overlay.items():
    product[key] = value
with open(vendor_path, "w", encoding="utf-8") as f:
    json.dump(product, f, indent="\t", ensure_ascii=False)
    f.write("\n")
print(f"applied product overlay -> {vendor_path}")
'@
    $tmp = [System.IO.Path]::GetTempFileName() + ".py"
    try {
        Set-Content -Path $tmp -Value $py -Encoding UTF8
        & $python $tmp $codeProduct $overlay
        if ($LASTEXITCODE -ne 0) {
            Throw-Horizon "product.json overlay merge failed"
        }
    } finally {
        Remove-Item -Force $tmp -ErrorAction SilentlyContinue
    }
}

function Sync-HorizonContrib {
    param(
        $Roots,
        [ValidateSet("copy", "link")]
        [string]$Mode = "copy"
    )
    $src = $Roots.ContribSrc
    $dst = $Roots.ContribDst
    if (-not (Test-Path $Roots.CodeOssDir)) {
        Throw-Horizon "Code-OSS checkout missing at $($Roots.CodeOssDir) (run Bootstrap.ps1 first)"
    }
    if (-not (Test-Path (Join-Path $Roots.CodeOssDir "src\vs\workbench\contrib"))) {
        Throw-Horizon "Code-OSS workbench contrib/ missing"
    }
    if (-not (Test-Path (Join-Path $src "browser\horizon.contribution.ts"))) {
        Throw-Horizon "contrib entry missing: $src\browser\horizon.contribution.ts"
    }

    $parent = Split-Path -Parent $dst
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    if (Test-Path $dst) {
        Remove-Item -Recurse -Force $dst
    }

    if ($Mode -eq "link") {
        New-Item -ItemType Junction -Path $dst -Target $src | Out-Null
        Write-HorizonInfo "linked $src -> $dst"
    } else {
        New-Item -ItemType Directory -Force -Path $dst | Out-Null
        Copy-Item -Path (Join-Path $src "*") -Destination $dst -Recurse -Force
        Write-HorizonInfo "copied $src -> $dst"
    }

    Wire-HorizonContribImport -Roots $Roots
}

function Wire-HorizonContribImport {
    param($Roots)
    $mainTs = $Roots.WorkbenchMain
    $marker = "contrib/horizon/browser/horizon.contribution"
    $importLine = "import './contrib/horizon/browser/horizon.contribution.js';"
    if (-not (Test-Path $mainTs)) { Throw-Horizon "missing $mainTs" }

    $text = [System.IO.File]::ReadAllText($mainTs)
    if ($text.Contains($marker)) {
        Write-HorizonInfo "Horizon contrib already registered in workbench.common.main.ts"
        return
    }

    $block = "`r`n// Horizon Map (built-in workbench contrib — not an extension)`r`n$importLine`r`n"
    $contribHdr = $text.IndexOf("--- workbench contributions")
    $needle = "//#endregion"
    $idx = -1
    if ($contribHdr -ge 0) {
        $idx = $text.IndexOf($needle, $contribHdr)
    }
    if ($idx -lt 0) {
        $text = $text.TrimEnd() + "`r`n" + $block
    } else {
        $text = $text.Substring(0, $idx) + $block + "`r`n" + $text.Substring($idx)
    }
    [System.IO.File]::WriteAllText($mainTs, $text, [System.Text.UTF8Encoding]::new($false))
    Write-HorizonInfo "wired Horizon contrib import -> $mainTs"
}

function Test-HorizonCodeOssBuilt {
    param($Roots)
    $dir = $Roots.CodeOssDir
    if (-not (Test-Path (Join-Path $dir "scripts\code.bat"))) { return $false }
    if (Test-Path (Join-Path $dir "out\main.js")) { return $true }
    if (Test-Path (Join-Path $dir "out\vs\code\electron-main\main.js")) { return $true }
    return $false
}

Export-ModuleMember -Function @(
    "Get-HorizonRoots",
    "Write-HorizonInfo",
    "Write-HorizonWarn",
    "Throw-Horizon",
    "Initialize-HorizonNode",
    "Assert-HorizonWindowsPrereqs",
    "Merge-HorizonProductOverlay",
    "Sync-HorizonContrib",
    "Wire-HorizonContribImport",
    "Test-HorizonCodeOssBuilt",
    "Test-HorizonWindowsBuildTools",
    "Test-HorizonCommand"
)
