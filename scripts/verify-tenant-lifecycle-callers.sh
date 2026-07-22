#!/usr/bin/env bash
# Proves that every reachable synchronous tenant-creation caller is classified.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INVENTORY="${REPO_ROOT}/scripts/tenant-lifecycle-callers.tsv"
SOURCE_FILTER="${REPO_ROOT}/scripts/tenant-lifecycle-production-source.py"

fail() {
  printf 'tenant-lifecycle-callers: %s\n' "$1" >&2
  exit 1
}

[[ -f "${INVENTORY}" ]] || fail "missing inventory: ${INVENTORY#${REPO_ROOT}/}"
[[ -f "${SOURCE_FILTER}" ]] || fail "missing source filter: ${SOURCE_FILTER#${REPO_ROOT}/}"
python3 "${SOURCE_FILTER}" --self-test
python3 "${SOURCE_FILTER}" --verify-root "${REPO_ROOT}" --inventory "${INVENTORY}"
