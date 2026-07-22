#!/usr/bin/env bash
# Proves that every reachable synchronous tenant-creation caller is classified.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INVENTORY="${REPO_ROOT}/scripts/tenant-lifecycle-callers.tsv"

fail() {
  printf 'tenant-lifecycle-callers: %s\n' "$1" >&2
  exit 1
}

[[ -f "${INVENTORY}" ]] || fail "missing inventory: ${INVENTORY#${REPO_ROOT}/}"

production_source() {
  local file="$1"
  awk '
    pending_test_cfg {
      if ($0 ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{/) {
        exit
      }
      print pending_line
      pending_test_cfg = 0
    }
    /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ {
      pending_test_cfg = 1
      pending_line = $0
      next
    }
    { print }
    END {
      if (pending_test_cfg) {
        print pending_line
      }
    }
  ' "${file}"
}

inventory_keys=$'\n'
while IFS=$'\t' read -r path classification needle expected_count enforcement representative_test; do
  [[ -z "${path}" || "${path}" == "path" || "${path}" == \#* ]] && continue
  file="${REPO_ROOT}/${path}"
  [[ -f "${file}" ]] || fail "inventory path does not exist: ${path}"
  case "${classification}" in
    provider_async|embedded_sync|provider_internal) ;;
    *) fail "unknown classification '${classification}' for ${path}" ;;
  esac
  [[ "${expected_count}" =~ ^[1-9][0-9]*$ ]] \
    || fail "invalid expected_count '${expected_count}' for ${path}"
  [[ -n "${enforcement}" && -n "${representative_test}" ]] \
    || fail "missing enforcement/test evidence for ${path}"
  actual_count="$(production_source "${file}" | awk -v needle="${needle}" '
    index($0, needle) { count += 1 }
    END { print count + 0 }
  ')"
  [[ "${actual_count}" == "${expected_count}" ]] \
    || fail "${path}: expected ${expected_count} production occurrence(s) of '${needle}', found ${actual_count}"
  inventory_keys+="${path}|${classification}"$'\n'
done < "${INVENTORY}"

found_sync=0
while IFS= read -r file; do
  rel="${file#${REPO_ROOT}/}"
  case "${rel}" in
    */tests/*|*/test/*|*/fixtures/*|*/tests.rs|*_tests.rs|*/test_*.rs) continue ;;
  esac
  while IFS= read -r occurrence; do
    [[ -z "${occurrence}" ]] && continue
    [[ "${occurrence}" == *"tenant-lifecycle: test-only"* ]] && continue
    found_sync=$((found_sync + 1))
    if [[ "${occurrence}" == *"tenant-lifecycle: embedded-only"* ]]; then
      classification="embedded_sync"
    elif [[ "${occurrence}" == *"tenant-lifecycle: provider-adapter-internal"* ]]; then
      classification="provider_internal"
    else
      fail "unclassified production synchronous/internal tenant creation: ${rel}:${occurrence}"
    fi
    key="${rel}|${classification}"
    case "${inventory_keys}" in
      *$'\n'"${key}"$'\n'*) ;;
      *) fail "classified call missing from inventory: ${rel}:${occurrence}" ;;
    esac
  done < <(
    production_source "${file}" \
      | awk '
        pending != "" {
          print pending " " $0
          pending = ""
        }
        /\.create_tenant[[:space:]]*\(/ || /Engine::create_tenant([^_[:alnum:]]|$)/ {
          pending = FNR ":" $0
        }
        END {
          if (pending != "") {
            print pending
          }
        }
      '
  )
done < <(find "${REPO_ROOT}/crates" "${REPO_ROOT}/packages" "${REPO_ROOT}/examples" \
  -type f -name '*.rs' -print | LC_ALL=C sort)

[[ "${found_sync}" -gt 0 ]] \
  || fail "zero production synchronous/internal tenant-creation calls found; scanner/filter is vacuous"

printf 'tenant-lifecycle-callers: pass (%s classified synchronous/internal call sites)\n' "${found_sync}"
