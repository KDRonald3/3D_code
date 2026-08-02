#!/usr/bin/env bash
# Run: launch the built Horizon IDE (forked Code-OSS product).
#
# Before launch:
#   - Re-apply product overlay (branding)
#   - If contrib sources are newer than out/, auto sync + gulp compile-client
#   - Start / attach horizon-server sidecar (URL → ide/.cache/horizon-sidecar.url)
#
# Does NOT fall back to Extension Development Host — that is not the product path.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

OPEN_PATH="${1:-}"
EXTRA_ARGS=()
if [[ $# -gt 1 ]]; then
  EXTRA_ARGS=("${@:2}")
fi

if ! horizon_code_oss_built; then
  horizon_die "built Horizon IDE not ready.
Run:
  ./ide/scripts/bootstrap.sh
  ./ide/scripts/build.sh
Then re-run ./ide/scripts/run.sh

(Legacy Extension Development Host is ./ide/scripts/dev-extension.sh — not the product.)"
fi

# Keep product branding applied.
horizon_ensure_product_overlay

# If Map sources changed since last compile, refresh out/ automatically.
if horizon_contrib_out_stale; then
  horizon_warn "contrib newer than out/ — syncing and running gulp compile-client"
  horizon_sync_contrib "${HORIZON_CONTRIB_SYNC_MODE:-copy}"
  horizon_compile_client
else
  # Still re-copy sources so link/copy mode stays consistent; skip compile.
  horizon_sync_contrib "${HORIZON_CONTRIB_SYNC_MODE:-copy}"
  if [[ ! -f "$(horizon_contrib_out_js)" ]]; then
    horizon_warn "Horizon contrib JS missing under out/ — compiling client"
    horizon_compile_client
  fi
fi

if ! horizon_horizon_built_in; then
  horizon_die "Horizon contrib is not compiled into out/.
Run ./ide/scripts/build.sh (default: gulp compile-client)."
fi

# Sidecar for analyse — URL file shared with run-sidecar.sh / W4 attach.
horizon_ensure_sidecar

horizon_setup_node

horizon_ensure_rust_analyzer

cd "${HORIZON_CODE_OSS_DIR}"
horizon_info "launching built Horizon IDE (scripts/code.sh)"
if [[ -n "${HORIZON_SIDECAR_URL:-}" ]]; then
  horizon_info "HORIZON_SIDECAR_URL=${HORIZON_SIDECAR_URL}"
fi
if [[ -n "${OPEN_PATH}" ]]; then
  OPEN_PATH="$(horizon_abspath "${OPEN_PATH}")"
  exec ./scripts/code.sh "${OPEN_PATH}" "${EXTRA_ARGS[@]}"
else
  exec ./scripts/code.sh "${EXTRA_ARGS[@]}"
fi
