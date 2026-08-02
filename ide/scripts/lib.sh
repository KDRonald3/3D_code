#!/usr/bin/env bash
# Shared helpers for Horizon IDE scripts.
# shellcheck disable=SC2034

set -euo pipefail

HORIZON_IDE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HORIZON_REPO_ROOT="$(cd "${HORIZON_IDE_ROOT}/.." && pwd)"
HORIZON_PRODUCT_JSON="${HORIZON_IDE_ROOT}/product/product.json"
HORIZON_VSCODE_REF_FILE="${HORIZON_IDE_ROOT}/product/vscode-ref.txt"
# Code-OSS checkout lives at ide/code-oss/ (gitignored).
HORIZON_CODE_OSS_DIR="${HORIZON_IDE_ROOT}/code-oss"
# Back-compat alias used by earlier drafts.
HORIZON_VENDOR_DIR="${HORIZON_CODE_OSS_DIR}"
# Product surface: workbench contrib (synced into src/vs/workbench/contrib/horizon).
HORIZON_CONTRIB_SRC="${HORIZON_IDE_ROOT}/contrib/horizon"
HORIZON_CONTRIB_DST="${HORIZON_CODE_OSS_DIR}/src/vs/workbench/contrib/horizon"
HORIZON_WORKBENCH_COMMON_MAIN="${HORIZON_CODE_OSS_DIR}/src/vs/workbench/workbench.common.main.ts"
# Deprecated extension package (not the product surface).
HORIZON_EXTENSION_SRC="${HORIZON_IDE_ROOT}/extensions/horizon-map"
HORIZON_EXTENSION_DST="${HORIZON_CODE_OSS_DIR}/extensions/horizon-map"
HORIZON_PREBUILT_DIR="${HORIZON_IDE_ROOT}/.cache/prebuilt"
# Sidecar listen URL written by run-sidecar / run.sh for IDE attach (gitignored under .cache/).
# Canonical name: horizon-sidecar.url (also accept legacy sidecar.url when reading).
HORIZON_SIDECAR_URL_FILE="${HORIZON_SIDECAR_URL_FILE:-${HORIZON_IDE_ROOT}/.cache/horizon-sidecar.url}"
HORIZON_SIDECAR_PID_FILE="${HORIZON_SIDECAR_PID_FILE:-${HORIZON_IDE_ROOT}/.cache/horizon-sidecar.pid}"
HORIZON_SIDECAR_LOG_FILE="${HORIZON_SIDECAR_LOG_FILE:-${HORIZON_IDE_ROOT}/.cache/horizon-sidecar.log}"
HORIZON_WORKBENCH_DESKTOP_MAIN="${HORIZON_CODE_OSS_DIR}/src/vs/workbench/workbench.desktop.main.ts"
# Compile mode for build.sh / compile helpers: client (default) | full
# client = npx gulp compile-client (workbench src → out/, includes contrib/horizon)
# full   = npm run compile (client + extensions; heavier / flakier)
HORIZON_COMPILE_MODE="${HORIZON_COMPILE_MODE:-client}"

# Pinned upstream tag/commit (override with HORIZON_VSCODE_REF).
if [[ -n "${HORIZON_VSCODE_REF:-}" ]]; then
  :
elif [[ -f "${HORIZON_VSCODE_REF_FILE}" ]]; then
  HORIZON_VSCODE_REF="$(tr -d '[:space:]' < "${HORIZON_VSCODE_REF_FILE}")"
else
  HORIZON_VSCODE_REF="1.105.1"
fi

HORIZON_VSCODE_REPO="${HORIZON_VSCODE_REPO:-https://github.com/microsoft/vscode.git}"

horizon_die() {
  echo "error: $*" >&2
  exit 1
}

horizon_info() {
  echo "==> $*" >&2
}

horizon_warn() {
  echo "warning: $*" >&2
}

horizon_detect_platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "${arch}" in
    x86_64|amd64) arch="x64" ;;
    aarch64|arm64) arch="arm64" ;;
    armv7l) arch="armhf" ;;
    *) horizon_warn "unrecognized arch '${arch}', passing through" ;;
  esac
  HORIZON_OS="${os}"
  HORIZON_ARCH="${arch}"
  echo "${os}-${arch}"
}

