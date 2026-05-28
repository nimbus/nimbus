#!/usr/bin/env bash
# Aggregate completion-gate verifier for
# docs/plans/archive/storage-architecture-trust-hardening-plan.md.
#
# This gate is intentionally behavior-biased: proof files are necessary, but
# each completed phase must also leave typed code, tests, or docs that can be
# audited without reconstructing the implementation history.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/plans/archive/storage-architecture-trust-hardening-plan.md"
PROOF_DIR="docs/plans/proof/storage-architecture-trust-hardening"
DEBT_DOC="docs/technical-debt.md"

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

proof_done() {
  local proof="$1"
  [ -f "${proof}" ] && grep -q '^status: done$' "${proof}"
}

printf '\033[1mSATH verification gate - storage-architecture-trust-hardening\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Plan, proof bundle, debt rows, and verifier exist"
SATH_DEBT_COUNT=0
if [ -f "${DEBT_DOC}" ]; then
  SATH_DEBT_COUNT="$(grep -Ec '^\|[[:space:]]*[A-Z]-[0-9]+[[:space:]]*\|.*\|[[:space:]]*SATH[0-9]+' "${DEBT_DOC}")"
fi
if [ -f "${PLAN}" ] \
   && [ -d "${PROOF_DIR}" ] \
   && proof_done "${PROOF_DIR}/sath0-review.md" \
   && [ -x "scripts/verify-storage-architecture-trust-hardening.sh" ] \
   && [ "${SATH_DEBT_COUNT}" -ge 8 ] \
   && grep -q "${PLAN}" docs/plans/README.md; then
  pass "SATH0 control-plane artifacts exist"
else
  fail "SATH0 artifacts incomplete" "Expected plan, executable verifier, docs/plans routing, sath0 proof with status: done, and >= 8 SATH debt rows"
fi

step 2 "Tenant event journal covers replay-affecting state"
if grep -R "struct TenantEventRecord\|enum TenantEventKind" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "DocumentWrite" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "SchemaChange\|SchemaSet" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "TableLifecycle" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "IndexLifecycle\|IndexChange" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "Scheduler\|ScheduledExecution" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "TriggerDelivery" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath1-tenant-event-journal.md"; then
  pass "Typed tenant event journal model and SATH1 proof exist"
else
  fail "Tenant event journal incomplete" "Expected TenantEventRecord/TenantEventKind covering documents, schema, table lifecycle, index lifecycle, scheduler, trigger delivery, and SATH1 proof"
fi

step 3 "Replay-affecting writes append events atomically"
if grep -R "append_tenant_event" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "record_tenant_event(TenantEventKind::SchemaChange" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "record_tenant_event(TenantEventKind::IndexLifecycle" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "record_tenant_event(TenantEventKind::TableLifecycle" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "record_tenant_event(TenantEventKind::ScheduledExecution" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "record_tenant_event(TenantEventKind::TriggerDelivery" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "tenant_event_journal_appends_schema_table_index_scheduler_and_trigger_events_atomically" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath2-replay-snapshot-materializer.md"; then
  pass "Atomic event append/apply coverage exists"
else
  fail "Atomic event append/apply coverage missing" "Expected append_tenant_event journal storage plus record_tenant_event usage across schema/index/lifecycle/scheduler/trigger paths, behavior test, and SATH2 proof"
fi

step 4 "External backends implement the same event journal contract"
if grep -R "tenant_event" crates/nimbus-storage/src/postgres crates/nimbus-storage/src/mysql crates/nimbus-storage/src/libsql >/dev/null 2>&1 \
   && grep -R "postgres_tenant_event_journal_replays_mixed_history" crates/nimbus-storage/src/tests crates/nimbus-engine/src/tests >/dev/null 2>&1 \
   && grep -R "mysql_tenant_event_journal_replays_mixed_history" crates/nimbus-storage/src/tests crates/nimbus-engine/src/tests >/dev/null 2>&1 \
   && grep -R "libsql_tenant_event_journal_replays_mixed_history" crates/nimbus-storage/src/tests crates/nimbus-engine/src/tests >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath3-external-backend-event-journal.md"; then
  pass "External backend event-journal proof exists"
else
  fail "External backend event journal incomplete" "Expected Postgres/MySQL/libSQL tenant_event storage, mixed replay tests, and SATH3 proof"
fi

step 5 "Hard delete is retention-gated"
if grep -R "struct RetentionFloor\|enum RetentionParticipant\|HardDeleteDecision" crates/nimbus-core/src crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "hard_delete.*retention" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "hard_delete_denied_while_retention_floor_pins_table_identity" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "retention_floor_survives_crash_recovery" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath4-retention-hard-delete.md"; then
  pass "Retention floor gates destructive table cleanup"
else
  fail "Retention-gated hard delete missing" "Expected retention floor types, hard-delete gate, crash-recovery tests, and SATH4 proof"
fi

