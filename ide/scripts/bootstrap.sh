#!/usr/bin/env bash
# Bootstrap: shallow-clone microsoft/vscode into ide/code-oss/,
# apply Horizon product.json overlay, sync workbench contrib/horizon.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

horizon_info "Horizon IDE bootstrap"
horizon_detect_platform >/dev/null
horizon_info "platform: ${HORIZON_OS}-${HORIZON_ARCH}"
horizon_info "vscode ref: ${HORIZON_VSCODE_REF}"
horizon_info "code-oss dir: ${HORIZON_CODE_OSS_DIR}"

mkdir -p "$(dirname "${HORIZON_CODE_OSS_DIR}")"

if [[ -d "${HORIZON_CODE_OSS_DIR}/.git" ]]; then
  horizon_info "Code-OSS checkout already present"
  (
    cd "${HORIZON_CODE_OSS_DIR}"
    current="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    describe="$(git describe --tags --exact-match 2>/dev/null || git rev-parse --short HEAD)"
    horizon_info "current checkout: ${describe} (branch=${current})"
    if ! git describe --tags --exact-match 2>/dev/null | grep -qx "${HORIZON_VSCODE_REF}" \
      && ! git rev-parse --verify "${HORIZON_VSCODE_REF}^{commit}" >/dev/null 2>&1; then
      horizon_info "fetching pinned ref ${HORIZON_VSCODE_REF}"
      git fetch --depth 1 origin "refs/tags/${HORIZON_VSCODE_REF}:refs/tags/${HORIZON_VSCODE_REF}" \
        || git fetch --depth 1 origin "${HORIZON_VSCODE_REF}"
    fi
    if git rev-parse --verify "refs/tags/${HORIZON_VSCODE_REF}" >/dev/null 2>&1; then
      git checkout -q "refs/tags/${HORIZON_VSCODE_REF}"
    elif git rev-parse --verify "${HORIZON_VSCODE_REF}" >/dev/null 2>&1; then
      git checkout -q "${HORIZON_VSCODE_REF}"
    else
      horizon_warn "could not checkout ${HORIZON_VSCODE_REF}; leaving existing tree as-is"
    fi
  )
else
  if [[ -e "${HORIZON_CODE_OSS_DIR}" ]]; then
    horizon_die "${HORIZON_CODE_OSS_DIR} exists but is not a git checkout; remove it and re-run"
  fi
  horizon_info "shallow-cloning ${HORIZON_VSCODE_REPO} @ ${HORIZON_VSCODE_REF}"
  if ! git clone --depth 1 --branch "${HORIZON_VSCODE_REF}" \
      "${HORIZON_VSCODE_REPO}" "${HORIZON_CODE_OSS_DIR}"; then
    horizon_info "branch clone failed; trying commit fetch"
    git clone --depth 1 "${HORIZON_VSCODE_REPO}" "${HORIZON_CODE_OSS_DIR}"
    (
      cd "${HORIZON_CODE_OSS_DIR}"
      git fetch --depth 1 origin "${HORIZON_VSCODE_REF}"
      git checkout -q FETCH_HEAD
    )
  fi
fi

[[ -f "${HORIZON_CODE_OSS_DIR}/product.json" ]] || horizon_die "clone succeeded but product.json missing"
[[ -f "${HORIZON_CODE_OSS_DIR}/package.json" ]] || horizon_die "clone succeeded but package.json missing"

# Backup stock product.json once for reference / recovery.
if [[ ! -f "${HORIZON_CODE_OSS_DIR}/product.json.upstream" ]]; then
  cp "${HORIZON_CODE_OSS_DIR}/product.json" "${HORIZON_CODE_OSS_DIR}/product.json.upstream"
fi

# Always re-apply from the upstream backup so overlay is idempotent.
cp "${HORIZON_CODE_OSS_DIR}/product.json.upstream" "${HORIZON_CODE_OSS_DIR}/product.json"
horizon_apply_product_overlay

# Sync first-class Map workbench contrib into the Code-OSS tree.
SYNC_MODE="${HORIZON_CONTRIB_SYNC_MODE:-${HORIZON_EXTENSION_SYNC_MODE:-copy}}"
horizon_sync_contrib "${SYNC_MODE}"

horizon_info "bootstrap complete"
echo
echo "Next:"
echo "  ./ide/scripts/build.sh                 # full Code-OSS compile (required for product)"
echo "  ./ide/scripts/run.sh [workspace]       # launch built Horizon IDE"
echo "  ./ide/scripts/sync-contrib.sh          # re-sync contrib after editing"
echo
echo "Note: ./ide/scripts/dev-extension.sh is legacy (Extension Development Host) — not the product path."