# Prefer an nvm-managed Node that satisfies vscode's Node 22.15.1+ check.
# /exec-daemon/node (or similar shims) can shadow nvm on PATH — always prepend.
horizon_setup_node() {
  local nvm_dir candidate want_major=22 want_min_minor=15
  nvm_dir="${NVM_DIR:-${HOME}/.nvm}"

  if [[ -s "${nvm_dir}/nvm.sh" ]]; then
    # shellcheck source=/dev/null
    . "${nvm_dir}/nvm.sh"
    if command -v nvm >/dev/null 2>&1; then
      if nvm ls 22 >/dev/null 2>&1; then
        nvm use 22 >/dev/null 2>&1 || true
      fi
      candidate="$(nvm which 22 2>/dev/null || true)"
      if [[ -n "${candidate}" && -x "${candidate}" ]]; then
        export PATH="$(dirname "${candidate}"):${PATH}"
      fi
    fi
  fi

  command -v node >/dev/null 2>&1 || horizon_die "node is required (Node.js >= 22.15.1)"
  command -v npm >/dev/null 2>&1 || horizon_die "npm is required"

  local ver major minor patch
  ver="$(node -v | sed 's/^v//')"
  IFS=. read -r major minor patch <<<"${ver}"
  major="${major:-0}"
  minor="${minor:-0}"
  patch="${patch:-0}"

  if (( major < want_major )) || \
     (( major == want_major && minor < want_min_minor )) || \
     (( major == want_major && minor == want_min_minor && patch < 1 )); then
    if [[ -z "${VSCODE_SKIP_NODE_VERSION_CHECK:-}" ]]; then
      horizon_die "Node.js >= 22.15.1 required (found v${ver}). Install via nvm, or set VSCODE_SKIP_NODE_VERSION_CHECK=1 to bypass."
    fi
    horizon_warn "Node.js v${ver} is below 22.15.1; continuing because VSCODE_SKIP_NODE_VERSION_CHECK is set"
  fi

  horizon_info "Node $(node -v) ($(command -v node)), npm $(npm -v)"
}

