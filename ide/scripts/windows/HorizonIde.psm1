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
        WorkbenchDesktopMain = Join-Path $ide "code-oss\src\vs\workbench\workbench.desktop.main.ts"
        CacheDir      = Join-Path $ide ".cache"
        SidecarUrlFile = if ($env:HORIZON_SIDECAR_URL_FILE) { $env:HORIZON_SIDECAR_URL_FILE } else { Join-Path $ide ".cache\horizon-sidecar.url" }
        SidecarPidFile = if ($env:HORIZON_SIDECAR_PID_FILE) { $env:HORIZON_SIDECAR_PID_FILE } else { Join-Path $ide ".cache\horizon-sidecar.pid" }
        SidecarLogFile = if ($env:HORIZON_SIDECAR_LOG_FILE) { $env:HORIZON_SIDECAR_LOG_FILE } else { Join-Path $ide ".cache\horizon-sidecar.log" }
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
    if ($major -gt 22) {
        # Node 24+ V8 headers demand C++20; pinned native deps (tree-sitter 0.22.x) still force /std:c++17.
        Write-HorizonWarn "Node v$ver is newer than the Code-OSS pin (see ide\code-oss\.nvmrc). Native modules such as tree-sitter fail to compile on Node 24+ ('C++20 or later required'). Use Node 22.x."
    }
    # npm.ps1 (npm 10.x) reads $MyInvocation.Statement, which throws under Set-StrictMode -Version Latest.
    $npmVersion = & npm.cmd -v
    Write-HorizonInfo "Node v$ver ($((Get-Command node).Source)), npm $npmVersion"
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

