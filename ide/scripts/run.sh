#!/usr/bin/env bash
# Run: launch the built Horizon IDE (Code-OSS), or fall back to a system /
# prebuilt VS Code/VSCodium CLI with --extensionDevelopmentPath=horizon-map.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

OPEN_PATH="${1:-}"
EXTRA_ARGS=()
if [[ $# -gt 1 ]]; then
  EXTRA_ARGS=("${@:2}")
fi

horizon_launch_built() {
  horizon_code_oss_built || return 1

  # Keep product branding + extension in sync for iterative extension work.
  if [[ -f "${HORIZON_CODE_OSS_DIR}/product.json.upstream" ]]; then
    cp "${HORIZON_CODE_OSS_DIR}/product.json.upstream" "${HORIZON_CODE_OSS_DIR}/product.json"
    horizon_apply_product_overlay
  fi
  horizon_sync_extension "${HORIZON_EXTENSION_SYNC_MODE:-copy}"

  horizon_setup_node
  cd "${HORIZON_CODE_OSS_DIR}"
  horizon_info "launching built Horizon IDE (scripts/code.sh)"
  if [[ -n "${OPEN_PATH}" ]]; then
    OPEN_PATH="$(horizon_abspath "${OPEN_PATH}")"
    exec ./scripts/code.sh "${OPEN_PATH}" "${EXTRA_ARGS[@]}"
  else
    exec ./scripts/code.sh "${EXTRA_ARGS[@]}"
  fi
}

horizon_launch_fallback() {
  local cli ext_path
  ext_path="${HORIZON_EXTENSION_SRC}"
  [[ -d "${ext_path}" ]] || horizon_die "extension missing at ${ext_path}"

  # Compile the extension TypeScript if needed so EDH can load ./out/extension.js.
  if [[ -f "${ext_path}/package.json" ]] && [[ ! -f "${ext_path}/out/extension.js" ]]; then
    horizon_info "compiling horizon-map extension (tsc)…"
    horizon_setup_node
    (
      cd "${ext_path}"
      if [[ ! -d node_modules ]]; then
        npm install --no-fund --no-audit
      fi
      npm run compile 2>/dev/null || npx tsc -p .
    )
  fi

  if ! cli="$(horizon_ensure_prebuilt_editor)"; then
    horizon_die "no built Code-OSS app and no code/codium CLI available.
Run ./ide/scripts/build.sh, install VS Code/VSCodium, or set HORIZON_CODE_CLI."
  fi

  horizon_info "falling back to Extension Development Host via: ${cli}"
  local args=(
    --extensionDevelopmentPath="$(horizon_abspath "${ext_path}")"
    --disable-extensions
  )
  if [[ -n "${OPEN_PATH}" ]]; then
    args+=("$(horizon_abspath "${OPEN_PATH}")")
  fi
  args+=("${EXTRA_ARGS[@]}")
  exec "${cli}" "${args[@]}"
}

if horizon_launch_built; then
  :
else
  horizon_warn "built Horizon IDE not ready (need bootstrap + build); using editor fallback"
  horizon_launch_fallback
fi