horizon_require_linux_build_deps() {
  local missing=()
  for bin in python3 make g++ pkg-config; do
    command -v "${bin}" >/dev/null 2>&1 || missing+=("${bin}")
  done
  if ((${#missing[@]})); then
    horizon_die "missing build tools: ${missing[*]} (install build-essential, python3, pkg-config)"
  fi
  # Native module kerberos needs GSSAPI headers (libkrb5-dev on Debian/Ubuntu).
  if [[ ! -f /usr/include/gssapi/gssapi.h && ! -f /usr/include/gssapi.h ]]; then
    horizon_die "missing Kerberos/GSSAPI headers (gssapi/gssapi.h). On Debian/Ubuntu: sudo apt-get install -y libkrb5-dev"
  fi
}

horizon_apply_product_overlay() {
  local code_product="${HORIZON_CODE_OSS_DIR}/product.json"
  local overlay="${HORIZON_PRODUCT_JSON}"

  [[ -f "${code_product}" ]] || horizon_die "missing Code-OSS product.json at ${code_product} (run bootstrap first)"
  [[ -f "${overlay}" ]] || horizon_die "missing Horizon product overlay at ${overlay}"

  python3 - "${code_product}" "${overlay}" <<'PY'
import json, sys
vendor_path, overlay_path = sys.argv[1], sys.argv[2]
with open(vendor_path, encoding="utf-8") as f:
    product = json.load(f)
with open(overlay_path, encoding="utf-8") as f:
    overlay = json.load(f)
# Top-level overlay merge; preserve upstream builtInExtensions unless overridden.
for key, value in overlay.items():
    product[key] = value
with open(vendor_path, "w", encoding="utf-8") as f:
    json.dump(product, f, indent="\t", ensure_ascii=False)
    f.write("\n")
print(f"applied product overlay -> {vendor_path}")
PY
}

# Upstream vscode 1.105.x preinstall only auto-detects VS 2019/2022.
# Prefer VS 2026 when present (current Windows toolchain).
horizon_patch_preinstall_vs2026() {
  local preinstall="${HORIZON_CODE_OSS_DIR}/build/npm/preinstall.js"
  [[ -f "${preinstall}" ]] || {
    horizon_warn "preinstall.js missing; skipping VS 2026 toolchain patch"
    return 0
  }
  python3 - "${preinstall}" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
old = "const supportedVersions = ['2022', '2019'];"
new = "const supportedVersions = ['2026', '2022', '2019'];"
if "['2026'" in text or '["2026"' in text:
    print(f"VS 2026 already accepted in {path}")
elif old in text:
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"patched {path} to accept Visual Studio 2026")
else:
    print(f"warning: could not locate supportedVersions in {path}; leave unchanged", file=sys.stderr)
    sys.exit(0)
PY
}

# Upstream workbench CSP allows only `'self' https: ws:` on connect-src, which blocks the
# loopback sidecar (http://127.0.0.1:PORT) that Horizon analyse depends on.
horizon_patch_workbench_csp() {
  local html_dir="${HORIZON_CODE_OSS_DIR}/src/vs/code/electron-browser/workbench"
  local name
  for name in workbench.html workbench-dev.html; do
    local file="${html_dir}/${name}"
    [[ -f "${file}" ]] || {
      horizon_warn "${name} missing; skipping sidecar CSP patch"
      continue
    }
    python3 - "${file}" <<'PY'
from pathlib import Path
import re
import sys
path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
if "http://127.0.0.1:*" in text:
    print(f"sidecar CSP already allowed in {path.name}")
    sys.exit(0)
indent = "\n\t\t\t\t\t"
pattern = re.compile(r"(connect-src\s+'self'\s+https:\s+ws:)(\s*;)", re.S)
updated, n = pattern.subn(
    lambda m: f"{m.group(1)}{indent}http://127.0.0.1:*{indent}http://localhost:*{m.group(2)}",
    text,
    count=1,
)
if n:
    path.write_text(updated, encoding="utf-8")
    print(f"patched {path.name} connect-src to allow the loopback sidecar")
else:
    print(f"warning: could not locate connect-src in {path.name}; leave unchanged", file=sys.stderr)
PY
  done

  # The browser workbench builds its CSP in TypeScript and has the same gap.
  local web_server="${HORIZON_CODE_OSS_DIR}/src/vs/server/node/webClientServer.ts"
  [[ -f "${web_server}" ]] || return 0
  python3 - "${web_server}" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
if "http://127.0.0.1:*" in text:
    print(f"sidecar CSP already allowed in {path.name}")
    sys.exit(0)
old = "'connect-src \\'self\\' ws: wss: https:;'"
new = "'connect-src \\'self\\' ws: wss: https: http://127.0.0.1:* http://localhost:*;'"
if old in text:
    path.write_text(text.replace(old, new, 1), encoding="utf-8")
    print(f"patched {path.name} connect-src to allow the loopback sidecar")
else:
    print(f"warning: could not locate connect-src in {path.name}; leave unchanged", file=sys.stderr)
PY
}

# rust-analyzer is not bundled: a fresh product has an empty extensions dir, so
# hover / go-to-definition / semantic highlighting are silently absent. Install
# from Open VSX (product.json gallery) on first launch.
horizon_ensure_rust_analyzer() {
  local ext_dir="${HOME}/.horizon-ide/extensions"
  if compgen -G "${ext_dir}/rust-lang.rust-analyzer-*" > /dev/null 2>&1; then
    horizon_info "rust-analyzer present in ${ext_dir}"
    return 0
  fi
  horizon_info "installing rust-lang.rust-analyzer (first launch) -> ${ext_dir}"
  if ! (cd "${HORIZON_CODE_OSS_DIR}" && ./scripts/code.sh --install-extension rust-lang.rust-analyzer); then
    horizon_warn "rust-analyzer install failed (offline?) — hover/definitions/semantic highlighting unavailable until installed"
  fi
}

horizon_sync_extension() {
  # DEPRECATED: never copy the extension package into Code-OSS — it conflicts
  # with the built-in workbench contrib (duplicate commands / viewlet ids).
  local dst="${HORIZON_EXTENSION_DST}"
  horizon_warn "horizon-map extension sync is disabled — product surface is ide/contrib/horizon"
  if [[ -e "${dst}" || -L "${dst}" ]]; then
    rm -rf "${dst}"
    horizon_info "removed leftover ${dst}"
  fi
  return 0
}

# Sync ide/contrib/horizon into Code-OSS workbench contrib and register the import.
horizon_sync_contrib() {
  local mode="${1:-copy}" # copy | link
  local src="${HORIZON_CONTRIB_SRC}"
  local dst="${HORIZON_CONTRIB_DST}"
  local main_ts="${HORIZON_WORKBENCH_COMMON_MAIN}"

  [[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS checkout missing at ${HORIZON_CODE_OSS_DIR} (run bootstrap first)"
  [[ -d "${HORIZON_CODE_OSS_DIR}/src/vs/workbench/contrib" ]] || horizon_die "Code-OSS workbench contrib/ missing"

  if [[ ! -d "${src}" ]]; then
    horizon_die "contrib source missing: ${src}"
  fi
  if [[ ! -f "${src}/browser/horizon.contribution.ts" ]]; then
    horizon_die "contrib entry missing: ${src}/browser/horizon.contribution.ts"
  fi

  mkdir -p "$(dirname "${dst}")"
  if [[ -e "${dst}" || -L "${dst}" ]]; then
    rm -rf "${dst}"
  fi

  case "${mode}" in
    link)
      ln -s "${src}" "${dst}"
      horizon_info "linked ${src} -> ${dst}"
      ;;
    copy|*)
      if command -v rsync >/dev/null 2>&1; then
        mkdir -p "${dst}"
        rsync -a --delete \
          --exclude .git \
          --exclude node_modules \
          "${src}/" "${dst}/"
      else
        mkdir -p "${dst}"
        cp -a "${src}/." "${dst}/"
      fi
      horizon_info "copied ${src} -> ${dst}"
      ;;
  esac

  horizon_wire_contrib_import
  horizon_wire_desktop_contrib_import
}

# Ensure workbench.common.main.ts imports the Horizon Map contribution.
horizon_wire_contrib_import() {
  local main_ts="${HORIZON_WORKBENCH_COMMON_MAIN}"
  local marker="contrib/horizon/browser/horizon.contribution"
  local import_line="import './contrib/horizon/browser/horizon.contribution.js';"

  [[ -f "${main_ts}" ]] || horizon_die "missing ${main_ts}"

  if grep -qF "${marker}" "${main_ts}"; then
    horizon_info "Horizon contrib already registered in workbench.common.main.ts"
    return 0
  fi

  python3 - "${main_ts}" "${import_line}" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1])
import_line = sys.argv[2]
text = path.read_text(encoding="utf-8")
block = (
    "\n// Horizon Map (built-in workbench contrib — not an extension)\n"
    f"{import_line}\n"
)
# Prefer inserting before the contributions endregion.
needle = "//#endregion"
idx = text.rfind(needle)
# Find the contributions region end: last //#endregion after "workbench contributions"
contrib_hdr = text.find("--- workbench contributions")
if contrib_hdr != -1:
    idx = text.find(needle, contrib_hdr)
if idx == -1:
    path.write_text(text.rstrip() + "\n" + block, encoding="utf-8")
else:
    text = text[:idx] + block + "\n" + text[idx:]
    path.write_text(text, encoding="utf-8")
print(f"wired Horizon contrib import -> {path}")
PY
}