step 6 "Read visibility routes through typed APIs"
if grep -R "enum ReadVisibility\|struct RequiredSequence\|struct PinnedServingSnapshot" crates/nimbus-core/src crates/nimbus-engine/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "wait_for_snapshot_covering" crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "read_visibility_waits_for_required_sequence" crates/nimbus-engine/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath5-read-visibility.md"; then
  pass "Typed read visibility boundary exists"
else
  fail "Typed read visibility incomplete" "Expected ReadVisibility/RequiredSequence/PinnedServingSnapshot, behavior test, and SATH5 proof"
fi

step 7 "Storage capabilities and health diagnostics exist"
if grep -R "struct StorageCapabilities\|struct StorageHealthDiagnostic" crates/nimbus-core/src crates/nimbus-storage/src crates/nimbus-engine/src crates/nimbus-server/src >/dev/null 2>&1 \
   && grep -R "event_log_head\|applied_head\|retention_floor\|format_version\|encryption_posture\|freshness_lag" crates/nimbus-core/src crates/nimbus-storage/src crates/nimbus-engine/src crates/nimbus-server/src >/dev/null 2>&1 \
   && grep -R "storage_health_diagnostic_reports_backend_layout_and_heads" crates/nimbus-storage/src crates/nimbus-engine/src crates/nimbus-server/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath6-capabilities-health.md"; then
  pass "Storage capability and health diagnostics exist"
else
  fail "Storage diagnostics incomplete" "Expected StorageCapabilities, StorageHealthDiagnostic, backend-head fields, behavior tests, and SATH6 proof"
fi

step 8 "Backend storage format/version gates fail closed"
if grep -R "StorageFormatVersion\|CURRENT_STORAGE_FORMAT_VERSION\|storage_format_version" crates/nimbus-storage/src crates/nimbus-core/src >/dev/null 2>&1 \
   && grep -R "unknown.*format.*version\|unsupported.*format.*version" crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "unknown_storage_format_version_is_rejected" crates/nimbus-storage/src >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath7-format-versioning.md"; then
  pass "Format/version startup validation exists"
else
  fail "Format/version gates missing" "Expected storage format metadata, unknown-version rejection tests, and SATH7 proof"
fi

step 9 "Table lifecycle transition rules are shared"
if grep -R "enum TableLifecycleTransition\|struct TableLifecycleStateMachine\|fn apply_table_lifecycle_transition" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "table_lifecycle_state_machine_rejects_invalid_transitions" crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1 \
   && grep -R "uses_shared_table_lifecycle_transition" crates/nimbus-storage/src/tests >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath8-lifecycle-state-machine.md"; then
  pass "Shared table lifecycle transition machine exists"
else
  fail "Shared lifecycle transition rules missing" "Expected pure lifecycle transition machine, tests, backend conformance marker, and SATH8 proof"
fi

step 10 "Generated/metamorphic storage conformance covers mixed histories"
if grep -R "storage_conformance_required_seed_corpus_matches_model\|generated_storage_history_includes_schema_index_lifecycle_scheduler_retention" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "NIMBUS_STORAGE_CONFORMANCE_SEED\|NIMBUS_VERIFY_CASE" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
   && grep -R "crash.*replay.*diagnostic\|retention.*snapshot.*diagnostic" crates/nimbus-storage/src/tests crates/nimbus-storage/src/simulation >/dev/null 2>&1 \
   && proof_done "${PROOF_DIR}/sath9-generated-conformance.md"; then
  pass "Generated storage conformance exists"
else
  fail "Generated storage conformance incomplete" "Expected mixed-history seed corpus, deterministic seed replay, crash/replay/retention diagnostics coverage, and SATH9 proof"
fi

step 11 "Operator and architecture docs describe the storage trust contract"
if grep -q "tenant event journal" docs/architecture/storage/persistence-engine-baseline.md \
   && grep -q "StorageHealthDiagnostic" docs/operating/storage-backends.md \
   && grep -q "retention floor" docs/architecture/storage/persistence-engine-baseline.md \
   && grep -q "storage format version" docs/architecture/storage/persistence-engine-baseline.md \
   && proof_done "${PROOF_DIR}/sath10-docs-operator-evidence.md"; then
  pass "Operator docs and architecture docs are updated"
else
  fail "Storage trust docs incomplete" "Expected tenant event journal, retention, diagnostics, and format-version docs plus SATH10 proof"
fi

step 12 "Closeout proof records final verification"
if proof_done "${PROOF_DIR}/sath11-closeout.md" \
   && grep -q "12 passed, 0 failed" "${PROOF_DIR}/sath11-closeout.md" \
   && [ "$(row_status A-011)" = "done" ] \
   && [ "$(row_status A-012)" = "done" ] \
   && [ "$(row_status A-013)" = "done" ] \
   && [ "$(row_status A-014)" = "done" ] \
   && [ "$(row_status A-015)" = "done" ] \
   && [ "$(row_status T-007)" = "done" ] \
   && [ "$(row_status O-005)" = "done" ] \
   && [ "$(row_status O-006)" = "done" ]; then
  pass "Closeout proof and debt statuses are complete"
else
  fail "Closeout incomplete" "Expected SATH11 closeout proof with final verifier output and all SATH debt rows marked done"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
