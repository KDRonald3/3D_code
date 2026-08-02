#!/usr/bin/env bash
# LEGACY — sync deprecated ide/extensions/horizon-map into Code-OSS extensions/.
#
# The product surface is the workbench contrib:
#   ./ide/scripts/sync-contrib.sh
#
# Usage (legacy only):
#   ./ide/scripts/sync-extension.sh [copy|link]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

horizon_warn "sync-extension.sh is legacy. Prefer ./ide/scripts/sync-contrib.sh (workbench contrib)."

MODE="${1:-${HORIZON_EXTENSION_SYNC_MODE:-copy}}"
case "${MODE}" in
  copy|link) ;;
  *) horizon_die "usage: $0 [copy|link]" ;;
esac

[[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
horizon_sync_extension "${MODE}"