# Ensure workbench.desktop.main.ts imports the Horizon electron-browser sidecar contribution.
# Skips cleanly when electron-browser sources are not present yet.
horizon_wire_desktop_contrib_import() {
  local main_ts="${HORIZON_WORKBENCH_DESKTOP_MAIN}"
  local marker="contrib/horizon/electron-browser/horizon.contribution"
  local import_line="import './contrib/horizon/electron-browser/horizon.contribution.js';"
  local entry_src="${HORIZON_CONTRIB_SRC}/electron-browser/horizon.contribution.ts"
  local entry_dst="${HORIZON_CONTRIB_DST}/electron-browser/horizon.contribution.ts"

  if [[ ! -f "${entry_src}" && ! -f "${entry_dst}" ]]; then
    horizon_info "no electron-browser horizon contribution yet; skip desktop import wire"
    return 0
  fi

  [[ -f "${main_ts}" ]] || {
    horizon_warn "missing ${main_ts}; skip desktop contrib wire"
    return 0
  }

  if grep -qF "${marker}" "${main_ts}"; then
    horizon_info "Horizon electron sidecar already registered in workbench.desktop.main.ts"
    return 0
  fi

  python3 - "${main_ts}" "${import_line}" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1])
import_line = sys.argv[2]
text = path.read_text(encoding="utf-8")
block = (
    "\n// Horizon Map sidecar (desktop spawn/attach — electron-browser)\n"
    f"{import_line}\n"
)
needle = "export { main }"
idx = text.find(needle)
if idx == -1:
    path.write_text(text.rstrip() + "\n" + block, encoding="utf-8")
