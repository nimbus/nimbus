#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/private/plans/archive/convex-storage-trust-hardening-plan.md.
#
# Keep this gate tied to behavior-bearing code and tests, not only narrative
# proof files.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/archive/convex-storage-trust-hardening-plan.md"
PROOF_DIR="docs/private/plans/proof/convex-storage-trust-hardening"
DEBT_DOC="docs/private/technical-debt.md"

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

row_status() {
  local id="$1"
  awk -F'|' -v id="$id" '
    $2 ~ "^[[:space:]]*" id "[[:space:]]*$" {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", $6)
      print $6
      exit
    }
  ' "${DEBT_DOC}"
}

has_debt_owner() {
  local owner="$1"
  grep -Eq "^\|[[:space:]]*[A-Z]-[0-9]+[[:space:]]*\|.*\|[[:space:]]*${owner}[[:space:]]*\|" "${DEBT_DOC}"
}

printf '\033[1mCST verification gate - convex-storage-trust-hardening\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Plan and baseline proof exist"
if [ -f "${PLAN}" ] && [ -d "${PROOF_DIR}" ] && [ -f "${PROOF_DIR}/cst0-convex-storage-comparison.md" ]; then
  pass "Plan and CST0 proof exist"
else
  fail "Plan or CST0 proof missing" "Expected ${PLAN} and ${PROOF_DIR}/cst0-convex-storage-comparison.md"
fi

step 2 "Archived routing entries exist"
if grep -q "${PLAN}" AGENTS.md && grep -q "Completed plans are stored in \`docs/private/plans/archive/\`" docs/private/plans/README.md; then
  pass "Archived plan is routed from AGENTS.md and archive policy is documented"
else
  fail "Archived routing entries missing" "Expected ${PLAN} in AGENTS.md and archive policy in docs/private/plans/README.md"
fi

step 3 "Debt ledger closes stale TableId rows and CST1 proof exists"
if [ ! -f "${DEBT_DOC}" ]; then
  fail "Debt document missing" "Expected ${DEBT_DOC}"
else
  S004="$(row_status S-004)"
  A003="$(row_status A-003)"
  CST1_PROOF="${PROOF_DIR}/cst1-table-catalog-closeout.md"
  if [ "${S004}" != "open" ] && [ "${A003}" != "open" ] \
    && [ -f "${CST1_PROOF}" ] \
    && grep -q '^status: done$' "${CST1_PROOF}" \
    && has_debt_owner "CST2" && has_debt_owner "CST3" && has_debt_owner "CST4" && has_debt_owner "CST5" && has_debt_owner "CST6" && has_debt_owner "CST7"; then
    pass "Debt ledger and CST1 table-catalog proof are complete"
  else
    fail "Debt ledger or CST1 proof incomplete" "S-004=${S004:-missing}, A-003=${A003:-missing}; expected ${CST1_PROOF} with status: done and CST2-CST7 rows"
  fi
fi

step 4 "Table lifecycle state implemented and exercised"
if grep -R "enum TableState" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "stage_hidden_table_identity" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "activate_hidden_table_identity" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "hard_delete_table_identity" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "native_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "sqlite_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && [ -f "${PROOF_DIR}/cst2-table-lifecycle.md" ] \
   && grep -q '^status: done$' "${PROOF_DIR}/cst2-table-lifecycle.md"; then
  pass "Table lifecycle state, operations, and behavior tests exist"
else
  fail "Table lifecycle missing" "Expected TableState, lifecycle operations, native/sqlite lifecycle tests, and cst2-table-lifecycle.md with status: done"
fi

step 5 "Table-aware document identity implemented"
if grep -R "struct ResolvedDocumentId\|enum ResolvedDocumentId" crates/nimbus-core/src crates/nimbus-engine/src crates/nimbus-server/src >/dev/null 2>&1 \
   && [ -f "${PROOF_DIR}/cst3-table-aware-document-identity.md" ]; then
  pass "Resolved document identity and proof exist"
