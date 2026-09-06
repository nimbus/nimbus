#!/usr/bin/env bash
# Mutation-tests real per-path conditions in the retention verifier.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="${repo_root}/scripts/verify-storage-metadata-retention.sh"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-retention-verifier.XXXXXX")"
trap 'rm -rf "${temporary}"' EXIT

fixture_root="${temporary}/root"
mkdir -p \
  "${fixture_root}/crates" \
  "${fixture_root}/docs/private/plans/proof"
cp -R "${repo_root}/crates/nimbus-storage" "${fixture_root}/crates/"
cp -R "${repo_root}/crates/nimbus-engine" "${fixture_root}/crates/"
cp -R \
  "${repo_root}/docs/private/plans/proof/storage-metadata-retention" \
  "${fixture_root}/docs/private/plans/proof/"

passed_groups=0
passed_omissions=0
verifier_output=''
verifier_status=0

run_verifier() {
  if verifier_output="$(
    NIMBUS_STORAGE_RETENTION_VERIFY_ROOT="${fixture_root}" \
      bash "${verifier}" 2>&1
  )"; then
    verifier_status=0
  else
    verifier_status=$?
  fi
}

run_verifier
if [[ "${verifier_status}" -ne 0 ]] \
  || [[ "${verifier_output}" != *'Summary: 18 passed, 0 failed'* ]]; then
  printf 'FAIL: copied repository fixture is not green\n%s\n' "${verifier_output}" >&2
  exit 1
fi
printf 'PASS: copied repository fixture satisfies all 18 conditions\n'

require_real_condition_each_path() {
  local label="$1"
  local condition="$2"
  shift 2
  local relative_path path omitted_path index=0

  for relative_path in "$@"; do
    index=$((index + 1))
    path="${fixture_root}/${relative_path}"
    omitted_path="${path}.srr3-omitted"
    mv -- "${path}" "${omitted_path}"
    run_verifier
    mv -- "${omitted_path}" "${path}"

    if [[ "${verifier_status}" -eq 0 ]]; then
      printf 'FAIL: %s omission %d passed the full verifier (%s)\n' \
        "${label}" "${index}" "${relative_path}" >&2
      exit 1
    fi
    if [[ "${verifier_output}" != *"FAIL: ${condition}"* ]]; then
      printf 'FAIL: %s omission %d did not fail its owning condition (%s)\n%s\n' \
        "${label}" "${index}" "${relative_path}" "${verifier_output}" >&2
      exit 1
    fi
    passed_omissions=$((passed_omissions + 1))
  done

  passed_groups=$((passed_groups + 1))
  printf 'PASS: %s real condition rejects all %d path omissions\n' \
    "${label}" "${index}"
}

require_real_condition_each_path \
  sql_compaction \
  'MVCC compaction exists on embedded and SQL storage seams' \
  crates/nimbus-storage/src/sqlite.rs \
  crates/nimbus-storage/src/sql/store_core.rs

require_real_condition_each_path \
  typed_retention_errors \
  'trimmed cursor and PITR errors already have a typed classification' \
  crates/nimbus-storage/src/changefeed.rs \
  crates/nimbus-storage/src/store/journal_snapshot.rs

require_real_condition_each_path \
  optimistic_page_checks \
  'journal pages perform an optimistic retention-floor check' \
  crates/nimbus-storage/src/store/journal_stream.rs \
  crates/nimbus-storage/src/sqlite/read.rs \
  crates/nimbus-storage/src/postgres/backend.rs \
  crates/nimbus-storage/src/mysql/read.rs \
  crates/nimbus-storage/src/libsql

require_real_condition_each_path \
  provider_lease_tests \
  'provider retention finalization is lease-fenced and tested' \
  crates/nimbus-storage/src/tests/postgres_provider/retention.rs \
  crates/nimbus-storage/src/tests/mysql_provider/retention.rs \
  crates/nimbus-storage/src/tests/libsql_provider/retention.rs

require_real_condition_each_path \
  concurrent_prune_tests \
  'paged consumers revalidate after reads and cover concurrent pruning' \
  crates/nimbus-storage/src/store/journal_stream/tests.rs \
  crates/nimbus-storage/src/tests/memory_conformance.rs \
  crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs \
  crates/nimbus-storage/src/tests/postgres_provider/retention.rs \
  crates/nimbus-storage/src/tests/mysql_provider/retention.rs \
  crates/nimbus-storage/src/tests/libsql_provider/retention.rs

if [[ "${passed_groups}" -ne 5 ]] || [[ "${passed_omissions}" -ne 18 ]]; then
  printf 'FAIL: expected 5 groups and 18 omissions, got %d and %d\n' \
    "${passed_groups}" "${passed_omissions}" >&2
    exit 1
fi

printf '\nSummary: %d groups passed; %d real-condition omissions failed closed\n' \
  "${passed_groups}" "${passed_omissions}"