else:
    text = text[:idx] + block + "\n" + text[idx:]
    path.write_text(text, encoding="utf-8")
print(f"wired Horizon desktop contrib import -> {path}")
PY
}


# Resolve a VS Code / VSCodium / code-oss CLI for Extension Development Host.
horizon_find_editor_cli() {
  local candidate
  for candidate in \
    "${HORIZON_CODE_CLI:-}" \
    code \
    codium \
    code-oss \
    code-insiders; do
    [[ -z "${candidate}" ]] && continue
    if command -v "${candidate}" >/dev/null 2>&1; then
      command -v "${candidate}"
      return 0
    fi
  done
  # Cached VSCodium binary from a previous download.
  if [[ -x "${HORIZON_PREBUILT_DIR}/codium/bin/codium" ]]; then
    echo "${HORIZON_PREBUILT_DIR}/codium/bin/codium"
    return 0
  fi
  if [[ -x "${HORIZON_PREBUILT_DIR}/VSCodium/bin/codium" ]]; then
    echo "${HORIZON_PREBUILT_DIR}/VSCodium/bin/codium"
    return 0
  fi
  return 1
}

# Resolve a VSCodium release tag that publishes VSCodium-linux-<arch>-*.tar.gz.
horizon_resolve_vscodium_version() {
  if [[ -n "${HORIZON_VSCODIUM_VERSION:-}" ]]; then
    echo "${HORIZON_VSCODIUM_VERSION}"
    return 0
  fi
  # Prefer a cached pin from a previous successful download.
  if [[ -f "${HORIZON_PREBUILT_DIR}/vscodium-version.txt" ]]; then
    tr -d '[:space:]' < "${HORIZON_PREBUILT_DIR}/vscodium-version.txt"
    return 0
  fi
  # Query GitHub for the newest release that has the desktop tarball.
  local api_json version
  api_json="$(mktemp)"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 2 \
      "https://api.github.com/repos/VSCodium/vscodium/releases?per_page=15" \
      -o "${api_json}" 2>/dev/null || true
  fi
  version="$(python3 - "${api_json}" <<'PY' 2>/dev/null || true
import json, sys
path = sys.argv[1]
try:
    with open(path, encoding="utf-8") as f:
        releases = json.load(f)
except Exception:
    sys.exit(1)
if not isinstance(releases, list):
    sys.exit(1)
for rel in releases:
    tag = rel.get("tag_name") or ""
    for asset in rel.get("assets") or []:
        name = asset.get("name") or ""
        if name.startswith("VSCodium-linux-x64-") and name.endswith(".tar.gz"):
            print(tag)
            raise SystemExit(0)
sys.exit(1)
PY
)"
  rm -f "${api_json}"
  if [[ -n "${version}" ]]; then
    echo "${version}"
    return 0
  fi
  # Known-good fallback (desktop tarball exists on this tag).
  echo "1.110.11631"
}

