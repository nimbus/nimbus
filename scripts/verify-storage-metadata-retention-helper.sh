#!/usr/bin/env bash
# Mutation-tests the per-path requirement used by the retention verifier.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="${repo_root}/scripts/verify-storage-metadata-retention.sh"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-retention-verifier.XXXXXX")"
trap 'rm -rf "${temporary}"' EXIT

passed=0

require_each_path() {
  local label="$1"
  local count="$2"
  local group="${temporary}/${label}"
  local marker="${label}_evidence"
  local index
  local path
  local paths=()

  mkdir -p "${group}"
  for ((index = 1; index <= count; index++)); do
    path="${group}/backend-${index}.rs"
    paths+=("${path}")
    printf '%s\n' "${marker}" >"${path}"
  done

  if ! bash "${verifier}" --require-each "${marker}" "${paths[@]}"; then
    printf 'FAIL: %s green fixture did not satisfy every path\n' "${label}" >&2
    exit 1
  fi

  for ((index = 0; index < count; index++)); do
    mv "${paths[index]}" "${paths[index]}.missing"
    if bash "${verifier}" --require-each "${marker}" "${paths[@]}"; then
      printf 'FAIL: %s omission %d passed\n' "${label}" "$((index + 1))" >&2
      exit 1
    fi
    mv "${paths[index]}.missing" "${paths[index]}"
  done

  passed=$((passed + 1))
  printf 'PASS: %s requires all %d paths\n' "${label}" "${count}"
}

require_each_path sql_compaction 2
require_each_path typed_retention_errors 2
require_each_path optimistic_page_checks 5
require_each_path provider_lease_tests 3
require_each_path concurrent_prune_tests 6

printf '\nSummary: %d passed, 0 failed\n' "${passed}"
