#!/usr/bin/env bash
# Run: launch the built Horizon IDE (forked Code-OSS product).
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

# Keep product branding + workbench contrib in sync.
if [[ -f "${HORIZON_CODE_OSS_DIR}/product.json.upstream" ]]; then
  cp "${HORIZON_CODE_OSS_DIR}/product.json.upstream" "${HORIZON_CODE_OSS_DIR}/product.json"
  horizon_apply_product_overlay
fi
horizon_sync_contrib "${HORIZON_CONTRIB_SYNC_MODE:-copy}"

horizon_setup_node
cd "${HORIZON_CODE_OSS_DIR}"
horizon_info "launching built Horizon IDE (scripts/code.sh)"
if [[ -n "${OPEN_PATH}" ]]; then
  OPEN_PATH="$(horizon_abspath "${OPEN_PATH}")"
  exec ./scripts/code.sh "${OPEN_PATH}" "${EXTRA_ARGS[@]}"
else
  exec ./scripts/code.sh "${EXTRA_ARGS[@]}"
fi
