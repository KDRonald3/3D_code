#!/usr/bin/env bash
# Sync: copy or symlink ide/extensions/horizon-map into the Code-OSS extensions tree.
#
# Usage:
#   ./ide/scripts/sync-extension.sh          # copy (default)
#   ./ide/scripts/sync-extension.sh copy
#   ./ide/scripts/sync-extension.sh link     # symlink for live extension edits
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

MODE="${1:-${HORIZON_EXTENSION_SYNC_MODE:-copy}}"
case "${MODE}" in
  copy|link) ;;
  *) horizon_die "usage: $0 [copy|link]" ;;
esac

[[ -d "${HORIZON_CODE_OSS_DIR}" ]] || horizon_die "Code-OSS missing; run ./ide/scripts/bootstrap.sh first"
horizon_sync_extension "${MODE}"