function Get-HorizonVisualStudioInstallPath {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { return $null }
    $found = & $vswhere -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath `
        -latest 2>$null
    if ($found) { return [string]$found }
    return $null
}

function Resolve-HorizonNodeGyp {
    # Returns a node-gyp >= 12 entry point (VS 2026 aware), or $null when only the bundled one exists.
    $nodeDir = Split-Path -Parent (Get-Command node).Source
    $candidates = @(
        (Join-Path $nodeDir "node_modules\node-gyp\bin\node-gyp.js"),
        (Join-Path $env:APPDATA "npm\node_modules\node-gyp\bin\node-gyp.js")
    )
    foreach ($candidate in $candidates) {
        if (-not (Test-Path $candidate)) { continue }
        $version = (& node $candidate --version 2>$null)
        if (-not $version) { continue }
        $major = 0
        [void][int]::TryParse(($version -replace '^v', '' -split '\.')[0], [ref]$major)
        if ($major -ge 12) { return $candidate }
    }
    return $null
}

function Repair-HorizonPreinstallVs2026 {
    param($Roots)
    $preinstall = Join-Path $Roots.CodeOssDir "build\npm\preinstall.js"
    if (-not (Test-Path $preinstall)) {
        Write-HorizonWarn "preinstall.js missing; skipping VS 2026 toolchain patch"
        return
    }
    $text = [System.IO.File]::ReadAllText($preinstall)
    $old = "const supportedVersions = ['2022', '2019'];"
    $new = "const supportedVersions = ['2026', '2022', '2019'];"
    if ($text.Contains("['2026'") -or $text.Contains('["2026"')) {
        Write-HorizonInfo "VS 2026 already accepted in preinstall.js"
        return
    }
    if ($text.Contains($old)) {
        $text = $text.Replace($old, $new)
        [System.IO.File]::WriteAllText($preinstall, $text, [System.Text.UTF8Encoding]::new($false))
        Write-HorizonInfo "patched preinstall.js to accept Visual Studio 2026"
    } else {
        Write-HorizonWarn "could not locate supportedVersions in preinstall.js; leave unchanged"
    }
}

function Repair-HorizonWorkbenchCsp {
    param($Roots)
    # Upstream workbench CSP allows only `'self' https: ws:` on connect-src, which blocks the
    # loopback sidecar (http://127.0.0.1:PORT) that Horizon analyse depends on.
    $htmlDir = Join-Path $Roots.CodeOssDir "src\vs\code\electron-browser\workbench"
    $marker = "http://127.0.0.1:*"
    foreach ($name in @("workbench.html", "workbench-dev.html")) {
        $file = Join-Path $htmlDir $name
        if (-not (Test-Path $file)) {
            Write-HorizonWarn "$name missing; skipping sidecar CSP patch"
            continue
        }
        $text = [System.IO.File]::ReadAllText($file)
        if ($text.Contains($marker)) {
            Write-HorizonInfo "sidecar CSP already allowed in $name"
            continue
        }
        # Whitespace/EOL-agnostic: append the loopback origins just before the directive's `;`.
        $pattern = "(?s)(connect-src\s+'self'\s+https:\s+ws:)(\s*;)"
        $updated = [regex]::Replace($text, $pattern, {
            param($m)
            $indent = "`n`t`t`t`t`t"
            "$($m.Groups[1].Value)${indent}http://127.0.0.1:*${indent}http://localhost:*$($m.Groups[2].Value)"
        }, 1)
        if ($updated -ne $text) {
            [System.IO.File]::WriteAllText($file, $updated, [System.Text.UTF8Encoding]::new($false))
            Write-HorizonInfo "patched $name connect-src to allow the loopback sidecar"
        } else {
            Write-HorizonWarn "could not locate connect-src in $name; leave unchanged"
        }
    }

    # The browser workbench builds its CSP in TypeScript and has the same gap.
    $webServer = Join-Path $Roots.CodeOssDir "src\vs\server\node\webClientServer.ts"
    if (Test-Path $webServer) {
        $text = [System.IO.File]::ReadAllText($webServer)
        if ($text.Contains($marker)) {
            Write-HorizonInfo "sidecar CSP already allowed in webClientServer.ts"
        } else {
            $old = "'connect-src \'self\' ws: wss: https:;'"
            $new = "'connect-src \'self\' ws: wss: https: http://127.0.0.1:* http://localhost:*;'"
            if ($text.Contains($old)) {
                [System.IO.File]::WriteAllText($webServer, $text.Replace($old, $new), [System.Text.UTF8Encoding]::new($false))
                Write-HorizonInfo "patched webClientServer.ts connect-src to allow the loopback sidecar"
            } else {
                Write-HorizonWarn "could not locate connect-src in webClientServer.ts; leave unchanged"
            }
        }
    }
}

function Ensure-HorizonRustAnalyzer {
    param($Roots, [string[]]$ExtraArgs)
    # rust-analyzer is not bundled: a fresh product has an empty extensions dir,
    # so hover / go-to-definition / semantic highlighting are silently absent.
    # Install it from Open VSX (product.json gallery) on first launch.
    #
    # Which directory that is depends on how the product runs. Launched from
    # source (scripts\code.bat) Electron appends "-dev" to product.json's
    # dataFolderName, so extensions live under .horizon-ide-dev, not
    # .horizon-ide. Probing only the packaged path made this re-run the
    # install on *every* launch, and rust-analyzer answers nothing while it is
    # being reinstalled - hover looked simply broken.
    $extDirArgs = @()
    $explicitDir = $null
    if ($ExtraArgs) {
        for ($i = 0; $i -lt $ExtraArgs.Count; $i++) {
            $arg = $ExtraArgs[$i]
            if ($arg -like "--extensions-dir=*") {
                $explicitDir = $arg.Substring("--extensions-dir=".Length).Trim('"')
                $extDirArgs = @("--extensions-dir", $explicitDir)
            } elseif ($arg -eq "--extensions-dir" -and $i + 1 -lt $ExtraArgs.Count) {
                $explicitDir = $ExtraArgs[$i + 1]
                $extDirArgs = @("--extensions-dir", $explicitDir)
            }
        }
    }

    $candidates = @()
    if ($explicitDir) {
        $candidates = @($explicitDir)
    } else {
        $dataFolder = ".horizon-ide"
        $productJson = Join-Path $Roots.CodeOssDir "product.json"
        if (Test-Path $productJson) {
            try {
                $name = (Get-Content $productJson -Raw | ConvertFrom-Json).dataFolderName
                if ($name) { $dataFolder = $name }
            } catch {
                # keep the default; a malformed product.json is reported elsewhere
            }
        }
        # Dev (from source) first: that is what Run.ps1 launches.
        $candidates = @(
            (Join-Path $env:USERPROFILE "$dataFolder-dev\extensions"),
            (Join-Path $env:USERPROFILE "$dataFolder\extensions")
        )
    }

    foreach ($dir in $candidates) {
        if ((Test-Path $dir) -and (Get-ChildItem $dir -Directory -Filter "rust-lang.rust-analyzer-*" -ErrorAction SilentlyContinue)) {
            Write-HorizonInfo "rust-analyzer present in $dir"
            return
        }
    }

    $codeBat = Join-Path $Roots.CodeOssDir "scripts\code.bat"
    Write-HorizonInfo "installing rust-lang.rust-analyzer (first launch) -> $($candidates[0])"
    Push-Location $Roots.CodeOssDir
    try {
        & $codeBat --install-extension rust-lang.rust-analyzer @extDirArgs | ForEach-Object { Write-Host $_ }
        if ($LASTEXITCODE -ne 0) {
            Write-HorizonWarn "rust-analyzer install failed (offline?) - hover/definitions/semantic highlighting unavailable until installed"
        }
    } finally {
        Pop-Location
    }
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
A Visual C++ toolchain is required to compile Code-OSS on Windows.

Prefer: Visual Studio 2026 (or Build Tools) with workload
  Desktop development with C++

Also accepted: Visual Studio 2022 Build Tools with the same workload.

Or see: https://github.com/microsoft/vscode/wiki/How-to-Contribute#prerequisites

If this is too heavy, use WSL2 instead - see ide\WINDOWS.md
"@
    }
    $vsPath = Get-HorizonVisualStudioInstallPath
    if ($vsPath) {
        Write-HorizonInfo "MSVC toolchain: $vsPath"
        # Help older node-gyp / electron tooling that still keys off vs2022_install.
        if (-not $env:vs2022_install -and -not $env:vs2026_install) {
            # VS 2026 installs under ...\18\<edition>, not ...\2026\<edition>.
            if ($vsPath -match '\\2026\\' -or $vsPath -match '\\18\\') {
                $env:vs2026_install = $vsPath
                # Some node-gyp versions only honor vs2022_install - point it at 2026 too.
                $env:vs2022_install = $vsPath
                Write-HorizonInfo "set vs2026_install / vs2022_install -> $vsPath"
            } elseif ($vsPath -match '\\2022\\') {
                $env:vs2022_install = $vsPath
                Write-HorizonInfo "set vs2022_install -> $vsPath"
            }
        }
        if (-not $env:npm_config_msvs_version) {
            if ($env:vs2026_install) {
                $env:npm_config_msvs_version = "2026"
            } elseif ($env:vs2022_install) {
                $env:npm_config_msvs_version = "2022"
            }
            if ($env:npm_config_msvs_version) {
                $env:GYP_MSVS_VERSION = $env:npm_config_msvs_version
            }
        }
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
    Wire-HorizonDesktopContribImport -Roots $Roots
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

    $block = "`r`n// Horizon Map (built-in workbench contrib - not an extension)`r`n$importLine`r`n"
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

function Wire-HorizonDesktopContribImport {
    param($Roots)
    $entrySrc = Join-Path $Roots.ContribSrc "electron-browser\horizon.contribution.ts"
    $entryDst = Join-Path $Roots.ContribDst "electron-browser\horizon.contribution.ts"
    if (-not (Test-Path $entrySrc) -and -not (Test-Path $entryDst)) {
        Write-HorizonInfo "no electron-browser horizon contribution yet; skip desktop import wire"
        return
    }
    $mainTs = $Roots.WorkbenchDesktopMain
    if (-not $mainTs -or -not (Test-Path $mainTs)) {
        Write-HorizonWarn "missing workbench.desktop.main.ts; skip desktop contrib wire"
        return
    }
    $marker = "contrib/horizon/electron-browser/horizon.contribution"
    $importLine = "import './contrib/horizon/electron-browser/horizon.contribution.js';"
    $text = [System.IO.File]::ReadAllText($mainTs)
    if ($text.Contains($marker)) {
        Write-HorizonInfo "Horizon electron sidecar already registered in workbench.desktop.main.ts"
        return
    }
    $block = "`r`n// Horizon Map sidecar (desktop spawn/attach - electron-browser)`r`n$importLine`r`n"
    $needle = "export { main }"
    $idx = $text.IndexOf($needle)
    if ($idx -lt 0) {
        $text = $text.TrimEnd() + "`r`n" + $block
    } else {
        $text = $text.Substring(0, $idx) + $block + "`r`n" + $text.Substring($idx)
    }
    [System.IO.File]::WriteAllText($mainTs, $text, [System.Text.UTF8Encoding]::new($false))
    Write-HorizonInfo "wired Horizon desktop contrib import -> $mainTs"
}

function Test-HorizonCodeOssBuilt {
    param($Roots)
    $dir = $Roots.CodeOssDir
    if (-not (Test-Path (Join-Path $dir "scripts\code.bat"))) { return $false }
    if (Test-Path (Join-Path $dir "out\main.js")) { return $true }
    if (Test-Path (Join-Path $dir "out\vs\code\electron-main\main.js")) { return $true }
    return $false
}

function Get-HorizonContribOutJs {
    param($Roots)
    return (Join-Path $Roots.CodeOssDir "out\vs\workbench\contrib\horizon\browser\horizon.contribution.js")
}

function Test-HorizonBuiltIn {
    param($Roots)
    if (-not (Test-HorizonCodeOssBuilt -Roots $Roots)) { return $false }
    return (Test-Path (Get-HorizonContribOutJs -Roots $Roots))
}

function Ensure-HorizonProductOverlay {
    param($Roots)
    $upstream = Join-Path $Roots.CodeOssDir "product.json.upstream"
    $codeProduct = Join-Path $Roots.CodeOssDir "product.json"
    if (Test-Path $upstream) {
        Copy-Item $upstream $codeProduct -Force
    }
    Merge-HorizonProductOverlay -Roots $Roots
}

function Invoke-HorizonCompileClient {
    param($Roots)
    if (-not (Test-Path (Join-Path $Roots.CodeOssDir "node_modules"))) {
        Throw-Horizon "node_modules missing; run .\ide\scripts\windows\Build.ps1 first (npm ci)"
    }
    Initialize-HorizonNode
    if (-not $env:NODE_OPTIONS) {
        $env:NODE_OPTIONS = "--max-old-space-size=8192"
    }
    Push-Location $Roots.CodeOssDir
    try {
        Write-HorizonInfo "compiling client (npx gulp compile-client) - includes contrib/horizon -> out\"
        # .cmd, not the PowerShell shim: npx.ps1 throws under Set-StrictMode -Version Latest.
        npx.cmd gulp compile-client
        if ($LASTEXITCODE -ne 0) {
            Throw-Horizon "gulp compile-client failed"
        }
    } finally {
        Pop-Location
    }
    $outJs = Get-HorizonContribOutJs -Roots $Roots
    if (-not (Test-Path $outJs)) {
        Throw-Horizon "compile-client finished but Horizon contrib JS missing: $outJs. Did Sync-Contrib run before compile?"
    }
    Write-HorizonInfo "Horizon contrib compiled -> $outJs"
}

function Invoke-HorizonCompileFull {
    param($Roots)
    Initialize-HorizonNode
    if (-not $env:NODE_OPTIONS) {
        $env:NODE_OPTIONS = "--max-old-space-size=8192"
    }
    Push-Location $Roots.CodeOssDir
    try {
        Write-HorizonInfo "compiling Code-OSS (npm run compile - full client + extensions)..."
        npm.cmd run compile
        if ($LASTEXITCODE -ne 0) {
            Throw-Horizon "npm run compile failed. Tip: default Build uses compile-client (HORIZON_COMPILE_MODE=client)."
        }
    } finally {
        Pop-Location
    }
    $outJs = Get-HorizonContribOutJs -Roots $Roots
    if (-not (Test-Path $outJs)) {
        Throw-Horizon "full compile finished but Horizon contrib JS missing: $outJs"
    }
}

function Invoke-HorizonCompileCodeOss {
    param($Roots)
    $mode = if ($env:HORIZON_COMPILE_MODE) { $env:HORIZON_COMPILE_MODE.Trim().ToLowerInvariant() } else { "client" }
    switch ($mode) {
        { $_ -in @("client", "compile-client") } { Invoke-HorizonCompileClient -Roots $Roots }
        { $_ -in @("full", "all") } { Invoke-HorizonCompileFull -Roots $Roots }
        default { Throw-Horizon "unknown HORIZON_COMPILE_MODE='$mode' (use client or full)" }
    }
    if (-not (Test-HorizonCodeOssBuilt -Roots $Roots)) {
        Throw-Horizon "compile finished but electron main entry is missing under out\"
    }
}

function Test-HorizonContribOutStale {
    param($Roots)
    $src = $Roots.ContribSrc
    $outJs = Get-HorizonContribOutJs -Roots $Roots
    if (-not (Test-Path $src)) { return $true }
    if (-not (Test-Path $outJs)) { return $true }
    $outTime = (Get-Item $outJs).LastWriteTimeUtc
    $exts = @(".ts", ".js", ".css", ".html", ".json", ".svg", ".png")
    $newest = Get-ChildItem -Path $src -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $exts -contains $_.Extension.ToLowerInvariant() } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if (-not $newest) { return $false }
    return ($newest.LastWriteTimeUtc -gt $outTime)
}

function Ensure-HorizonContribCompiled {
    param($Roots)
    $mode = if ($env:HORIZON_CONTRIB_SYNC_MODE) { $env:HORIZON_CONTRIB_SYNC_MODE } else { "copy" }
    if (Test-HorizonContribOutStale -Roots $Roots) {
        Write-HorizonWarn "contrib sources newer than out/ (or contrib JS missing) - sync + compile-client"
        Sync-HorizonContrib -Roots $Roots -Mode $mode
        Invoke-HorizonCompileClient -Roots $Roots
    } else {
        Write-HorizonInfo "Horizon contrib out/ is up to date"
    }
}

function Test-HorizonSidecarUrlHealthy {
    param([string]$Url)
    if (-not $Url) { return $false }
    $base = $Url.TrimEnd("/")
    try {
        $resp = Invoke-WebRequest -Uri "$base/api/health" -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        return ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300)
    } catch {
        return $false
    }
}

function Read-HorizonSidecarUrlFile {
    param($Roots)
    $candidates = @(
        $Roots.SidecarUrlFile,
        (Join-Path $Roots.IdeRoot ".cache\sidecar.url")
    )
    foreach ($f in $candidates) {
        if (-not (Test-Path $f)) { continue }
        $url = ((Get-Content -Raw $f) -split "\r?\n" | Select-Object -First 1).Trim()
        if ($url -match '^http://(127\.0\.0\.1|localhost|\[::1\]):\d+/?$') {
            return $url.TrimEnd("/")
        }
    }
    return $null
}

function Write-HorizonSidecarUrlFile {
    param($Roots, [string]$Url)
    $dir = Split-Path -Parent $Roots.SidecarUrlFile
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $clean = $Url.TrimEnd("/")
    Set-Content -Path $Roots.SidecarUrlFile -Value $clean -Encoding ascii
    Write-HorizonInfo "wrote sidecar URL -> $($Roots.SidecarUrlFile) ($clean)"
}

function Resolve-HorizonServerBinary {
    param($Roots)
    if ($env:HORIZON_SERVER_PATH -and (Test-Path $env:HORIZON_SERVER_PATH)) {
        return (Resolve-Path $env:HORIZON_SERVER_PATH).Path
    }
    $candidates = @(
        (Join-Path $Roots.RepoRoot "target\release\horizon-server.exe"),
        (Join-Path $Roots.RepoRoot "target\debug\horizon-server.exe")
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    if (Test-HorizonCommand "horizon-server") {
        return (Get-Command horizon-server).Source
    }
    return $null
}

function Ensure-HorizonSidecar {
    param($Roots)
    $url = $null
    if ($env:HORIZON_SIDECAR_URL) {
        $url = $env:HORIZON_SIDECAR_URL.TrimEnd("/")
        if (Test-HorizonSidecarUrlHealthy -Url $url) {
            Write-HorizonSidecarUrlFile -Roots $Roots -Url $url
            $env:HORIZON_SIDECAR_URL = $url
            Write-HorizonInfo "using existing HORIZON_SIDECAR_URL=$url"
            return
        }
        Write-HorizonWarn "HORIZON_SIDECAR_URL=$url failed /api/health; will try URL file or spawn"
    }

    $fromFile = Read-HorizonSidecarUrlFile -Roots $Roots
    if ($fromFile -and (Test-HorizonSidecarUrlHealthy -Url $fromFile)) {
        $env:HORIZON_SIDECAR_URL = $fromFile
        Write-HorizonInfo "attached sidecar from $($Roots.SidecarUrlFile) ($fromFile)"
        return
    }

    $bin = Resolve-HorizonServerBinary -Roots $Roots
    if (-not $bin) {
        Write-HorizonWarn "horizon-server not found - Map analyse needs: cargo build -p horizon-server --release"
        Write-HorizonWarn "or .\ide\scripts\windows\Run-Sidecar.ps1 (writes $($Roots.SidecarUrlFile))"
        return
    }

    $cacheDir = $Roots.CacheDir
    New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
    if (Test-Path $Roots.SidecarLogFile) { Remove-Item -Force $Roots.SidecarLogFile -ErrorAction SilentlyContinue }

    Write-HorizonInfo "starting sidecar in background: $bin"
    $errLog = Join-Path $Roots.CacheDir "sidecar.err.log"
    if (Test-Path $errLog) { Remove-Item -Force $errLog -ErrorAction SilentlyContinue }
    # stdout and stderr must be different files for Start-Process
    $proc = Start-Process -FilePath $bin -ArgumentList @("--no-open") `
        -RedirectStandardOutput $Roots.SidecarLogFile `
        -RedirectStandardError $errLog `
        -PassThru -WindowStyle Hidden
    Set-Content -Path $Roots.SidecarPidFile -Value $proc.Id -Encoding ascii

    for ($i = 0; $i -lt 50; $i++) {
        if ($proc.HasExited) {
            Write-HorizonWarn "sidecar exited early - see $($Roots.SidecarLogFile) / $errLog"
            return
        }
        if (Test-Path $Roots.SidecarLogFile) {
            $line = Get-Content $Roots.SidecarLogFile -ErrorAction SilentlyContinue |
                Where-Object { $_ -match '^http://(127\.0\.0\.1|localhost|\[::1\]):\d+/?$' } |
                Select-Object -First 1
            if ($line) {
                $url = $line.TrimEnd("/")
                Write-HorizonSidecarUrlFile -Roots $Roots -Url $url
                $env:HORIZON_SIDECAR_URL = $url
                Write-HorizonInfo "sidecar ready pid=$($proc.Id) $url"
                return
            }
        }
        Start-Sleep -Milliseconds 100
    }
    Write-HorizonWarn "sidecar started (pid=$($proc.Id)) but URL not seen yet - check $($Roots.SidecarLogFile)"
}