# Best-effort: download a portable VSCodium Linux tarball for EDH testing.
horizon_ensure_prebuilt_editor() {
  local cli
  if cli="$(horizon_find_editor_cli)"; then
    echo "${cli}"
    return 0
  fi

  horizon_detect_platform >/dev/null
  if [[ "${HORIZON_OS}" != "linux" ]]; then
    horizon_warn "no editor CLI found and automatic prebuilt download only supports Linux"
    return 1
  fi

  local version arch_tag asset url dest extract_dir
  version="$(horizon_resolve_vscodium_version)"
  case "${HORIZON_ARCH}" in
    x64) arch_tag="x64" ;;
    arm64) arch_tag="arm64" ;;
    *)
      horizon_warn "unsupported arch for VSCodium prebuilt: ${HORIZON_ARCH}"
      return 1
      ;;
  esac

  asset="VSCodium-linux-${arch_tag}-${version}.tar.gz"
  url="https://github.com/VSCodium/vscodium/releases/download/${version}/${asset}"
  dest="${HORIZON_PREBUILT_DIR}/${asset}"
  extract_dir="${HORIZON_PREBUILT_DIR}/codium"

  mkdir -p "${HORIZON_PREBUILT_DIR}"
  if [[ ! -f "${dest}" ]]; then
    horizon_info "downloading VSCodium ${version} (${arch_tag}) for Extension Development Host…"
    if command -v curl >/dev/null 2>&1; then
      curl -fL --retry 3 -o "${dest}" "${url}" || {
        rm -f "${dest}"
        horizon_warn "VSCodium download failed from ${url}"
        return 1
      }
    elif command -v wget >/dev/null 2>&1; then
      wget -O "${dest}" "${url}" || {
        rm -f "${dest}"
        horizon_warn "VSCodium download failed from ${url}"
        return 1
      }
    else
      horizon_warn "curl/wget required to download a prebuilt editor"
      return 1
    fi
  fi
  echo "${version}" > "${HORIZON_PREBUILT_DIR}/vscodium-version.txt"

  mkdir -p "${extract_dir}"
  if [[ ! -x "${extract_dir}/bin/codium" && ! -x "${extract_dir}/codium" ]]; then
    horizon_info "extracting ${asset}…"
    # Clear stale extract so nested layouts do not confuse the finder.
    find "${extract_dir}" -mindepth 1 -maxdepth 1 -exec rm -rf {} + 2>/dev/null || true
    tar -xzf "${dest}" -C "${extract_dir}"
  fi

  if [[ -x "${extract_dir}/bin/codium" ]]; then
    echo "${extract_dir}/bin/codium"
    return 0
  fi
  if [[ -x "${extract_dir}/codium" ]]; then
    echo "${extract_dir}/codium"
    return 0
  fi
  local nested
  nested="$(find "${extract_dir}" -maxdepth 3 -type f -name codium 2>/dev/null | head -n1 || true)"
  if [[ -n "${nested}" && -x "${nested}" ]]; then
    echo "${nested}"
    return 0
  fi

  horizon_warn "extracted VSCodium but could not locate the codium binary"
  return 1
}

horizon_abspath() {
  local target="$1"
  if [[ -d "${target}" ]]; then
    (cd "${target}" && pwd)
  elif [[ -e "${target}" ]]; then
    echo "$(cd "$(dirname "${target}")" && pwd)/$(basename "${target}")"
  else
    echo "${target}"
  fi
}

# True when Code-OSS has been compiled far enough to launch via scripts/code.sh.
horizon_code_oss_built() {
  [[ -d "${HORIZON_CODE_OSS_DIR}" ]] || return 1
  [[ -f "${HORIZON_CODE_OSS_DIR}/scripts/code.sh" ]] || return 1
  # A bare/partial out/ directory is not enough — require the electron main entry.
  [[ -f "${HORIZON_CODE_OSS_DIR}/out/main.js" ]] \
    || [[ -f "${HORIZON_CODE_OSS_DIR}/out/vs/code/electron-main/main.js" ]] \
    || return 1
  return 0
}

# Compiled Horizon contrib JS under out/ (produced by gulp compile-client / compile).
horizon_contrib_out_js() {
  echo "${HORIZON_CODE_OSS_DIR}/out/vs/workbench/contrib/horizon/browser/horizon.contribution.js"
}

# True when Electron main + Horizon contrib JS are present under out/.
horizon_horizon_built_in() {
  horizon_code_oss_built || return 1
  [[ -f "$(horizon_contrib_out_js)" ]] || return 1
  return 0
}

# Re-apply product.json overlay from upstream backup (idempotent branding).
horizon_ensure_product_overlay() {
  if [[ -f "${HORIZON_CODE_OSS_DIR}/product.json.upstream" ]]; then
    cp "${HORIZON_CODE_OSS_DIR}/product.json.upstream" "${HORIZON_CODE_OSS_DIR}/product.json"
  fi
  horizon_apply_product_overlay
}

