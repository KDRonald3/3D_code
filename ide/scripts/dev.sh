#!/usr/bin/env bash
# Fast iteration: sync contrib → gulp compile-client → run.
# Use after editing ide/contrib/horizon/ when a full build already exists.
#
# Usage:
#   ./ide/scripts/dev.sh [workspace-path] [extra code.sh args...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

if ! horizon_code_oss_built; then
  horizon_die "no prior build.
Run once:
  ./ide/scripts/bootstrap.sh
  ./ide/scripts/build.sh
Then use ./ide/scripts/dev.sh for sync + compile-client + run."
fi

horizon_info "Horizon IDE fast path (sync-contrib + compile-client + run)"
horizon_ensure_product_overlay
horizon_patch_preinstall_vs2026
horizon_patch_workbench_csp
horizon_sync_contrib "${HORIZON_CONTRIB_SYNC_MODE:-copy}"
horizon_compile_client
horizon_ensure_sidecar

# Delegate launch (skip redundant compile — out/ is fresh).
# Temporarily mark out fresh by invoking code.sh path via run internals:
OPEN_PATH="${1:-}"
EXTRA_ARGS=()
if [[ $# -gt 1 ]]; then
  EXTRA_ARGS=("${@:2}")
fi

horizon_setup_node
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