Export-ModuleMember -Function @(
    "Get-HorizonRoots",
    "Write-HorizonInfo",
    "Write-HorizonWarn",
    "Throw-Horizon",
    "Initialize-HorizonNode",
    "Assert-HorizonWindowsPrereqs",
    "Merge-HorizonProductOverlay",
    "Ensure-HorizonProductOverlay",
    "Sync-HorizonContrib",
    "Wire-HorizonContribImport",
    "Wire-HorizonDesktopContribImport",
    "Test-HorizonCodeOssBuilt",
    "Test-HorizonBuiltIn",
    "Get-HorizonContribOutJs",
    "Invoke-HorizonCompileClient",
    "Invoke-HorizonCompileFull",
    "Invoke-HorizonCompileCodeOss",
    "Test-HorizonContribOutStale",
    "Ensure-HorizonContribCompiled",
    "Ensure-HorizonSidecar",
    "Write-HorizonSidecarUrlFile",
    "Read-HorizonSidecarUrlFile",
    "Resolve-HorizonServerBinary",
    "Test-HorizonWindowsBuildTools",
    "Get-HorizonVisualStudioInstallPath",
    "Ensure-HorizonRustAnalyzer",
    "Repair-HorizonPreinstallVs2026",
    "Repair-HorizonWorkbenchCsp",
    "Resolve-HorizonNodeGyp",
    "Test-HorizonCommand"
)