# Compile workbench TypeScript (src → out/), including contrib/horizon.
# Prefer this over full `npm run compile` for product iteration — extensions
# compile is heavier and more failure-prone; the Map lives in the client tree.
horizon_compile_client() {
  [[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
  [[ -d "${HORIZON_CODE_OSS_DIR}/node_modules" ]] || horizon_die "node_modules missing; run ./ide/scripts/build.sh first (npm ci)"
  horizon_setup_node
  export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=8192}"
  (
    cd "${HORIZON_CODE_OSS_DIR}"
    horizon_info "compiling client (npx gulp compile-client) — includes contrib/horizon → out/"
    npx gulp compile-client
  ) || horizon_die "gulp compile-client failed"
  local out_js
  out_js="$(horizon_contrib_out_js)"
  [[ -f "${out_js}" ]] || horizon_die "compile-client finished but Horizon contrib JS missing: ${out_js} (did sync-contrib run? re-run ./ide/scripts/build.sh)"
  horizon_info "Horizon contrib compiled -> ${out_js}"
}

# Full upstream compile (client + extensions). Use HORIZON_COMPILE_MODE=full.
horizon_compile_full() {
  [[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
  horizon_setup_node
  export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=8192}"
  (
    cd "${HORIZON_CODE_OSS_DIR}"
    horizon_info "compiling Code-OSS (npm run compile — full client + extensions)…"
    npm run compile
  ) || horizon_die "npm run compile failed (tip: default build uses HORIZON_COMPILE_MODE=client)"
  local out_js
  out_js="$(horizon_contrib_out_js)"
  [[ -f "${out_js}" ]] || horizon_die "full compile finished but Horizon contrib JS missing: ${out_js}"
}

# Dispatch compile by HORIZON_COMPILE_MODE (client|full). Default: client.
horizon_compile_code_oss() {
  local mode="${HORIZON_COMPILE_MODE:-client}"
  case "${mode}" in
    client|compile-client)
      horizon_compile_client
      ;;
    full|all)
      horizon_compile_full
      ;;
    *)
      horizon_die "unknown HORIZON_COMPILE_MODE='${mode}' (use client or full)"
      ;;
  esac
  if ! horizon_code_oss_built; then
    horizon_die "compile finished but electron main entry is missing under out/"
  fi
}

# Return 0 when contrib sources are newer than compiled out/ JS (or out JS missing).
horizon_contrib_out_stale() {
  local out_js src
  out_js="$(horizon_contrib_out_js)"
  src="${HORIZON_CONTRIB_SRC}"
  [[ -d "${src}" ]] || return 0
  python3 - "${src}" "${out_js}" <<'PY'
import sys
from pathlib import Path
src, out_js = Path(sys.argv[1]), Path(sys.argv[2])
if not out_js.is_file():
    sys.exit(0)  # stale / missing
out_mtime = out_js.stat().st_mtime
suffixes = {".ts", ".js", ".css", ".html", ".json", ".svg", ".png", ".woff", ".woff2"}
newest = 0.0
for p in src.rglob("*"):
    if not p.is_file() or p.suffix.lower() not in suffixes:
        continue
    try:
        newest = max(newest, p.stat().st_mtime)
    except OSError:
        pass
sys.exit(0 if newest > out_mtime else 1)
PY
}

# Sync contrib + compile-client when out/ is missing or older than sources.
horizon_ensure_contrib_compiled() {
  local mode="${HORIZON_CONTRIB_SYNC_MODE:-copy}"
  if horizon_contrib_out_stale; then
    horizon_warn "contrib sources newer than out/ (or contrib JS missing) — sync + compile-client"
    horizon_sync_contrib "${mode}"
    horizon_compile_client
  else
    horizon_info "Horizon contrib out/ is up to date"
  fi
}

horizon_sidecar_url_healthy() {
  local url="${1:-}"
  [[ -n "${url}" ]] || return 1
  url="${url%/}"
  if command -v curl >/dev/null 2>&1; then
    curl -fsS --max-time 2 "${url}/api/health" >/dev/null 2>&1
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O - --timeout=2 "${url}/api/health" >/dev/null 2>&1
  else
    return 1
  fi
}