else
  fail "Table-aware document identity missing" "Expected ResolvedDocumentId and CST3 proof"
fi

step 6 "Dependency tracking uses stable table identity"
if grep -R "TableId" crates/nimbus-core/src/dependency.rs crates/nimbus-server/src/execution/read_tracking >/dev/null 2>&1 \
   && [ -f "${PROOF_DIR}/cst4-table-id-dependencies.md" ]; then
  pass "Dependency tracking references TableId"
else
  fail "TableId dependency tracking missing" "Expected DependencySet/read_tracking TableId usage and CST4 proof"
fi

step 7 "Index identity lifecycle exists and schema publishes reconciled IDs"
if grep -R "struct IndexId\|enum IndexState\|enum IndexLifecycle" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "reconcile_index_metadata" crates/nimbus-core/src crates/nimbus-engine/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "set_table_schema_publishes_reconciled_index_identity" crates/nimbus-engine/src/tests >/dev/null 2>&1 \
   && [ -f "${PROOF_DIR}/cst5-index-identity-lifecycle.md" ]; then
  pass "Index identity/lifecycle, reconciliation, and behavior test exist"
else
  fail "Index identity/lifecycle missing" "Expected IndexId/IndexState, schema reconciliation test, and CST5 proof"
fi

step 8 "History/repeatable-read posture is documented"
if [ -f "${PROOF_DIR}/cst6-history-repeatable-read-decision.md" ] \
   && grep -Eq "posture:[[:space:]]*(adopted|intentionally_latest_row)" "${PROOF_DIR}/cst6-history-repeatable-read-decision.md"; then
  pass "History/repeatable-read posture is explicit"
else
  fail "History/repeatable-read decision missing" "Expected CST6 proof with posture"
fi

step 9 "Read-only diagnostics exist"
if grep -R "TableIdentityDiagnostic" crates/nimbus-core/src crates/nimbus-storage/src crates/nimbus-engine/src crates/nimbus-server/src >/dev/null 2>&1 \
   && [ -f "${PROOF_DIR}/cst7-diagnostics-summaries.md" ]; then
  pass "Table identity diagnostics and proof exist"
else
  fail "Diagnostics missing" "Expected TableIdentityDiagnostic and CST7 proof"
fi

step 10 "Closeout behavior and evidence exist"
if [ -f "${PROOF_DIR}/cst8-cross-backend-conformance.md" ] \
   && [ -f "${PROOF_DIR}/cst9-closeout.md" ] \
   && grep -q "10 passed, 0 failed" "${PROOF_DIR}/cst9-closeout.md" \
   && grep -R "redb_durable_replay_retires_recreated_table_identity" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "sqlite_durable_replay_retires_recreated_table_identity" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "postgres_durable_replay_retires_recreated_table_identity" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "mysql_durable_replay_retires_recreated_table_identity" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "libsql_durable_replay_retires_recreated_table_identity" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && grep -R "shadow_materializer_promotes_recreated_table_and_exports_only_active_documents" crates/nimbus-storage/src/materializer >/dev/null 2>&1 \
   && grep -R "materialized_snapshot_rejects_lifecycle_namespace_state_mismatch" crates/nimbus-storage/src/store >/dev/null 2>&1 \
   && grep -q "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES" crates/nimbus-storage/src/tests/provider_fixtures.rs \
   && grep -R "require_explicit_external_provider_fixture_envs" crates/nimbus-storage/src/tests/postgres_provider.rs crates/nimbus-storage/src/tests/mysql_provider.rs crates/nimbus-storage/src/tests/libsql_provider.rs >/dev/null 2>&1; then
  pass "Cross-backend replay tests, fixture guards, and closeout proof exist"
else
  fail "Closeout behavior or evidence missing" "Expected CST8/CST9 proof, replay/materializer/snapshot tests, and explicit external fixture guard"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
