#!/usr/bin/env bash
# Run horizon-server on loopback for manual IDE / preview testing (W4).
#
# Prefers target/release/horizon-server, then target/debug, then
# `cargo run -p horizon-server`. Always passes --no-open.
#
# Usage:
#   ./ide/scripts/run-sidecar.sh
#   ./ide/scripts/run-sidecar.sh --map /path/to/map.json
#
# Env:
#   HORIZON_SERVER_PATH  Absolute path to a horizon-server binary (overrides search)
#   HORIZON_SIDECAR_URL  Printed after start so you can export it for the IDE:
#                        export HORIZON_SIDECAR_URL=http://127.0.0.1:PORT
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
  if [[ -n "${HORIZON_SERVER_PATH:-}" ]]; then
    if [[ -x "${HORIZON_SERVER_PATH}" ]]; then
      echo "${HORIZON_SERVER_PATH}"
      return 0
    fi
    horizon_die "HORIZON_SERVER_PATH is set but not executable: ${HORIZON_SERVER_PATH}"
  fi

  local release debug
  release="${REPO_ROOT}/target/release/horizon-server"
  debug="${REPO_ROOT}/target/debug/horizon-server"

  if [[ -x "${release}" ]]; then
    echo "${release}"
    return 0
  fi
  if [[ -x "${debug}" ]]; then
    echo "${debug}"
    return 0
  fi

  if command -v horizon-server >/dev/null 2>&1; then
    command -v horizon-server
    return 0
  fi

  return 1
}

horizon_info "Horizon sidecar (loopback only, --no-open)"

BIN=""
if BIN="$(pick_binary)"; then
  horizon_info "using binary: ${BIN}"
  # Print URL on stdout; keep server in foreground for Ctrl-C.
  # Capture the first line (listen URL) while still streaming output.
  exec "${BIN}" --no-open "${EXTRA_ARGS[@]}"
fi

horizon_info "no built binary found; falling back to: cargo run -p horizon-server -- --no-open"
horizon_info "(tip: cargo build -p horizon-server --release for faster restarts)"
cd "${REPO_ROOT}"
exec cargo run -p horizon-server -- --no-open "${EXTRA_ARGS[@]}"
