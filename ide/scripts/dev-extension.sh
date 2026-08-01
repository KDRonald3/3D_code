#!/usr/bin/env bash
# LEGACY / NON-PRODUCT — Extension Development Host against ide/extensions/horizon-map.
#
# The shipping surface is the Code-OSS workbench contrib:
#   ./ide/scripts/bootstrap.sh && ./ide/scripts/build.sh && ./ide/scripts/run.sh
#
# This script remains only for temporary media/webview experiments against a
# stock editor CLI. Do not treat EDH as the Horizon IDE product path.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

horizon_warn "dev-extension.sh is LEGACY — product path is build.sh + run.sh (workbench contrib)."
horizon_warn "See ide/extensions/horizon-map/DEPRECATED.md and ide/contrib/horizon/README.md"

OPEN_PATH="${1:-${HORIZON_REPO_ROOT}}"
EXT_PATH="${HORIZON_EXTENSION_SRC}"

[[ -d "${EXT_PATH}" ]] || horizon_die "deprecated extension missing at ${EXT_PATH}"
[[ -f "${EXT_PATH}/package.json" ]] || horizon_die "${EXT_PATH}/package.json missing"

horizon_setup_node

# Ensure TypeScript output exists (main: ./out/extension.js).
if [[ ! -f "${EXT_PATH}/out/extension.js" ]] || [[ "${HORIZON_FORCE_EXT_COMPILE:-}" == "1" ]]; then
  horizon_info "compiling deprecated horizon-map extension…"
  (
    cd "${EXT_PATH}"
    if [[ ! -d node_modules ]]; then
      npm install --no-fund --no-audit
    fi
    if npm run compile 2>/dev/null; then
      :
    else
      npx --yes tsc -p .
    fi
  )
  [[ -f "${EXT_PATH}/out/extension.js" ]] || horizon_die "extension compile did not produce out/extension.js"
else
  horizon_info "extension out/ present (set HORIZON_FORCE_EXT_COMPILE=1 to rebuild)"
fi

CLI=""
if ! CLI="$(horizon_ensure_prebuilt_editor)"; then
  horizon_die "no VS Code / VSCodium / code-oss CLI found.
Install one of: code, codium, code-oss — or set HORIZON_CODE_CLI=/path/to/binary.
Prefer the product path instead: ./ide/scripts/build.sh && ./ide/scripts/run.sh"
fi

EXT_ABS="$(horizon_abspath "${EXT_PATH}")"
OPEN_ABS="$(horizon_abspath "${OPEN_PATH}")"

horizon_info "LEGACY Extension Development Host"
horizon_info "  editor:    ${CLI}"
horizon_info "  extension: ${EXT_ABS}"
horizon_info "  workspace: ${OPEN_ABS}"
echo
echo "This is NOT the Horizon IDE product. Use ./ide/scripts/run.sh after build.sh."
echo

exec "${CLI}" \
  --extensionDevelopmentPath="${EXT_ABS}" \
  --disable-extensions \
  "${OPEN_ABS}" \
  "${@:2}"
