#!/usr/bin/env bash
# Bootstrap and completion-gate verifier for
# docs/plans/storage-engine-quality-and-mvcc-plan.md.
#
# SEQ0 intentionally starts with control-plane checks. Later SEQ phases must
# extend this script as their proof files, tests, benchmarks, and docs land.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/plans/storage-engine-quality-and-mvcc-plan.md"
PROOF_DIR="docs/plans/proof/storage-engine-quality-and-mvcc"
SEQ0_PROOF="${PROOF_DIR}/seq0-design-refresh.md"
SEQ1_PROOF="${PROOF_DIR}/seq1-mvcc-semantics.md"
SEQ2_PROOF="${PROOF_DIR}/seq2-versioned-registries.md"
SEQ3_PROOF="${PROOF_DIR}/seq3-versioned-documents.md"
SEQ4_PROOF="${PROOF_DIR}/seq4-versioned-indexes.md"
SEQ5_PROOF="${PROOF_DIR}/seq5-serving-snapshot-manager.md"
SEQ6_PROOF="${PROOF_DIR}/seq6-occ-conflict-detection.md"
SEQ7_PROOF="${PROOF_DIR}/seq7-retention-gc.md"
SEQ8_PROOF="${PROOF_DIR}/seq8-pitr-export-import.md"
SEQ9_PROOF="${PROOF_DIR}/seq9-cdc-changefeed.md"
SEQ10_PROOF="${PROOF_DIR}/seq10-metamorphic-mvcc.md"
SEQ11_PROOF="${PROOF_DIR}/seq11-deterministic-parity.md"
SEQ12_PROOF="${PROOF_DIR}/seq12-diagnostics-knobs.md"
SEQ13_PROOF="${PROOF_DIR}/seq13-performance.md"
SEQ14_PROOF="${PROOF_DIR}/seq14-closeout.md"
DEBT_DOC="docs/technical-debt.md"
SATH_VERIFIER="scripts/verify-storage-architecture-trust-hardening.sh"
EMBEDDED_BENCH_REPORT="docs/plans/research/sqlite-storage-benchmark-report.md"
POSTGRES_BENCH_REPORT="docs/plans/research/postgres-provider-benchmark-report.md"
MYSQL_BENCH_REPORT="docs/plans/research/mysql-provider-benchmark-report.md"
LIBSQL_BENCH_REPORT="docs/plans/research/sqlite-replica-provider-benchmark-report.md"
SEQ0_EMBEDDED_POINT_READ_REPORT="${PROOF_DIR}/seq0-embedded-point-read-baseline.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

contains() {
  local file="$1"
  local pattern="$2"
  [ -f "${file}" ] && grep -q "${pattern}" "${file}"
}

line_count_at_most() {
  local file="$1"
  local max_lines="$2"
  [ -f "${file}" ] && [ "$(wc -l <"${file}" | tr -d ' ')" -le "${max_lines}" ]
}

printf '\033[1mSEQ verification gate - storage-engine-quality-and-mvcc\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Plan, routing entry, proof bundle, and verifier exist"
SEQ_DEBT_COUNT=0
if [ -f "${DEBT_DOC}" ]; then
  SEQ_DEBT_COUNT="$(grep -Ec '^\|[[:space:]]*[A-Z]-[0-9]+[[:space:]]*\|.*\|[[:space:]]*SEQ[0-9]+' "${DEBT_DOC}")"
fi
if [ -f "${PLAN}" ] \
   && [ -d "${PROOF_DIR}" ] \
   && [ -f "${SEQ0_PROOF}" ] \
   && [ -f "${SEQ1_PROOF}" ] \
   && [ -f "${SEQ2_PROOF}" ] \
   && [ -f "${SEQ3_PROOF}" ] \
   && [ -f "${SEQ4_PROOF}" ] \
   && [ -f "${SEQ5_PROOF}" ] \
   && [ -f "${SEQ6_PROOF}" ] \
   && [ -f "${SEQ7_PROOF}" ] \
   && [ -f "${SEQ8_PROOF}" ] \
   && [ -f "${SEQ9_PROOF}" ] \
   && [ -f "${SEQ10_PROOF}" ] \
   && [ -f "${SEQ11_PROOF}" ] \
   && [ -f "${SEQ12_PROOF}" ] \
   && [ -f "${SEQ13_PROOF}" ] \
   && [ -f "${SEQ14_PROOF}" ] \
   && [ -x "scripts/verify-storage-engine-quality-and-mvcc.sh" ] \
   && grep -q "${PLAN}" docs/plans/README.md \
   && [ "${SEQ_DEBT_COUNT}" -ge 5 ]; then
  pass "SEQ control-plane artifacts exist"
else
  fail "SEQ control-plane artifacts incomplete" \
    "Expected plan, routing entry, executable verifier, SEQ0/SEQ1/SEQ2/SEQ3/SEQ4/SEQ5/SEQ6/SEQ7/SEQ8/SEQ9/SEQ10/SEQ11/SEQ12/SEQ13/SEQ14 proof files, proof directory, and >= 5 SEQ debt rows"
fi

step 2 "Plan is autonomous from the current bootstrap state"
IN_PROGRESS_PHASE_COUNT="$(grep -Ec '^\|[[:space:]]*SEQ[0-9]+[[:space:]]*\|[[:space:]]*`in_progress`[[:space:]]*\|' "${PLAN}")"
if contains "${PLAN}" 'seq14-done' \
   && contains "${PLAN}" 'Start at SEQ0' \
   && contains "${PLAN}" 'at most one phase in_progress' \
   && contains "${PLAN}" 'external_evidence_pending' \
   && contains "${PLAN}" 'Control Plane Rules' \
   && contains "${PLAN}" 'Verifier Contract' \
   && [ "${IN_PROGRESS_PHASE_COUNT}" -le 1 ]; then
  pass "Plan has final status, control rules, verifier contract, autonomous /goal prompt, and at most one in-progress phase"
else
  fail "Plan autonomy incomplete" \
    "Expected final status, Start-at-SEQ0 goal prompt, at-most-one-in-progress rule, external_evidence_pending semantics, Control Plane Rules, Verifier Contract, and <= 1 in-progress phase"
fi

step 3 "SEQ0 proof records worktree, branch, base, sources, and enterprise charter"
if contains "${SEQ0_PROOF}" '^status: done$' \
   && contains "${SEQ0_PROOF}" 'codex/storage-engine-quality-and-mvcc' \
   && contains "${SEQ0_PROOF}" '4a9e6a77bcd3c51ef14018d1e34c3e2dfd199d38' \
   && contains "${SEQ0_PROOF}" 'Enterprise Guarantee Charter' \
   && contains "${SEQ0_PROOF}" 'Source-Backed Design Decisions' \
   && contains "${SEQ0_PROOF}" 'All-Supported Backend And Adapter Matrix' \
   && contains "${SEQ0_PROOF}" 'Staged Proof Order' \
   && contains "${SEQ0_PROOF}" 'Performance Baseline Inputs' \
   && contains "${SEQ0_PROOF}" 'Performance Budgets' \
   && contains "${SEQ0_PROOF}" 'External Provider Benchmark Gate State' \
   && contains "${SEQ0_PROOF}" 'SEQ1 And SEQ9 Pre-Implementation Decisions' \
   && contains "${SEQ0_PROOF}" 'SEQ0 Closeout' \
   && contains "${SEQ0_PROOF}" '602dc945' \
   && contains "${SEQ0_PROOF}" '5f5932a2bf5' \
   && contains "${SEQ0_PROOF}" '64899c7a4' \
   && contains "${SEQ0_PROOF}" '8bcadb7' \
   && contains "${SEQ0_PROOF}" '93de8e3'; then
  pass "SEQ0 proof records required bootstrap evidence"
else
  fail "SEQ0 proof evidence incomplete" \
    "Expected status, worktree/branch/base, enterprise charter, support matrix, staged proof order, budgets, and refreshed source refs"
fi

step 4 "Supported backends and adapter surfaces are represented"
if contains "${SEQ0_PROOF}" 'SQLite embedded tenant backend' \
   && contains "${SEQ0_PROOF}" 'redb embedded tenant backend' \
   && contains "${SEQ0_PROOF}" 'Postgres tenant backend' \
   && contains "${SEQ0_PROOF}" 'MySQL tenant backend' \
   && contains "${SEQ0_PROOF}" 'libSQL tenant backend' \
   && contains "${SEQ0_PROOF}" 'Convex adapter surface' \
   && contains "${SEQ0_PROOF}" 'Firebase/Firestore adapter surface' \
   && contains "${SEQ0_PROOF}" 'Cloud Functions trigger surface' \
   && contains "${SEQ0_PROOF}" 'DynamoDB adapter surface' \
   && contains "${SEQ0_PROOF}" 'MongoDB adapter surface' \
   && contains "${SEQ0_PROOF}" 'Nimbus-native APIs' \
   && contains "${SEQ0_PROOF}" 'Adapter Exposure Policy' \
   && contains "${SEQ0_PROOF}" 'typed errors' \
   && contains "${SEQ0_PROOF}" 'unsupported backend/provider' \
   && contains "${SEQ0_PROOF}" 'unsupported adapter extensions' \
   && contains "${PLAN}" 'Current adapter inventory for this plan' \
   && contains "${PLAN}" 'Convex adapter surface' \
   && contains "${PLAN}" 'Firebase/Firestore adapter surface' \
   && contains "${PLAN}" 'Cloud Functions trigger surface' \
   && contains "${PLAN}" 'DynamoDB adapter surface' \
   && contains "${PLAN}" 'MongoDB adapter surface' \
   && contains "${PLAN}" 'Native HTTP/WebSocket surface'; then
  pass "SEQ0 support matrix covers current backend and adapter surfaces with fail-closed policy"
