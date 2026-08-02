#!/usr/bin/env bash
# Build Horizon IDE: sync contrib, install deps, compile client into out/.
#
# Always:
#   1. Re-apply product.json overlay (branding)
#   2. Patch VS 2026 into Code-OSS preinstall.js (Windows toolchain)
#   3. Sync ide/contrib/horizon → code-oss/src/vs/workbench/contrib/horizon
#   4. npm ci when needed
#   5. Compile so Horizon TS lands in out/ (Map buttons, analyse, folder picker)
#
# Compile mode (HORIZON_COMPILE_MODE):
#   client (default) — `npx gulp compile-client`
#       Compiles workbench src → out/, including contrib/horizon.
#       Prefer this: full `npm run compile` also builds extensions and is flakier.
#   full — `npm run compile` (client + extensions)
#
# After a successful build, `./ide/scripts/run.sh` launches the full product.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

horizon_info "Horizon IDE build"
horizon_detect_platform >/dev/null
horizon_info "platform: ${HORIZON_OS}-${HORIZON_ARCH}"
horizon_info "compile mode: ${HORIZON_COMPILE_MODE:-client}"

[[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
[[ -f "${HORIZON_CODE_OSS_DIR}/package.json" ]] || horizon_die "Code-OSS package.json missing"

horizon_setup_node
if [[ "${HORIZON_OS}" == "linux" ]]; then
  horizon_require_linux_build_deps
fi

# --- Product surface: brand + toolchain patch + always sync contrib -----------
horizon_ensure_product_overlay
horizon_patch_preinstall_vs2026
horizon_patch_workbench_csp
horizon_sync_contrib "${HORIZON_CONTRIB_SYNC_MODE:-copy}"

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

# Default: compile-client so contrib/horizon TypeScript is emitted under out/.
# Full compile: HORIZON_COMPILE_MODE=full ./ide/scripts/build.sh
horizon_compile_code_oss

[[ -f scripts/code.sh ]] || horizon_die "scripts/code.sh missing from Code-OSS tree"
if ! horizon_horizon_built_in; then
  horizon_die "build finished but Horizon is not present under out/ (missing electron main or contrib JS)"
fi

horizon_info "build complete — Horizon Map contrib is compiled into out/"
echo
echo "Launch with:"
echo "  ./ide/scripts/run.sh [workspace-path]"
echo
echo "Fast iteration after editing ide/contrib/horizon:"
echo "  ./ide/scripts/dev.sh [workspace-path]   # sync + compile-client + run"
echo "  # or: HORIZON_COMPILE_MODE=full ./ide/scripts/build.sh   # full npm run compile"
