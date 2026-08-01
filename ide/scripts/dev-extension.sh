#!/usr/bin/env bash
# Dev extension: launch an Extension Development Host against
# ide/extensions/horizon-map for fast UI iteration WITHOUT a full Code-OSS compile.
#
# Prefers a system `code` / `codium` / `code-oss` CLI; otherwise downloads a
# portable VSCodium Linux binary into ide/.cache/prebuilt/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

OPEN_PATH="${1:-${HORIZON_REPO_ROOT}}"
EXT_PATH="${HORIZON_EXTENSION_SRC}"

[[ -d "${EXT_PATH}" ]] || horizon_die "extension missing at ${EXT_PATH}"
[[ -f "${EXT_PATH}/package.json" ]] || horizon_die "${EXT_PATH}/package.json missing"

horizon_setup_node

# Ensure TypeScript output exists (main: ./out/extension.js).
if [[ ! -f "${EXT_PATH}/out/extension.js" ]] || [[ "${HORIZON_FORCE_EXT_COMPILE:-}" == "1" ]]; then
  horizon_info "compiling horizon-map…"
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
(On Linux x64 this script can also download a portable VSCodium into ide/.cache/prebuilt/.)"
fi

EXT_ABS="$(horizon_abspath "${EXT_PATH}")"
OPEN_ABS="$(horizon_abspath "${OPEN_PATH}")"

horizon_info "Extension Development Host"
horizon_info "  editor:    ${CLI}"
horizon_info "  extension: ${EXT_ABS}"
horizon_info "  workspace: ${OPEN_ABS}"
echo
echo "Tip: use Command Palette → \"Developer: Reload Window\" after editing the extension."
echo "     For live source edits without re-copying into Code-OSS, this path is preferred."
echo

# --disable-extensions avoids marketplace noise; re-enable rust-analyzer later via
# recommendations or by dropping that flag when testing inspection (W3).
exec "${CLI}" \
  --extensionDevelopmentPath="${EXT_ABS}" \
  --disable-extensions \
  "${OPEN_ABS}" \
  "${@:2}"