else
  fail "SEQ0 support matrix incomplete" \
    "Expected all current surfaces in both the plan inventory and SEQ0 proof plus adapter exposure and typed fail-closed policy"
fi

step 5 "SEQ0 performance baseline inputs and refresh commands are recorded"
if [ -f "${EMBEDDED_BENCH_REPORT}" ] \
   && [ -f "${POSTGRES_BENCH_REPORT}" ] \
   && [ -f "${MYSQL_BENCH_REPORT}" ] \
   && [ -f "${LIBSQL_BENCH_REPORT}" ] \
   && contains "${SEQ0_PROOF}" "${EMBEDDED_BENCH_REPORT}" \
   && contains "${SEQ0_PROOF}" "${POSTGRES_BENCH_REPORT}" \
   && contains "${SEQ0_PROOF}" "${MYSQL_BENCH_REPORT}" \
   && contains "${SEQ0_PROOF}" "${LIBSQL_BENCH_REPORT}" \
   && contains "${SEQ0_PROOF}" 'make bench-embedded-providers' \
   && contains "${SEQ0_PROOF}" 'make bench-postgres-provider' \
   && contains "${SEQ0_PROOF}" 'make bench-mysql-provider' \
   && contains "${SEQ0_PROOF}" 'make bench-libsql-replica-provider' \
   && [ -f "${SEQ0_EMBEDDED_POINT_READ_REPORT}" ] \
   && contains "${SEQ0_PROOF}" "${SEQ0_EMBEDDED_POINT_READ_REPORT}" \
   && contains "${SEQ0_PROOF}" 'Focused Current Embedded Baseline' \
   && contains "${SEQ0_PROOF}" 'SEQ0 latest-path guardrail' \
   && contains "${SEQ0_EMBEDDED_POINT_READ_REPORT}" 'point read latency' \
   && contains "${SEQ0_EMBEDDED_POINT_READ_REPORT}" 'workload filter: `point read latency`' \
   && contains "${SEQ0_PROOF}" 'latest-path regressions are failures' \
   && contains "${SEQ0_PROOF}" 'RTT-sensitive lanes'; then
  pass "SEQ0 records existing benchmark reports, focused current baseline, refresh commands, and budget policy"
else
  fail "SEQ0 performance baseline inputs incomplete" \
    "Expected embedded/Postgres/MySQL/libSQL benchmark reports, focused current point-read report, refresh commands, and latest/RTT budget policy"
fi

step 6 "SEQ1 MVCC semantic contract is implemented and tested"
CORE_MVCC_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-core-mvcc.XXXXXX")"
CORE_HISTORICAL_READ_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-core-historical-read.XXXXXX")"
if contains "${PLAN}" 'SEQ0 | `done`' \
   && contains "${PLAN}" 'SEQ1 | `done`' \
   && contains "${SEQ1_PROOF}" '^status: done$' \
   && contains "${SEQ1_PROOF}" 'SEQ1 MVCC Semantics' \
   && contains "${SEQ1_PROOF}" 'HistoricalReadSnapshot' \
   && contains "${SEQ1_PROOF}" 'HistoricalCursorIdentity' \
   && contains "${SEQ1_PROOF}" 'HistoricalAuthorization' \
   && contains "${SEQ1_PROOF}" 'unsupported-backend' \
   && contains "${SEQ1_PROOF}" '11 passed, 0 failed' \
   && contains "${SEQ1_PROOF}" '2 passed, 0 failed' \
   && contains "crates/nimbus-core/src/mvcc.rs" 'pub struct HistoricalReadSnapshot' \
   && contains "crates/nimbus-core/src/mvcc.rs" 'pub struct HistoricalCursorIdentity' \
   && contains "crates/nimbus-core/src/mvcc.rs" 'pub enum HistoricalVersionVisibility' \
   && contains "crates/nimbus-core/src/error.rs" 'pub enum HistoricalReadErrorKind' \
   && cargo test -p nimbus-core mvcc -- --nocapture >"${CORE_MVCC_OUTPUT}" 2>&1 \
   && grep -q '11 passed; 0 failed' "${CORE_MVCC_OUTPUT}" \
   && cargo test -p nimbus-core historical_read -- --nocapture >"${CORE_HISTORICAL_READ_OUTPUT}" 2>&1 \
   && grep -q '2 passed; 0 failed' "${CORE_HISTORICAL_READ_OUTPUT}"; then
  pass "SEQ1 core MVCC semantics and typed fail-closed errors are implemented and tested"
else
  fail "SEQ1 active proof incomplete" \
    "Expected SEQ1 done, semantic source anchors, proof evidence, and passing nimbus-core mvcc/historical_read tests; captured outputs at ${CORE_MVCC_OUTPUT} and ${CORE_HISTORICAL_READ_OUTPUT}"
fi

step 7 "SEQ2 versioned registry read-shape contract is implemented and tested"
CORE_REGISTRY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-core-versioned-registry.XXXXXX")"
if contains "${PLAN}" 'SEQ2 | `done`' \
   && contains "${SEQ2_PROOF}" '^status: done$' \
   && contains "${SEQ2_PROOF}" 'SEQ2 Versioned Registries' \
   && contains "${SEQ2_PROOF}" 'VersionedRegistry' \
   && contains "${SEQ2_PROOF}" 'HistoricalReadShape' \
   && contains "${SEQ2_PROOF}" '8 passed, 0 failed' \
   && contains "crates/nimbus-core/src/versioned_registry.rs" 'pub struct HistoricalReadShape' \
   && contains "crates/nimbus-core/src/versioned_registry.rs" 'pub struct VersionedRegistry' \
   && cargo test -p nimbus-core versioned_registry -- --nocapture >"${CORE_REGISTRY_OUTPUT}" 2>&1 \
   && grep -q '8 passed; 0 failed' "${CORE_REGISTRY_OUTPUT}"; then
  pass "SEQ2 core registry oracle and read-shape bundle are implemented and tested"
else
  fail "SEQ2 proof or tests incomplete" \
    "Expected SEQ2 done, semantic source anchors, proof evidence, and passing nimbus-core versioned_registry tests; captured output at ${CORE_REGISTRY_OUTPUT}"
fi

