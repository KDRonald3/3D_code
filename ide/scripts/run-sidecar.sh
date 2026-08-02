#!/usr/bin/env bash
# Run horizon-server on loopback for IDE / manual testing.
#
# Prefers target/release/horizon-server, then target/debug, then
# `cargo run -p horizon-server`. Always passes --no-open.
#
# Writes the listen URL to ide/.cache/sidecar.url so ./ide/scripts/run.sh
# (and the workbench contrib) can attach via HORIZON_SIDECAR_URL.
#
# Usage:
#   ./ide/scripts/run-sidecar.sh
#   ./ide/scripts/run-sidecar.sh --map /path/to/map.json
#
# Env:
#   HORIZON_SERVER_PATH     Absolute path to a horizon-server binary
#   HORIZON_SIDECAR_URL_FILE  Override URL file path (default ide/.cache/sidecar.url)
#
# Health: GET http://127.0.0.1:PORT/api/health → {"ok":true}
# Analyse: POST /api/analyse  {"path":"/abs/workspace"}
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

REPO_ROOT="${HORIZON_REPO_ROOT}"
EXTRA_ARGS=("$@")

pick_binary() {
  horizon_find_sidecar_bin
}

write_url_from_line() {
  local line="$1"
  if [[ "${line}" =~ ^http://(127\.0\.0\.1|localhost|\[::1\]):[0-9]+/?$ ]]; then
    horizon_write_sidecar_url_file "${line}"
    echo "export HORIZON_SIDECAR_URL=${line%/}" >&2
  fi
}

horizon_info "Horizon sidecar (loopback only, --no-open)"
horizon_info "URL file: ${HORIZON_SIDECAR_URL_FILE}"

BIN=""
if BIN="$(pick_binary)"; then
  horizon_info "using binary: ${BIN}"
  # Stream output; capture first listen URL into the shared URL file.
  mkdir -p "$(dirname "${HORIZON_SIDECAR_URL_FILE}")"
  "${BIN}" --no-open "${EXTRA_ARGS[@]}" 2>&1 | while IFS= read -r line || [[ -n "${line}" ]]; do
    printf '%s\n' "${line}"
    write_url_from_line "${line}" || true
  done
  exit "${PIPESTATUS[0]:-0}"
fi

horizon_info "no built binary found; falling back to: cargo run -p horizon-server -- --no-open"
horizon_info "(tip: cargo build -p horizon-server --release for faster restarts)"
cd "${REPO_ROOT}"
mkdir -p "$(dirname "${HORIZON_SIDECAR_URL_FILE}")"
cargo run -p horizon-server -- --no-open "${EXTRA_ARGS[@]}" 2>&1 | while IFS= read -r line || [[ -n "${line}" ]]; do
  printf '%s\n' "${line}"
  write_url_from_line "${line}" || true
done
exit "${PIPESTATUS[0]:-0}"