horizon_read_sidecar_url_file() {
  local candidates=("${HORIZON_SIDECAR_URL_FILE}" "${HORIZON_IDE_ROOT}/.cache/sidecar.url")
  local f url
  for f in "${candidates[@]}"; do
    [[ -f "${f}" ]] || continue
    url="$(tr -d '[:space:]' < "${f}" | head -n1)"
    if [[ "${url}" == http://127.0.0.1:* || "${url}" == http://localhost:* || "${url}" == http://[::1]:* ]]; then
      echo "${url%/}"
      return 0
    fi
  done
  return 1
}

horizon_write_sidecar_url_file() {
  local url="${1%/}"
  mkdir -p "$(dirname "${HORIZON_SIDECAR_URL_FILE}")"
  printf '%s\n' "${url}" > "${HORIZON_SIDECAR_URL_FILE}"
  horizon_info "wrote sidecar URL -> ${HORIZON_SIDECAR_URL_FILE} (${url})"
}

# Resolve horizon-server binary (same search order as run-sidecar.sh).
horizon_find_sidecar_bin() {
  if [[ -n "${HORIZON_SERVER_PATH:-}" ]]; then
    if [[ -x "${HORIZON_SERVER_PATH}" ]]; then
      echo "${HORIZON_SERVER_PATH}"
      return 0
    fi
    return 1
  fi
  local release debug
  release="${HORIZON_REPO_ROOT}/target/release/horizon-server"
  debug="${HORIZON_REPO_ROOT}/target/debug/horizon-server"
  if [[ -x "${release}" ]]; then
    echo "${release}"
    return 0
  fi
  if [[ -x "${debug}" ]]; then
    echo "${debug}"
    return 0
  fi
  if command -v horizon-server >/dev/null 2>&1; then
    command -v horizon-server
    return 0
  fi
  return 1
}

# Ensure a loopback sidecar is reachable; start one in background if needed.
# Coordinates via ide/.cache/horizon-sidecar.url (also written by run-sidecar.sh).
# Best-effort: missing binary only warns so the IDE can still launch.
horizon_ensure_sidecar() {
  local url=""
  if [[ -n "${HORIZON_SIDECAR_URL:-}" ]]; then
    url="${HORIZON_SIDECAR_URL%/}"
    if horizon_sidecar_url_healthy "${url}"; then
      horizon_write_sidecar_url_file "${url}"
      export HORIZON_SIDECAR_URL="${url}"
      horizon_info "using existing HORIZON_SIDECAR_URL=${url}"
      return 0
    fi
    horizon_warn "HORIZON_SIDECAR_URL=${url} failed /api/health; will try URL file or spawn"
  fi

  if url="$(horizon_read_sidecar_url_file)"; then
    if horizon_sidecar_url_healthy "${url}"; then
      export HORIZON_SIDECAR_URL="${url}"
      horizon_info "attached sidecar from ${HORIZON_SIDECAR_URL_FILE} (${url})"
      return 0
    fi
    horizon_warn "stale sidecar URL file (${url}); starting a new sidecar if possible"
  fi

  local bin=""
  if ! bin="$(horizon_find_sidecar_bin)"; then
    horizon_warn "horizon-server not found — Map analyse needs: cargo build -p horizon-server --release"
    horizon_warn "or ./ide/scripts/run-sidecar.sh (writes ${HORIZON_SIDECAR_URL_FILE})"
    return 0
  fi

  mkdir -p "$(dirname "${HORIZON_SIDECAR_LOG_FILE}")"
  : > "${HORIZON_SIDECAR_LOG_FILE}"
  horizon_info "starting sidecar in background: ${bin}"
  # Detach so IDE exit does not kill analyse immediately; URL file is the handoff.
  nohup "${bin}" --no-open >>"${HORIZON_SIDECAR_LOG_FILE}" 2>&1 &
  local pid=$!
  echo "${pid}" > "${HORIZON_SIDECAR_PID_FILE}"

  local i line
  for i in $(seq 1 50); do
    if ! kill -0 "${pid}" 2>/dev/null; then
      horizon_warn "sidecar exited early — see ${HORIZON_SIDECAR_LOG_FILE}"
      return 0
    fi
    line="$(grep -E '^http://(127\.0\.0\.1|localhost|\[::1\]):[0-9]+/?$' "${HORIZON_SIDECAR_LOG_FILE}" 2>/dev/null | head -n1 || true)"
    if [[ -n "${line}" ]]; then
      url="${line%/}"
      horizon_write_sidecar_url_file "${url}"
      export HORIZON_SIDECAR_URL="${url}"
      horizon_info "sidecar ready pid=${pid} ${url}"
      return 0
    fi
    sleep 0.1
  done
  horizon_warn "sidecar started (pid=${pid}) but URL not seen yet — check ${HORIZON_SIDECAR_LOG_FILE}"
  return 0
}