step 8 "SEQ3 document-version storage has core and live all-provider evidence"
CORE_DOCUMENT_HISTORY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-core-document-history.XXXXXX")"
STORAGE_REDB_DOCUMENT_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-redb-document-versions.XXXXXX")"
STORAGE_SQLITE_DOCUMENT_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-sqlite-document-versions.XXXXXX")"
STORAGE_ALL_DOCUMENT_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-all-document-versions.XXXXXX")"
STORAGE_TENANT_INIT_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-tenant-init.XXXXXX")"
if contains "${PLAN}" 'SEQ3 | `done`' \
   && contains "${SEQ3_PROOF}" '^status: done$' \
   && contains "${SEQ3_PROOF}" 'SEQ3 Versioned Documents' \
   && contains "${SEQ3_PROOF}" 'Read-Before-Edit Checklist' \
   && contains "${SEQ2_PROOF}" 'HistoricalReadSnapshot' \
   && contains "${SEQ2_PROOF}" 'PolicySnapshotId' \
   && contains "${SEQ3_PROOF}" 'Implementation Evidence' \
   && contains "${SEQ3_PROOF}" 'DocumentVersionHistory' \
   && contains "${SEQ3_PROOF}" '4 passed, 0 failed' \
   && contains "${SEQ3_PROOF}" 'Embedded Physical Storage Evidence' \
   && contains "${SEQ3_PROOF}" 'redb physical document-version rows' \
   && contains "${SEQ3_PROOF}" 'SQLite physical document-version rows' \
   && contains "${SEQ3_PROOF}" 'Postgres physical document-version rows' \
   && contains "${SEQ3_PROOF}" 'MySQL physical document-version rows' \
   && contains "${SEQ3_PROOF}" 'libSQL physical document-version rows' \
   && contains "${SEQ3_PROOF}" 'document-version storage format marker' \
   && contains "${SEQ3_PROOF}" 'StorageHealthDiagnostic' \
   && contains "${SEQ3_PROOF}" 'redb_document_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ3_PROOF}" 'sqlite_document_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ3_PROOF}" 'redb_document_versions_reject_unknown_future_storage_format' \
   && contains "${SEQ3_PROOF}" 'sqlite_document_versions_reject_unknown_future_storage_format' \
   && contains "${SEQ3_PROOF}" 'redb_document_versions_storage_diagnostic_reports_format_and_range' \
   && contains "${SEQ3_PROOF}" 'sqlite_document_versions_storage_diagnostic_reports_format_and_range' \
   && contains "${SEQ3_PROOF}" 'postgres_document_versions_storage_diagnostic_reports_format_and_range' \
   && contains "${SEQ3_PROOF}" 'mysql_document_versions_storage_diagnostic_reports_format_and_range' \
   && contains "${SEQ3_PROOF}" 'libsql_document_versions_storage_diagnostic_reports_format_and_range' \
   && contains "${SEQ3_PROOF}" 'postgres_document_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ3_PROOF}" 'mysql_document_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ3_PROOF}" 'libsql_document_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ3_PROOF}" 'Live external-provider conformance evidence' \
   && contains "${SEQ3_PROOF}" 'Docker-backed live external-provider fixtures' \
   && contains "${SEQ3_PROOF}" 'explicit local Postgres fixture' \
   && contains "${SEQ3_PROOF}" '3 passed, 0 failed' \
   && contains "${SEQ3_PROOF}" 'tenant_init' \
   && contains "${SEQ3_PROOF}" '2 passed, 0 failed' \
   && contains "${SEQ3_PROOF}" '17 passed, 0 failed' \
   && contains "crates/nimbus-core/src/document_history.rs" 'pub struct DocumentVersionHistory' \
   && contains "crates/nimbus-core/src/document_history.rs" 'pub struct DocumentVersion' \
   && contains "crates/nimbus-storage/src/store/document_versions.rs" 'pub fn get_document_version_at' \
   && contains "crates/nimbus-storage/src/store/document_versions.rs" 'record_document_versions_for_writes' \
   && contains "crates/nimbus-storage/src/store/document_versions.rs" 'ensure_document_version_storage_format_in_write_txn' \
   && contains "crates/nimbus-storage/src/sqlite.rs" 'CREATE TABLE IF NOT EXISTS document_versions' \
   && contains "crates/nimbus-storage/src/sqlite/document_versions.rs" 'record_document_versions_for_writes_in_conn' \
   && contains "crates/nimbus-storage/src/sqlite/document_versions.rs" 'get_document_version_at_in_conn' \
   && contains "crates/nimbus-storage/src/sqlite/document_versions.rs" 'ensure_document_version_storage_format_in_conn' \
   && contains "crates/nimbus-storage/src/postgres/document_versions.rs" 'record_document_versions_for_writes_in_session' \
   && contains "crates/nimbus-storage/src/postgres/document_versions.rs" 'ensure_document_version_storage_format_in_session' \
   && contains "crates/nimbus-storage/src/postgres/config.rs" 'document_versions' \
   && contains "crates/nimbus-storage/src/postgres/config.rs" 'tombstone = TRUE ' \
   && contains "crates/nimbus-storage/src/mysql/document_versions.rs" 'record_document_versions_for_writes_in_session' \
   && contains "crates/nimbus-storage/src/mysql/document_versions.rs" 'ensure_document_version_storage_format_in_session' \
   && contains "crates/nimbus-storage/src/mysql/backend.rs" 'document_versions' \
   && contains "crates/nimbus-storage/src/mysql/backend.rs" 'tombstone = TRUE ' \
   && contains "crates/nimbus-storage/src/libsql/document_versions.rs" 'record_document_versions_for_writes_remote' \
   && contains "crates/nimbus-storage/src/libsql/document_versions.rs" 'ensure_document_version_storage_format_remote' \
   && contains "crates/nimbus-storage/src/libsql/remote.rs" 'document_versions' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'DocumentVersionStorageDiagnostic' \
   && contains "${SEQ3_PROOF}" 'version records for document insert' \
   && contains "${SEQ3_PROOF}" 'latest-at-or-before' \
   && cargo test -p nimbus-core document_history -- --nocapture >"${CORE_DOCUMENT_HISTORY_OUTPUT}" 2>&1 \
   && grep -q '4 passed; 0 failed' "${CORE_DOCUMENT_HISTORY_OUTPUT}" \
   && cargo test -p nimbus-storage redb_document_versions -- --nocapture >"${STORAGE_REDB_DOCUMENT_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '4 passed; 0 failed' "${STORAGE_REDB_DOCUMENT_VERSIONS_OUTPUT}" \
   && cargo test -p nimbus-storage sqlite_document_versions -- --nocapture >"${STORAGE_SQLITE_DOCUMENT_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '4 passed; 0 failed' "${STORAGE_SQLITE_DOCUMENT_VERSIONS_OUTPUT}" \
   && cargo test -p nimbus-storage tenant_init -- --nocapture >"${STORAGE_TENANT_INIT_OUTPUT}" 2>&1 \
   && grep -q '2 passed; 0 failed' "${STORAGE_TENANT_INIT_OUTPUT}" \
   && cargo test -p nimbus-storage document_versions -- --nocapture >"${STORAGE_ALL_DOCUMENT_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '17 passed; 0 failed' "${STORAGE_ALL_DOCUMENT_VERSIONS_OUTPUT}"; then
  pass "SEQ3 proof covers the core oracle plus live all-provider physical document-version storage, format gates, and diagnostics"
else
  fail "SEQ3 active proof incomplete" \
    "Expected plan SEQ3 done, seq3 proof with core+live all-provider physical evidence, document-version format gates, diagnostics, generated-DDL regression coverage, source anchors, and passing nimbus-core/nimbus-storage document-version tests; captured outputs at ${CORE_DOCUMENT_HISTORY_OUTPUT}, ${STORAGE_REDB_DOCUMENT_VERSIONS_OUTPUT}, ${STORAGE_SQLITE_DOCUMENT_VERSIONS_OUTPUT}, ${STORAGE_TENANT_INIT_OUTPUT}, and ${STORAGE_ALL_DOCUMENT_VERSIONS_OUTPUT}"
fi

step 9 "SEQ4 core, live all-provider index-version storage, and live all-provider historical routing are implemented and tested"
CORE_INDEX_HISTORY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-core-index-history.XXXXXX")"
STORAGE_INDEX_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-index-versions.XXXXXX")"
STORAGE_HISTORICAL_INDEX_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-storage-historical-index.XXXXXX")"
if contains "${PLAN}" 'SEQ4 | `done`' \
   && contains "${PLAN}" 'HistoricalIndexHistory' \
   && contains "${PLAN}" 'index_versions' \
   && contains "${PLAN}" 'crates/nimbus-storage/src/index/history_scan.rs' \
   && contains "${SEQ4_PROOF}" '^status: done$' \
   && contains "${SEQ4_PROOF}" 'SEQ4 Versioned Indexes' \
   && contains "${SEQ4_PROOF}" 'HistoricalIndexHistory' \
   && contains "${SEQ4_PROOF}" 'HistoricalIndexCursor' \
   && contains "${SEQ4_PROOF}" 'visible_from' \
   && contains "${SEQ4_PROOF}" 'exclusive `visible_until`' \
   && contains "${SEQ4_PROOF}" 'CursorMismatch' \
   && contains "${SEQ4_PROOF}" '6 passed, 0 failed' \
   && contains "${SEQ4_PROOF}" 'Embedded Physical Storage Evidence' \
   && contains "${SEQ4_PROOF}" 'redb physical index-version rows' \
   && contains "${SEQ4_PROOF}" 'SQLite physical index-version rows' \
   && contains "${SEQ4_PROOF}" 'same redb write transaction' \
   && contains "${SEQ4_PROOF}" 'same SQLite transaction' \
   && contains "${SEQ4_PROOF}" 'CURRENT_INDEX_VERSION_STORAGE_FORMAT' \
   && contains "${SEQ4_PROOF}" 'Embedded Historical Routing Evidence' \
   && contains "${SEQ4_PROOF}" 'Shared SQL-family scan planner' \
   && contains "${SEQ4_PROOF}" 'redb historical index scans' \
   && contains "${SEQ4_PROOF}" 'SQLite historical index scans' \
   && contains "${SEQ4_PROOF}" 'cursor mismatch fail-closed' \
   && contains "${SEQ4_PROOF}" 'Provider Physical Storage Evidence' \
   && contains "${SEQ4_PROOF}" 'Postgres physical index-version rows' \
   && contains "${SEQ4_PROOF}" 'MySQL physical index-version rows' \
   && contains "${SEQ4_PROOF}" 'libSQL physical index-version rows' \
   && contains "${SEQ4_PROOF}" 'libSQL replica cache' \
   && contains "${SEQ4_PROOF}" 'Provider Historical Routing Evidence' \
   && contains "${SEQ4_PROOF}" 'Postgres historical index scans' \
   && contains "${SEQ4_PROOF}" 'MySQL historical index scans' \
   && contains "${SEQ4_PROOF}" 'libSQL historical index scans' \
   && contains "${SEQ4_PROOF}" 'Docker-backed live fixture runs' \
   && contains "${SEQ4_PROOF}" 'Confirmed Bug Fixed' \
   && contains "${SEQ4_PROOF}" 'current_query_cache_store()' \
   && contains "${SEQ4_PROOF}" 'libsql_index_versions_are_materialized_during_durable_recovery' \
   && contains "${SEQ4_PROOF}" '1 passed, 0 failed' \
   && contains "${SEQ4_PROOF}" 'Full-Scan Oracle Conformance Evidence' \
   && contains "${SEQ4_PROOF}" 'document-version oracle conformance' \
   && contains "${SEQ4_PROOF}" '12 passed, 0 failed' \
   && contains "${SEQ4_PROOF}" '10 passed, 0 failed' \
   && contains "${SEQ4_PROOF}" 'postgres_index_versions' \
   && contains "${SEQ4_PROOF}" 'postgres_historical_index' \
   && contains "${SEQ4_PROOF}" '2 passed, 0 failed' \
   && contains "crates/nimbus-core/src/index_history.rs" 'pub struct HistoricalIndexHistory' \
   && contains "crates/nimbus-core/src/index_history.rs" 'pub enum HistoricalIndexQuery' \
   && contains "crates/nimbus-core/src/index_history.rs" 'pub struct HistoricalIndexCursor' \
   && contains "crates/nimbus-core/src/index_history.rs" 'policy_snapshot: PolicySnapshotId' \
   && contains "crates/nimbus-core/src/index_history.rs" 'storage_format_generation: u16' \
   && contains "crates/nimbus-core/src/index_history.rs" 'historical_index_cursor_rejects_policy_snapshot_drift' \
   && contains "crates/nimbus-core/src/index_history.rs" 'historical_index_cursor_rejects_storage_format_drift' \
   && contains "crates/nimbus-core/src/index_history.rs" 'pub fn from_document' \
   && contains "crates/nimbus-core/src/index_history.rs" 'pub fn validate_context' \
   && contains "crates/nimbus-core/src/index_history.rs" 'fn visible_at' \
   && contains "crates/nimbus-core/src/lib.rs" 'HistoricalIndexHistory' \
   && contains "crates/nimbus-storage/src/index/mod.rs" 'pub(crate) mod history_scan' \
   && contains "crates/nimbus-storage/src/index/history_scan.rs" 'pub(crate) struct HistoricalIndexScanPlan' \
   && contains "crates/nimbus-storage/src/index/history_scan.rs" 'pub(crate) fn finish_historical_index_page' \
   && contains "crates/nimbus-storage/src/index/history_scan.rs" 'pub fn composite_range' \
   && contains "crates/nimbus-storage/src/index/history_scan.rs" 'cursor.validate_context' \
   && contains "crates/nimbus-storage/src/format.rs" 'CURRENT_INDEX_VERSION_STORAGE_FORMAT' \
   && contains "crates/nimbus-storage/src/store.rs" 'INDEX_VERSIONS' \
   && contains "crates/nimbus-storage/src/store/index_versions.rs" 'record_index_versions_for_writes' \
   && contains "crates/nimbus-storage/src/store/index_versions.rs" 'historical_index_scan_eq_cancellable' \
   && contains "crates/nimbus-storage/src/store/index_versions.rs" 'historical_index_scan_composite_range_cancellable' \
   && contains "crates/nimbus-storage/src/store/index_versions.rs" 'visible_until' \
   && contains "crates/nimbus-storage/src/store/journal.rs" 'record_index_versions_for_events' \
   && contains "crates/nimbus-storage/src/sqlite.rs" 'CREATE TABLE IF NOT EXISTS index_versions' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'record_index_versions_for_writes_in_conn' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'historical_index_scan_eq_cancellable' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'HistoricalIndexScanPlan::equal' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'historical_index_scan_composite_range_cancellable' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'visible_until' \
   && contains "crates/nimbus-storage/src/sqlite/journal.rs" 'record_index_versions_for_events_in_conn' \
   && contains "crates/nimbus-storage/src/postgres/config.rs" 'index_versions' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'record_index_versions_for_writes_in_session' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'historical_index_scan_eq_cancellable' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'HistoricalIndexScanPlan::equal' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'visible_historical_index_entries_for_tuple_bounds' \
   && contains "crates/nimbus-storage/src/postgres/write.rs" 'record_index_versions_for_events_in_session' \
   && contains "crates/nimbus-storage/src/postgres/backend.rs" 'record_index_versions_for_writes_in_session' \
   && contains "crates/nimbus-storage/src/mysql/backend.rs" 'encoded_tuple_hash' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'record_index_versions_for_writes_in_session' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'historical_index_scan_eq_cancellable' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'HistoricalIndexScanPlan::equal' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'visible_historical_index_entries_for_tuple_bounds' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'encoded_tuple_hash' \
   && contains "crates/nimbus-storage/src/mysql/write.rs" 'record_index_versions_for_events_in_session' \
   && contains "crates/nimbus-storage/src/libsql/index_versions.rs" 'record_index_versions_for_writes_remote' \
   && contains "crates/nimbus-storage/src/libsql/read.rs" 'historical_index_scan_eq_cancellable' \
   && contains "crates/nimbus-storage/src/libsql/read.rs" 'current_query_cache_store' \
   && contains "crates/nimbus-storage/src/libsql/read.rs" 'let snapshot = self.current_query_cache_store()?.read_snapshot()?' \
   && contains "crates/nimbus-storage/src/libsql/remote.rs" 'RemoteIndexVersionRow' \
   && contains "crates/nimbus-storage/src/libsql/write.rs" 'record_index_versions_for_events_remote' \
   && contains "crates/nimbus-storage/src/tests/postgres_provider.rs" 'postgres_index_versions_track_direct_write_history' \
   && contains "crates/nimbus-storage/src/tests/postgres_provider.rs" 'postgres_historical_index_scan_eq_and_range_use_versioned_visibility' \
   && contains "crates/nimbus-storage/src/tests/mysql_provider.rs" 'mysql_index_versions_track_direct_write_history' \
   && contains "crates/nimbus-storage/src/tests/mysql_provider.rs" 'mysql_historical_index_scan_eq_and_range_use_versioned_visibility' \
   && contains "crates/nimbus-storage/src/tests/libsql_provider.rs" 'libsql_index_versions_track_direct_write_history_and_snapshot_cache' \
   && contains "crates/nimbus-storage/src/tests/libsql_provider.rs" 'libsql_historical_index_scan_eq_and_range_use_versioned_visibility' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'redb_historical_index_scan_eq_and_range_use_versioned_visibility' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'redb_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'redb_status_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs" 'sqlite_historical_index_scan_eq_and_range_use_versioned_visibility' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs" 'sqlite_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs" 'sqlite_status_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/postgres_provider.rs" 'postgres_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/mysql_provider.rs" 'mysql_rank_full_scan_oracle_titles' \
   && contains "crates/nimbus-storage/src/tests/libsql_provider.rs" 'libsql_rank_full_scan_oracle_titles' \
   && cargo test -p nimbus-core index_history -- --nocapture >"${CORE_INDEX_HISTORY_OUTPUT}" 2>&1 \
   && grep -q '6 passed; 0 failed' "${CORE_INDEX_HISTORY_OUTPUT}" \
   && cargo test -p nimbus-storage index_versions -- --nocapture >"${STORAGE_INDEX_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '12 passed; 0 failed' "${STORAGE_INDEX_VERSIONS_OUTPUT}" \
   && cargo test -p nimbus-storage historical_index -- --nocapture >"${STORAGE_HISTORICAL_INDEX_OUTPUT}" 2>&1 \
   && grep -q '10 passed; 0 failed' "${STORAGE_HISTORICAL_INDEX_OUTPUT}"; then
  pass "SEQ4 proof covers the core oracle, live all-provider physical index-version storage, live all-provider historical index routing, and the libSQL diagnostics freshness fix"
else
  fail "SEQ4 preflight proof incomplete" \
    "Expected seq4 proof, nimbus-core index_history source anchors, live all-provider index-version source anchors/tests, live all-provider historical routing anchors/tests, the libSQL diagnostics freshness fix, and passing cargo test -p nimbus-core index_history plus cargo test -p nimbus-storage index_versions and historical_index; captured outputs at ${CORE_INDEX_HISTORY_OUTPUT}, ${STORAGE_INDEX_VERSIONS_OUTPUT}, and ${STORAGE_HISTORICAL_INDEX_OUTPUT}"
fi

step 10 "SEQ5 serving snapshot read-shape boundary is implemented and tested"
ENGINE_SERVING_SNAPSHOT_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-engine-serving-snapshot.XXXXXX")"
if contains "${PLAN}" 'SEQ5 | `done`' \
   && contains "${SEQ5_PROOF}" '^status: done$' \
   && contains "${SEQ5_PROOF}" 'SEQ5 Serving Snapshot Manager' \
   && contains "${SEQ5_PROOF}" 'PinnedServingReadSnapshot' \
   && contains "${SEQ5_PROOF}" 'HistoricalReadShape' \
   && contains "${SEQ5_PROOF}" 'SnapshotUnavailable' \
   && contains "${SEQ5_PROOF}" '2 passed, 0 failed' \
   && contains "crates/nimbus-core/src/error.rs" 'SnapshotUnavailable' \
   && contains "crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs" 'pub struct PinnedServingReadSnapshot' \
   && contains "crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs" 'pub(crate) fn pin_read_shape' \
   && contains "crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs" 'read_shape.read_snapshot().sequence().sequence()' \
   && contains "crates/nimbus-engine/src/tenant/materialized_reads/snapshot.rs" 'HistoricalReadErrorKind::SnapshotUnavailable' \
   && contains "crates/nimbus-engine/src/service/queries/snapshot.rs" 'pub fn pin_serving_read_shape' \
   && contains "crates/nimbus-engine/src/lib.rs" 'PinnedServingReadSnapshot' \
   && contains "crates/nimbus-engine/src/tests/materialized_serving/retention.rs" 'pinned_serving_read_shape_handle_preserves_identity_and_documents_after_later_applies' \
   && contains "crates/nimbus-engine/src/tests/materialized_serving/retention.rs" 'pinned_serving_read_shape_handle_fails_closed_when_snapshot_does_not_cover_shape' \
   && cargo test -p nimbus-engine pinned_serving_read_shape -- --nocapture >"${ENGINE_SERVING_SNAPSHOT_OUTPUT}" 2>&1 \
   && grep -q '2 passed; 0 failed' "${ENGINE_SERVING_SNAPSHOT_OUTPUT}"; then
  pass "SEQ5 proof covers the existing serving snapshot manager read-shape pin and fail-closed coverage checks"
else
  fail "SEQ5 proof or tests incomplete" \
    "Expected SEQ5 done, seq5 proof, serving snapshot/read-shape source anchors, SnapshotUnavailable error anchor, and passing cargo test -p nimbus-engine pinned_serving_read_shape; captured output at ${ENGINE_SERVING_SNAPSHOT_OUTPUT}"
fi

step 11 "SEQ6 transaction sessions stage pending writes on the existing execution-unit path"
ENGINE_TRANSACTION_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-engine-transaction-session.XXXXXX")"
MONGODB_TRANSACTION_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-mongodb-transaction.XXXXXX")"
DYNAMODB_TRANSACTION_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-dynamodb-transaction.XXXXXX")"
DYNAMODB_ERROR_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-dynamodb-error.XXXXXX")"
FIREBASE_TRANSACTION_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-firebase-transaction.XXXXXX")"
FIREBASE_ERROR_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-firebase-error.XXXXXX")"
if contains "${PLAN}" 'SEQ6 | `done`' \
   && contains "${SEQ6_PROOF}" '^status: done$' \
   && contains "${SEQ6_PROOF}" 'SEQ6 Transaction Sessions And Pending Writes' \
   && contains "${SEQ6_PROOF}" 'Service::stage_atomic_write_batch_in_transaction' \
   && contains "${SEQ6_PROOF}" 'Service::query_documents_in_transaction' \
   && contains "${SEQ6_PROOF}" 'MongoDB no longer buffers outside the engine' \
   && contains "${SEQ6_PROOF}" 'DynamoDB and Firebase error surfaces cover SEQ errors' \
   && contains "${SEQ6_PROOF}" '11 passed, 0 failed' \
   && contains "${SEQ6_PROOF}" '10 passed, 0 failed' \
   && contains "${SEQ6_PROOF}" '7 passed, 0 failed' \
   && contains "crates/nimbus-engine/src/service/transactions.rs" 'pub fn stage_atomic_write_batch_in_transaction' \
   && contains "crates/nimbus-engine/src/service/transactions.rs" 'pub fn query_documents_in_transaction' \
   && contains "crates/nimbus-engine/src/service/transactions.rs" 'read-only transaction session cannot stage writes' \
   && contains "crates/nimbus-engine/src/service/transactions.rs" 'transaction_session_staged_writes_are_visible_inside_session_only_until_commit' \
   && contains "crates/nimbus-engine/src/service/transactions.rs" 'transaction_session_staged_writes_conflict_with_concurrent_document_change' \
   && contains "crates/nimbus-mongodb/src/commands/session.rs" 'pub fn active_transaction_token' \
   && ! contains "crates/nimbus-mongodb/src/commands/session.rs" 'buffered_writes' \
   && contains "crates/nimbus-mongodb/src/commands/crud/mod.rs" 'stage_atomic_write_batch_in_transaction' \
   && contains "crates/nimbus-mongodb/src/commands/crud/filter.rs" 'query_documents_in_transaction' \
   && contains "crates/nimbus-mongodb/src/commands/crud/filter.rs" 'get_document_in_transaction' \
   && contains "crates/nimbus-dynamodb/src/error.rs" 'CoreError::HistoricalRead' \
   && contains "crates/nimbus-firebase/src/errors.rs" 'Error::HistoricalRead' \
   && cargo test -p nimbus-engine transaction_session -- --nocapture >"${ENGINE_TRANSACTION_OUTPUT}" 2>&1 \
   && grep -q '9 passed; 0 failed' "${ENGINE_TRANSACTION_OUTPUT}" \
   && cargo test -p nimbus-mongodb transaction_ -- --nocapture >"${MONGODB_TRANSACTION_OUTPUT}" 2>&1 \
   && grep -q '11 passed; 0 failed' "${MONGODB_TRANSACTION_OUTPUT}" \
   && cargo test -p nimbus-dynamodb transact -- --nocapture >"${DYNAMODB_TRANSACTION_OUTPUT}" 2>&1 \
   && grep -q '10 passed; 0 failed' "${DYNAMODB_TRANSACTION_OUTPUT}" \
   && grep -q '1 passed; 0 failed' "${DYNAMODB_TRANSACTION_OUTPUT}" \
   && cargo test -p nimbus-dynamodb maps_each_core_error_class_to_the_expected_dynamodb_code -- --nocapture >"${DYNAMODB_ERROR_OUTPUT}" 2>&1 \
   && grep -q '1 passed; 0 failed' "${DYNAMODB_ERROR_OUTPUT}" \
   && cargo test -p nimbus-firebase transaction -- --nocapture >"${FIREBASE_TRANSACTION_OUTPUT}" 2>&1 \
   && grep -q '7 passed; 0 failed' "${FIREBASE_TRANSACTION_OUTPUT}" \
   && cargo test -p nimbus-firebase firebase_rest_error_maps_full_core_error_surface -- --nocapture >"${FIREBASE_ERROR_OUTPUT}" 2>&1 \
   && grep -q '1 passed; 0 failed' "${FIREBASE_ERROR_OUTPUT}"; then
  pass "SEQ6 proof covers engine-owned pending-write overlays, OCC conflict checks, MongoDB staging, and adapter error mappings"
else
  fail "SEQ6 proof or tests incomplete" \
    "Expected SEQ6 done, engine/Mongo/DynamoDB/Firebase source anchors, and passing focused transaction/error tests; captured outputs at ${ENGINE_TRANSACTION_OUTPUT}, ${MONGODB_TRANSACTION_OUTPUT}, ${DYNAMODB_TRANSACTION_OUTPUT}, ${DYNAMODB_ERROR_OUTPUT}, ${FIREBASE_TRANSACTION_OUTPUT}, and ${FIREBASE_ERROR_OUTPUT}"
fi

step 12 "SEQ7 retention GC computes safe watermarks and prunes version history"
RETENTION_GC_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-retention-gc.XXXXXX")"
DOCUMENT_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-document-versions.XXXXXX")"
INDEX_VERSIONS_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-index-versions.XXXXXX")"
if contains "${PLAN}" 'SEQ7 | `done`' \
   && contains "${SEQ7_PROOF}" '^status: done$' \
   && contains "${SEQ7_PROOF}" 'RetentionGcConfig' \
   && contains "${SEQ7_PROOF}" 'RetentionGcWatermarks' \
   && contains "${SEQ7_PROOF}" 'Document anchor preservation' \
   && contains "${SEQ7_PROOF}" 'Index interval pruning' \
   && contains "${SEQ7_PROOF}" 'StorageHealthDiagnostic' \
   && contains "${SEQ7_PROOF}" '3 passed, 0 failed' \
   && contains "${SEQ7_PROOF}" '17 passed, 0 failed' \
   && contains "${SEQ7_PROOF}" '12 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/retention.rs" 'pub struct RetentionGcConfig' \
   && contains "crates/nimbus-storage/src/retention.rs" 'pub struct RetentionGcWatermarks' \
   && contains "crates/nimbus-storage/src/retention.rs" 'fn pin_protects_resource' \
   && contains "crates/nimbus-storage/src/retention.rs" 'retention_gc_watermarks_are_resource_specific' \
   && contains "crates/nimbus-storage/src/retention.rs" 'pub struct RetentionGcSummary' \
   && contains "crates/nimbus-storage/src/retention.rs" 'pub fn compact_retained_versions' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub retention_pins: Vec<RetentionPin>' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub retention_gc: RetentionGcWatermarks' \
   && contains "crates/nimbus-storage/src/sqlite.rs" 'pub fn compact_retained_versions' \
   && contains "crates/nimbus-storage/src/postgres/write.rs" 'pub fn compact_retained_versions' \
   && contains "crates/nimbus-storage/src/mysql/write.rs" 'pub fn compact_retained_versions' \
   && contains "crates/nimbus-storage/src/libsql/write.rs" 'pub fn compact_retained_versions' \
   && contains "crates/nimbus-storage/src/store/document_versions.rs" 'document_version_key' \
   && contains "crates/nimbus-storage/src/sqlite/document_versions.rs" 'prune_document_versions_before_in_conn' \
   && contains "crates/nimbus-storage/src/postgres/document_versions.rs" 'prune_document_versions_before_in_session' \
   && contains "crates/nimbus-storage/src/mysql/document_versions.rs" 'prune_document_versions_before_in_session' \
   && contains "crates/nimbus-storage/src/libsql/document_versions.rs" 'prune_document_versions_before_remote' \
   && contains "crates/nimbus-storage/src/retention.rs" 'prune_redb_index_versions_before' \
   && contains "crates/nimbus-storage/src/retention.rs" 'visible_until' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'prune_index_versions_before_in_conn' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'prune_index_versions_before_in_session' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'prune_index_versions_before_in_session' \
   && contains "crates/nimbus-storage/src/libsql/index_versions.rs" 'prune_index_versions_before_remote' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'redb_retention_gc_preserves_document_anchor_and_respects_pins' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs" 'sqlite_retention_gc_preserves_document_anchor_and_respects_pins' \
   && cargo test -p nimbus-storage retention_gc -- --nocapture >"${RETENTION_GC_OUTPUT}" 2>&1 \
   && grep -q '3 passed; 0 failed' "${RETENTION_GC_OUTPUT}" \
   && NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage document_versions -- --nocapture >"${DOCUMENT_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '17 passed; 0 failed' "${DOCUMENT_VERSIONS_OUTPUT}" \
   && NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage index_versions -- --nocapture >"${INDEX_VERSIONS_OUTPUT}" 2>&1 \
   && grep -q '12 passed; 0 failed' "${INDEX_VERSIONS_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ7 proof covers retention GC watermarks, pin safety, document anchors, index interval pruning, diagnostics, and all-provider prune surfaces"
else
  fail "SEQ7 proof or tests incomplete" \
    "Expected SEQ7 done, retention proof/source anchors, passing retention_gc/document_versions/index_versions tests, and cargo check -p nimbus-storage; captured outputs at ${RETENTION_GC_OUTPUT}, ${DOCUMENT_VERSIONS_OUTPUT}, and ${INDEX_VERSIONS_OUTPUT}"
fi

step 13 "SEQ8 PITR export/import restores canonical historical snapshots"
PITR_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-pitr.XXXXXX")"
JOURNAL_SNAPSHOT_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-journal-snapshot.XXXXXX")"
if contains "${PLAN}" 'SEQ8 | `done`' \
   && contains "${SEQ8_PROOF}" '^status: done$' \
   && contains "${SEQ8_PROOF}" 'PointInTimeRestoreArchive' \
   && contains "${SEQ8_PROOF}" 'PointInTimeRestoreTarget' \
   && contains "${SEQ8_PROOF}" 'Canonical fingerprint' \
   && contains "${SEQ8_PROOF}" 'Shared export semantics' \
   && contains "${SEQ8_PROOF}" 'Fail-closed import validation' \
   && contains "${SEQ8_PROOF}" 'Provider restore path' \
   && contains "${SEQ8_PROOF}" '4 passed, 0 failed' \
   && contains "${SEQ8_PROOF}" '6 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'pub enum PointInTimeRestoreTarget' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'pub struct PointInTimeRestoreArchive' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'pub fn canonical_fingerprint' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'build_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'validate_point_in_time_archive_for_journal_replay_import' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'validate_materialized_journal_replay_base_is_empty' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'HistoricalReadErrorKind::RetentionExpired' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'HistoricalReadErrorKind::FormatMismatch' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot/tests.rs" 'point_in_time_archive_restores_sequence_and_timestamp_to_matching_fingerprints' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot/tests.rs" 'point_in_time_archive_rejects_expired_retention_target' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/snapshot.rs" 'sqlite_point_in_time_archive_restores_sequence_and_timestamp_targets' \
   && contains "crates/nimbus-storage/src/sqlite/journal.rs" 'pub fn export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/sqlite/journal.rs" 'pub fn import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/postgres/write.rs" 'pub fn export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/postgres/write.rs" 'pub fn import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/postgres/write.rs" 'recover_durable_journal' \
   && contains "crates/nimbus-storage/src/mysql/write.rs" 'pub fn export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/mysql/write.rs" 'pub fn import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/mysql/write.rs" 'recover_durable_journal' \
   && contains "crates/nimbus-storage/src/libsql/write.rs" 'pub fn export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/libsql/write.rs" 'pub fn import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/libsql/write.rs" 'recover_durable_journal' \
   && contains "crates/nimbus-storage/src/lib.rs" 'PointInTimeRestoreArchive' \
   && cargo test -p nimbus-storage point_in_time -- --nocapture >"${PITR_OUTPUT}" 2>&1 \
   && grep -q '4 passed; 0 failed' "${PITR_OUTPUT}" \
   && cargo test -p nimbus-storage journal_snapshot -- --nocapture >"${JOURNAL_SNAPSHOT_OUTPUT}" 2>&1 \
   && grep -q '6 passed; 0 failed' "${JOURNAL_SNAPSHOT_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ8 proof covers typed PITR archives, retention/format validation, canonical fingerprints, embedded tests, and all-provider production APIs"
else
  fail "SEQ8 proof or tests incomplete" \
    "Expected SEQ8 done, PITR proof/source anchors across redb/SQLite/Postgres/MySQL/libSQL, passing point_in_time and journal_snapshot tests, and cargo check -p nimbus-storage; captured outputs at ${PITR_OUTPUT} and ${JOURNAL_SNAPSHOT_OUTPUT}"
fi

step 14 "SEQ9 CDC/changefeed uses typed cursors and journal handoff"
CHANGEFEED_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-changefeed.XXXXXX")"
DURABLE_JOURNAL_STREAM_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-durable-journal-stream.XXXXXX")"
if contains "${PLAN}" 'SEQ9 | `done`' \
   && contains "${SEQ9_PROOF}" '^status: done$' \
   && contains "${SEQ9_PROOF}" 'ChangefeedHandle' \
   && contains "${SEQ9_PROOF}" 'ChangefeedCursor' \
   && contains "${SEQ9_PROOF}" 'ChangefeedBootstrap' \
   && contains "${SEQ9_PROOF}" 'ChangefeedPage' \
   && contains "${SEQ9_PROOF}" 'ChangefeedEvent' \
   && contains "${SEQ9_PROOF}" 'Snapshot-to-log handoff' \
   && contains "${SEQ9_PROOF}" 'Retention-expired errors' \
   && contains "${SEQ9_PROOF}" 'All-provider storage surface' \
   && contains "${SEQ9_PROOF}" 'MySQL durable-journal streams' \
   && contains "${SEQ9_PROOF}" 'durable_journal_stream' \
   && contains "${SEQ9_PROOF}" 'Engine service surface' \
   && contains "${SEQ9_PROOF}" '2 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'pub struct ChangefeedHandle' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'pub struct ChangefeedCursor' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'pub struct ChangefeedBootstrap' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'pub struct ChangefeedPage' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'pub struct ChangefeedEvent' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'HistoricalReadErrorKind::RetentionExpired' \
   && contains "crates/nimbus-storage/src/changefeed.rs" 'impl_changefeed_journal' \
   && contains "crates/nimbus-storage/src/mysql.rs" 'journal_cursor_floor: SequenceNumber' \
   && contains "crates/nimbus-storage/src/mysql/backend.rs" 'load_durable_journal_cursor_floor_from_session' \
   && contains "crates/nimbus-storage/src/mysql/read.rs" 'cursor_floor: self.journal_cursor_floor' \
   && contains "crates/nimbus-storage/src/mysql/read.rs" 'journal cursor {} is behind the retention floor {}' \
   && contains "crates/nimbus-storage/src/libsql.rs" 'load_remote_durable_journal_cursor_floor' \
   && contains "crates/nimbus-storage/src/traits/mod.rs" 'fn export_changefeed_bootstrap' \
   && contains "crates/nimbus-storage/src/traits/mod.rs" 'fn stream_changefeed' \
   && contains "crates/nimbus-storage/src/lib.rs" 'ChangefeedBootstrap' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'changefeed_bootstrap_pages_events_without_missing_or_duplicating_handoff_records' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'TenantEventKind::TableLifecycle' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'TenantEventKind::SchemaChange' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'TenantEventKind::IndexLifecycle' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'TenantEventKind::DocumentWrite' \
   && contains "crates/nimbus-storage/src/changefeed/tests.rs" 'TenantEventKind::TriggerDelivery' \
   && contains "crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs" 'sqlite_changefeed_stream_reports_retention_expired_after_journal_floor_cut' \
   && contains "crates/nimbus-engine/src/persistence/tenant/journal.rs" 'export_changefeed_bootstrap' \
   && contains "crates/nimbus-engine/src/persistence/tenant/journal.rs" 'stream_changefeed' \
   && contains "crates/nimbus-engine/src/service/queries/journal.rs" 'pub fn export_changefeed_bootstrap' \
   && contains "crates/nimbus-engine/src/service/queries/journal.rs" 'pub async fn export_changefeed_bootstrap_async' \
   && contains "crates/nimbus-engine/src/service/queries/journal.rs" 'pub fn stream_changefeed' \
   && contains "crates/nimbus-engine/src/service/queries/journal.rs" 'pub async fn stream_changefeed_async' \
   && cargo test -p nimbus-storage changefeed -- --nocapture >"${CHANGEFEED_OUTPUT}" 2>&1 \
   && grep -q '2 passed; 0 failed' "${CHANGEFEED_OUTPUT}" \
   && cargo test -p nimbus-storage durable_journal_stream -- --nocapture >"${DURABLE_JOURNAL_STREAM_OUTPUT}" 2>&1 \
   && grep -q '2 passed; 0 failed' "${DURABLE_JOURNAL_STREAM_OUTPUT}" \
   && cargo check -p nimbus-engine >/dev/null 2>&1; then
  pass "SEQ9 proof covers typed CDC handles/cursors, snapshot-to-log handoff, event payloads, retained cursor floors, all-provider storage APIs, and engine service APIs"
else
  fail "SEQ9 proof or tests incomplete" \
    "Expected SEQ9 done, CDC proof/source anchors, MySQL durable cursor-floor anchors, passing cargo test -p nimbus-storage changefeed/durable_journal_stream, and cargo check -p nimbus-engine; captured outputs at ${CHANGEFEED_OUTPUT} and ${DURABLE_JOURNAL_STREAM_OUTPUT}"
fi

step 15 "SEQ10 generated MVCC conformance covers PITR and CDC against pure models"
GENERATED_MVCC_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-generated-mvcc.XXXXXX")"
DATADRIVEN_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-datadriven-mvcc.XXXXXX")"
GENERATED_HISTORY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-generated-history.XXXXXX")"
if contains "${PLAN}" 'SEQ10 | `done`' \
   && contains "${SEQ10_PROOF}" '^status: done$' \
   && contains "${SEQ10_PROOF}" 'GeneratedTaskHistory::datadriven' \
   && contains "${SEQ10_PROOF}" 'assert_generated_task_mvcc_history_matches_model' \
   && contains "${SEQ10_PROOF}" 'PITR-restored historical prefixes' \
   && contains "${SEQ10_PROOF}" 'CDC/changefeed document-write sequences' \
   && contains "${SEQ10_PROOF}" '1 passed, 0 failed' \
   && contains "${SEQ10_PROOF}" '8 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/simulation/generated.rs" 'pub fn datadriven' \
   && contains "crates/nimbus-storage/src/simulation/generated.rs" 'insert <slot> <status> <rank> <title>' \
   && contains "crates/nimbus-storage/src/simulation/generated.rs" 'updates missing slot' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'assert_generated_task_mvcc_history_matches_model' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'stream_changefeed' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'datadriven_generated_task_history_drives_mvcc_pitr_and_cdc_conformance' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'generated_mvcc_history_required_seed_corpus_matches_pitr_and_cdc_models' \
   && cargo test -p nimbus-storage generated_mvcc -- --nocapture >"${GENERATED_MVCC_OUTPUT}" 2>&1 \
   && grep -q '1 passed; 0 failed' "${GENERATED_MVCC_OUTPUT}" \
   && cargo test -p nimbus-storage datadriven_generated_task_history -- --nocapture >"${DATADRIVEN_OUTPUT}" 2>&1 \
   && grep -q '1 passed; 0 failed' "${DATADRIVEN_OUTPUT}" \
   && cargo test -p nimbus-storage generated_history -- --nocapture >"${GENERATED_HISTORY_OUTPUT}" 2>&1 \
   && grep -q '9 passed; 0 failed' "${GENERATED_HISTORY_OUTPUT}" \
   && grep -q '2 ignored' "${GENERATED_HISTORY_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ10 proof covers datadriven/generated MVCC latest-prefix, PITR-prefix, CDC-sequence, recovery, and storage compile evidence"
else
  fail "SEQ10 proof or tests incomplete" \
    "Expected SEQ10 done, generated MVCC proof/source anchors, passing generated_mvcc/datadriven/generated_history tests, and cargo check -p nimbus-storage; captured outputs at ${GENERATED_MVCC_OUTPUT}, ${DATADRIVEN_OUTPUT}, and ${GENERATED_HISTORY_OUTPUT}"
fi

step 16 "SEQ11 deterministic parity compares canonical digests across embedded backends"
PARITY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-deterministic-parity.XXXXXX")"
GENERATED_HISTORY_PARITY_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-generated-history-parity.XXXXXX")"
if contains "${PLAN}" 'SEQ11 | `done`' \
   && contains "${SEQ11_PROOF}" '^status: done$' \
   && contains "${SEQ11_PROOF}" 'canonical_digest_generated_history_matches_redb_sqlite_pitr_cdc_and_rebuild_paths' \
   && contains "${SEQ11_PROOF}" 'collect_changefeed_document_sequences' \
   && contains "${SEQ11_PROOF}" 'canonical_fingerprint' \
   && contains "${SEQ11_PROOF}" 'Confirmed Bug Fixed' \
   && contains "${SEQ11_PROOF}" 'Document::set_field' \
   && contains "${SEQ11_PROOF}" 'document.update_time = self.clock.now()' \
   && contains "${SEQ11_PROOF}" '1 passed, 0 failed' \
   && contains "${SEQ11_PROOF}" '9 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'canonical_digest_generated_history_matches_redb_sqlite_pitr_cdc_and_rebuild_paths' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'ManualClock::new' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'TableId::new' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'canonical_fingerprint' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'export_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'import_point_in_time_restore_archive' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'collect_changefeed_document_sequences' \
   && contains "crates/nimbus-storage/src/tests/generated_history.rs" 'stream_changefeed' \
   && contains "crates/nimbus-storage/src/store/journal_snapshot.rs" 'pub fn canonical_fingerprint' \
   && contains "crates/nimbus-storage/src/store/write/direct.rs" 'document.set_field(field.clone(), value.clone())' \
   && contains "crates/nimbus-storage/src/store/write/direct.rs" 'document.update_time = self.clock.now()' \
   && cargo test -p nimbus-storage canonical_digest_generated_history -- --nocapture >"${PARITY_OUTPUT}" 2>&1 \
   && grep -q '1 passed; 0 failed' "${PARITY_OUTPUT}" \
   && cargo test -p nimbus-storage generated_history -- --nocapture >"${GENERATED_HISTORY_PARITY_OUTPUT}" 2>&1 \
   && grep -q '9 passed; 0 failed' "${GENERATED_HISTORY_PARITY_OUTPUT}" \
   && grep -q '2 ignored' "${GENERATED_HISTORY_PARITY_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ11 proof covers deterministic redb/SQLite canonical digest parity, PITR/replay parity, CDC sequence cuts, and the redb update-time fix"
else
  fail "SEQ11 proof or tests incomplete" \
    "Expected SEQ11 done, deterministic parity proof/source anchors, redb update-time fix anchor, passing canonical_digest_generated_history and generated_history tests, and cargo check -p nimbus-storage; captured outputs at ${PARITY_OUTPUT} and ${GENERATED_HISTORY_PARITY_OUTPUT}"
fi

step 17 "SEQ12 operator diagnostics expose MVCC support, admission, pressure, and parity states"
DIAGNOSTIC_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-operator-diagnostics.XXXXXX")"
if contains "${PLAN}" 'SEQ12 | `done`' \
   && contains "${SEQ12_PROOF}" '^status: done$' \
   && contains "${SEQ12_PROOF}" 'IndexVersionStorageDiagnostic' \
   && contains "${SEQ12_PROOF}" 'MvccOperatorDiagnostic' \
   && contains "${SEQ12_PROOF}" 'HistoricalQueryAdmissionDiagnostic' \
   && contains "${SEQ12_PROOF}" 'StoragePressureDiagnostic' \
   && contains "${SEQ12_PROOF}" 'StorageCapabilityProfile' \
   && contains "${SEQ12_PROOF}" 'BackendParityDiagnostic' \
   && contains "${SEQ12_PROOF}" 'AdapterSupportDiagnostic' \
   && contains "${SEQ12_PROOF}" 'storage_health_diagnostic_with_retention_config' \
   && contains "${SEQ12_PROOF}" '15 passed, 0 failed' \
   && contains "crates/nimbus-core/src/error.rs" 'pub enum HistoricalReadErrorKind' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct IndexVersionStorageDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct MvccOperatorDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct HistoricalQueryAdmissionDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct StoragePressureDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub enum StorageCapabilityProfile' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub backend_capability_profile: StorageCapabilityProfile' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub capability_profile: StorageCapabilityProfile' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct BackendParityDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub struct AdapterSupportDiagnostic' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'pub fn storage_health_diagnostic_with_retention_config' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'storage_operator_diagnostics_cover_healthy_lagging_compacting_and_backend_divergence' \
   && contains "crates/nimbus-storage/src/diagnostics.rs" 'historical_query_admission_diagnostics_cover_expired_unsupported_format_and_policy_gates' \
   && contains "crates/nimbus-storage/src/store/index_versions.rs" 'pub fn index_version_storage_diagnostic' \
   && contains "crates/nimbus-storage/src/sqlite/index_versions.rs" 'pub fn index_version_storage_diagnostic' \
   && contains "crates/nimbus-storage/src/postgres/index_versions.rs" 'pub fn index_version_storage_diagnostic' \
   && contains "crates/nimbus-storage/src/mysql/index_versions.rs" 'pub fn index_version_storage_diagnostic' \
   && contains "crates/nimbus-storage/src/libsql/index_versions.rs" 'pub fn index_version_storage_diagnostic' \
   && contains "crates/nimbus-storage/src/lib.rs" 'IndexVersionStorageDiagnostic' \
   && cargo test -p nimbus-storage diagnostic -- --nocapture >"${DIAGNOSTIC_OUTPUT}" 2>&1 \
   && grep -q '15 passed; 0 failed' "${DIAGNOSTIC_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ12 proof covers MVCC operator diagnostics, retention knobs, admission states, support matrices, provider index diagnostics, parity divergence, and compile evidence"
else
  fail "SEQ12 proof or tests incomplete" \
    "Expected SEQ12 done, operator diagnostic proof/source anchors, passing cargo test -p nimbus-storage diagnostic, and cargo check -p nimbus-storage; captured output at ${DIAGNOSTIC_OUTPUT}"
fi

step 18 "SEQ13 performance evidence protects latest and historical-path budgets"
PERFORMANCE_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-performance.XXXXXX")"
if contains "${PLAN}" 'SEQ13 | `done`' \
   && contains "${SEQ13_PROOF}" '^status: done$' \
   && contains "${SEQ13_PROOF}" 'seq13 performance budget' \
   && contains "${SEQ13_PROOF}" 'docs/plans/research/sqlite-storage-benchmark-report.md' \
   && contains "${SEQ13_PROOF}" 'docs/plans/proof/storage-engine-quality-and-mvcc/seq0-embedded-point-read-baseline.md' \
   && contains "${SEQ13_PROOF}" 'docs/plans/research/postgres-provider-benchmark-report.md' \
   && contains "${SEQ13_PROOF}" 'docs/plans/research/mysql-provider-benchmark-report.md' \
   && contains "${SEQ13_PROOF}" 'docs/plans/research/sqlite-replica-provider-benchmark-report.md' \
   && contains "${SEQ13_PROOF}" 'latest point reads' \
   && contains "${SEQ13_PROOF}" 'historical point reads' \
   && contains "${SEQ13_PROOF}" 'historical index pagination' \
   && contains "${SEQ13_PROOF}" 'CDC stream' \
   && contains "${SEQ13_PROOF}" 'PITR export/import' \
   && contains "${SEQ13_PROOF}" 'retention compaction' \
   && contains "${SEQ13_PROOF}" '1 passed, 0 failed' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'fn assert_seq13_budget' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'document-version write amplification should stay bounded' \
   && contains "crates/nimbus-storage/src/tests/crud_and_journal.rs" 'index-version write amplification should stay bounded' \
   && cargo test -p nimbus-storage redb_storage_engine_quality_performance_budget -- --nocapture >"${PERFORMANCE_OUTPUT}" 2>&1 \
   && grep -q 'seq13 performance budget' "${PERFORMANCE_OUTPUT}" \
   && grep -q '1 passed; 0 failed' "${PERFORMANCE_OUTPUT}" \
   && cargo check -p nimbus-storage >/dev/null 2>&1; then
  pass "SEQ13 proof covers current benchmark reports, latest/historical/CDC/PITR/GC budget smoke tests, bounded write amplification, and storage compile evidence"
else
  fail "SEQ13 proof or tests incomplete" \
    "Expected SEQ13 done, performance proof/report anchors, passing cargo test -p nimbus-storage redb_storage_engine_quality_performance_budget, and cargo check -p nimbus-storage; captured output at ${PERFORMANCE_OUTPUT}"
fi

step 19 "SATH baseline verifier still passes"
SATH_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/seq-sath.XXXXXX")"
if bash "${SATH_VERIFIER}" >"${SATH_OUTPUT}" 2>&1 \
   && grep -q 'Summary:.*12 passed, 0 failed' "${SATH_OUTPUT}"; then
  pass "SATH baseline passes before SEQ implementation"
else
  fail "SATH baseline failed" "Run ${SATH_VERIFIER} for details; captured output at ${SATH_OUTPUT}"
fi

step 20 "Closeout requires architecture docs and PR"
if contains "${PLAN}" 'ARCHITECTURE.md' \
   && contains "${PLAN}" 'docs/architecture/storage/persistence-engine-baseline.md' \
   && contains "${PLAN}" 'pull request' \
   && contains "${PLAN}" 'pushed branch' \
   && contains "${PLAN}" 'draft PR' \
   && contains "${SEQ14_PROOF}" '^status: done$' \
   && contains "${SEQ14_PROOF}" 'ARCHITECTURE.md' \
   && contains "${SEQ14_PROOF}" 'docs/architecture/storage/persistence-engine-baseline.md' \
   && contains "${SEQ14_PROOF}" 'docs/operating/storage-backends.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/convex/compatibility.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/firebase/compatibility.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/cloud-functions/compatibility.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/mongodb/operations.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/dynamodb/enterprise-readiness.md' \
   && contains "${SEQ14_PROOF}" 'docs/adapters/native/README.md' \
   && contains "${SEQ14_PROOF}" 'Draft PR URL: `https://github.com/nimbus/nimbus/pull/13`' \
   && contains "${SEQ14_PROOF}" 'Branch push and draft PR creation are complete' \
   && contains "${SEQ14_PROOF}" '20 passed, 0 failed' \
   && contains "${SEQ14_PROOF}" 'npm run build -w nimbus-ui' \
   && contains "${SEQ14_PROOF}" 'snapshot_unavailable_historical_read_maps_to_service_unavailable' \
   && contains "${SEQ14_PROOF}" 'crates/nimbus-storage/src/index/history_scan.rs' \
   && contains "${SEQ14_PROOF}" 'MySQL durable-journal stream/bootstrap' \
   && contains "${SEQ14_PROOF}" 'crates/nimbus-storage/src/mysql/table_catalog.rs' \
   && contains "${SEQ14_PROOF}" 'crates/nimbus-storage/src/mysql/query_helpers.rs' \
   && contains "${SEQ14_PROOF}" 'crates/nimbus-storage/src/postgres/query_helpers.rs' \
   && contains "${SEQ14_PROOF}" 'crates/nimbus-storage/src/postgres/write_schema_events.rs' \
   && contains "${SEQ14_PROOF}" 'mysql/backend.rs` 1329' \
   && contains "${SEQ14_PROOF}" 'postgres/backend.rs` 1470' \
   && contains "${SEQ14_PROOF}" 'postgres/write.rs` 1476' \
   && contains "${SEQ14_PROOF}" 'historical_index -- --nocapture' \
   && contains "${SEQ14_PROOF}" 'durable_journal_stream -- --nocapture' \
   && contains "${SEQ14_PROOF}" 'cargo fmt --all --check' \
   && contains "${SEQ14_PROOF}" 'npm run docs:validate-refs:strict' \
   && contains "${SEQ14_PROOF}" 'git diff --check' \
   && contains "${SEQ14_PROOF}" 'libSQL diagnostics freshness bug is fixed' \
   && contains "ARCHITECTURE.md" 'latest-row plus version-history architecture' \
   && contains "docs/architecture/storage/persistence-engine-baseline.md" 'MVCC, PITR, CDC, and retention contract' \
   && contains "docs/architecture/storage/persistence-engine-baseline.md" 'mysql/table_catalog.rs' \
   && contains "docs/architecture/storage/persistence-engine-baseline.md" 'postgres/write_schema_events.rs' \
   && contains "${PLAN}" 'SQL-family production storage roots stay below' \
   && contains "${PLAN}" 'crates/nimbus-storage/src/mysql/table_catalog.rs' \
   && contains "${PLAN}" 'crates/nimbus-storage/src/postgres/write_schema_events.rs' \
   && contains "crates/nimbus-storage/src/mysql.rs" 'mod query_helpers' \
   && contains "crates/nimbus-storage/src/mysql.rs" 'mod table_catalog' \
   && contains "crates/nimbus-storage/src/postgres.rs" 'mod query_helpers' \
   && contains "crates/nimbus-storage/src/postgres.rs" 'mod write_schema_events' \
   && contains "crates/nimbus-storage/src/mysql/table_catalog.rs" 'load_table_id_from_session' \
   && contains "crates/nimbus-storage/src/mysql/query_helpers.rs" 'filter_index_documents_with_cancel' \
   && contains "crates/nimbus-storage/src/postgres/query_helpers.rs" 'append_postgres_range_clause' \
   && contains "crates/nimbus-storage/src/postgres/write_schema_events.rs" 'durable_record_changes_schema_cache' \
   && line_count_at_most "crates/nimbus-storage/src/mysql/backend.rs" 1499 \
   && line_count_at_most "crates/nimbus-storage/src/postgres/backend.rs" 1499 \
   && line_count_at_most "crates/nimbus-storage/src/postgres/write.rs" 1499 \
   && contains "docs/operating/storage-backends.md" 'Historical reads use retained' \
   && contains "docs/adapters/convex/compatibility.md" 'Storage Semantics Inherited By Convex' \
   && contains "docs/adapters/firebase/compatibility.md" 'Storage Semantics Inherited By Firebase' \
   && contains "docs/adapters/cloud-functions/compatibility.md" 'Storage Semantics Inherited By Cloud Functions' \
   && contains "docs/adapters/mongodb/operations.md" 'Storage Semantics' \
   && contains "docs/adapters/mongodb/operations.md" 'CommandNotSupported' \
   && contains "docs/adapters/native/README.md" 'not implicitly' \
   && contains "docs/adapters/native/README.md" 'UnsupportedAdapter' \
   && contains "packages/nimbus-ui/package.json" 'node ../codegen/src/cli.mjs --app .' \
   && ! grep -q 'convex codegen --app' packages/nimbus-ui/package.json Makefile; then
  pass "Plan closeout records architecture docs, adapter docs, pushed branch, draft PR, verification, and final proof"
else
  fail "SEQ14 closeout contract incomplete" \
    "Expected architecture doc updates, adapter doc updates, SEQ14 proof, verification counts, pushed branch, draft PR URL, and pull request requirements"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
