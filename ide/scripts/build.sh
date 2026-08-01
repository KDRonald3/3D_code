#!/usr/bin/env bash
# Build: install deps and compile Code-OSS enough to launch via scripts/code.sh.
# On constrained VMs this can take a long time / OOM — use ./ide/scripts/dev-extension.sh
# for extension UI iteration without a full compile.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

horizon_info "Horizon IDE build"
horizon_detect_platform >/dev/null
horizon_info "platform: ${HORIZON_OS}-${HORIZON_ARCH}"

[[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
[[ -f "${HORIZON_CODE_OSS_DIR}/package.json" ]] || horizon_die "Code-OSS package.json missing"

horizon_setup_node
if [[ "${HORIZON_OS}" == "linux" ]]; then
  horizon_require_linux_build_deps
fi

# Re-apply product overlay + extension sync so rebuilds stay branded.
if [[ -f "${HORIZON_CODE_OSS_DIR}/product.json.upstream" ]]; then
  cp "${HORIZON_CODE_OSS_DIR}/product.json.upstream" "${HORIZON_CODE_OSS_DIR}/product.json"
fi
horizon_apply_product_overlay
horizon_sync_extension "${HORIZON_EXTENSION_SYNC_MODE:-copy}"

cd "${HORIZON_CODE_OSS_DIR}"

# npm only (upstream rejects yarn since ~1.100).
export npm_config_fund=false
export npm_config_audit=false

need_install=0
if [[ "${HORIZON_FORCE_NPM_CI:-}" == "1" ]]; then
  need_install=1
elif [[ ! -d node_modules ]]; then
  need_install=1
elif [[ ! -e node_modules/.bin/gulp && ! -e node_modules/gulp ]]; then
  horizon_warn "node_modules looks incomplete (gulp missing); reinstalling"
  need_install=1
fi

if (( need_install )); then
  horizon_info "installing npm dependencies (this takes a while)…"
  if [[ -d node_modules ]] && [[ ! -e node_modules/.bin/gulp && ! -e node_modules/gulp ]]; then
    rm -rf node_modules
  fi
  if [[ -f package-lock.json ]]; then
    npm ci --no-fund --no-audit || horizon_die "npm ci failed — see errors above (common fix on Linux: sudo apt-get install -y libkrb5-dev libx11-dev libxkbfile-dev libsecret-1-dev)"
  else
    npm install --no-fund --no-audit || horizon_die "npm install failed — see errors above"
  fi
else
  horizon_info "node_modules present; skipping npm ci (set HORIZON_FORCE_NPM_CI=1 to reinstall)"
fi

# Compile client + extensions. Full gulp compile is the standard from-source path.
horizon_info "compiling Code-OSS (npm run compile)…"
export NODE_OPTIONS="${NODE_OPTIONS:---max-old-space-size=8192}"
if ! npm run compile; then
  horizon_warn "full compile failed or was interrupted"
  echo
  echo "You can still iterate on the map extension without a full Code-OSS build:"
  echo "  ./ide/scripts/dev-extension.sh [workspace-path]"
  exit 1
fi

[[ -f scripts/code.sh ]] || horizon_die "scripts/code.sh missing from Code-OSS tree"
if [[ ! -f out/main.js && ! -f out/vs/code/electron-main/main.js ]]; then
  horizon_die "compile finished but electron main entry is missing under out/"
fi

horizon_info "build complete"
echo
echo "Launch with:"
echo "  ./ide/scripts/run.sh [workspace-path]"
