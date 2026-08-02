#!/usr/bin/env bash
# Sync: copy or symlink ide/contrib/horizon into Code-OSS workbench contrib
# and register the import in workbench.common.main.ts.
#
# Usage:
#   ./ide/scripts/sync-contrib.sh          # copy (default)
#   ./ide/scripts/sync-contrib.sh copy
#   ./ide/scripts/sync-contrib.sh link     # symlink for live contrib edits
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

MODE="${1:-${HORIZON_CONTRIB_SYNC_MODE:-copy}}"
case "${MODE}" in
  copy|link) ;;
  *) horizon_die "usage: $0 [copy|link]" ;;
esac

[[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
horizon_sync_contrib "${MODE}"
horizon_info "done — run ./ide/scripts/build.sh or ./ide/scripts/dev.sh (gulp compile-client) so out/ picks up changes"
