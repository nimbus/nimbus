#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Multi-Backend Multi-Adapter
# Hardening plan (`docs/private/plans/multi-backend-adapter-hardening-plan.md`).
#
# Ships in MBA0 so the plan can be audited from day one. Most conditions are
# expected to fail until MBA1-MBA14 land.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/multi-backend-adapter-hardening-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/multi-backend-adapter-hardening-plan.md"
PROOF_DIR="docs/private/plans/proof/multi-backend-adapter-hardening"
AGENTS_MD="AGENTS.md"

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

plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then
    printf '%s\n' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then
    printf '%s\n' "${PLAN_ARCHIVED}"
  else
    printf ''
  fi
}

line_count() {
  wc -l | tr -d ' '
}

printf '\033[1mMBA verification gate - multi-backend-adapter-hardening\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan file and MBA0 proof files exist.
step 1 "Plan file and MBA0 proof files exist"
PLAN_FILE="$(plan_file)"
MBA0_BASELINE="${PROOF_DIR}/mba0-baseline.md"
MBA0_PATTERN_MAP="${PROOF_DIR}/mba0-extenddb-pattern-map.md"
MBA0_RIGOR="${PROOF_DIR}/mba0-plan-rigor-review.md"
if [ -n "${PLAN_FILE}" ] && [ -f "${MBA0_BASELINE}" ] && [ -f "${MBA0_PATTERN_MAP}" ] && [ -f "${MBA0_RIGOR}" ]; then
  pass "Plan exists at ${PLAN_FILE}; MBA0 proof files exist"
else
  DETAIL=()
  [ -z "${PLAN_FILE}" ] && DETAIL+=("plan missing")
  [ ! -f "${MBA0_BASELINE}" ] && DETAIL+=("${MBA0_BASELINE} missing")
  [ ! -f "${MBA0_PATTERN_MAP}" ] && DETAIL+=("${MBA0_PATTERN_MAP} missing")
  [ ! -f "${MBA0_RIGOR}" ] && DETAIL+=("${MBA0_RIGOR} missing")
  fail "Plan or MBA0 proof missing" "$(printf '%s; ' "${DETAIL[@]}")"
fi

# 2. docs/private/technical-debt.md exists with >= 20 entries, seven fields, >= 5 categories.
step 2 "Technical debt tracker exists and is structured"
DEBT_DOC="docs/private/technical-debt.md"
DEBT_SEED_PROOF="${PROOF_DIR}/mba1-technical-debt-seed.md"
if [ ! -f "${DEBT_DOC}" ]; then
  fail "docs/private/technical-debt.md missing" "MBA1 must create the debt tracker"
else
  ENTRY_COUNT="$(grep -E '^\|[[:space:]]*[FCSTAPO]-[0-9]+' "${DEBT_DOC}" | line_count)"
  CATEGORY_COUNT="$(grep -E '^\|[[:space:]]*[FCSTAPO]-[0-9]+' "${DEBT_DOC}" \
    | sed -E 's/^\|[[:space:]]*([FCSTAPO])-.*/\1/' | sort -u | line_count)"
  BAD_FIELD_COUNT="$(awk '
    /^\|[[:space:]]*[FCSTAPO]-[0-9]+/ {
      pipes = gsub(/\|/, "|")
      if (pipes < 8) bad++
    }
    END { print bad + 0 }
  ' "${DEBT_DOC}")"
  SEED_DETAIL=()
  if [ ! -f "${DEBT_SEED_PROOF}" ]; then
    SEED_DETAIL+=("${DEBT_SEED_PROOF} missing")
  else
    grep -q '^source_scope:' "${DEBT_SEED_PROOF}" || SEED_DETAIL+=("source_scope missing")
    grep -q '^excluded_paths:' "${DEBT_SEED_PROOF}" || SEED_DETAIL+=("excluded_paths missing")
  fi
  if [ "${ENTRY_COUNT}" -ge 20 ] && [ "${CATEGORY_COUNT}" -ge 5 ] && [ "${BAD_FIELD_COUNT}" -eq 0 ] && [ "${#SEED_DETAIL[@]}" -eq 0 ]; then
    pass "Debt tracker has ${ENTRY_COUNT} entries across ${CATEGORY_COUNT} categories"
  else
    fail "Debt tracker incomplete" "entries=${ENTRY_COUNT}, categories=${CATEGORY_COUNT}, bad_field_rows=${BAD_FIELD_COUNT}, seed=$(printf '%s; ' "${SEED_DETAIL[@]}")"
  fi
fi

# 3. Focused storage traits exist and are free of stub implementations.
step 3 "Storage trait segregation exists"
TRAITS_DIR="crates/nimbus-storage/src/traits"
TRAIT_PROOF="${PROOF_DIR}/mba2-storage-trait-split.md"
if [ ! -d "${TRAITS_DIR}" ]; then
  fail "Focused storage traits missing" "Expected ${TRAITS_DIR}"
else
  TRAIT_DETAIL=()
  [ ! -f "${TRAIT_PROOF}" ] && TRAIT_DETAIL+=("${TRAIT_PROOF} missing")
  FOCUSED_TRAIT_COUNT="$(grep -R "^[[:space:]]*pub trait " "${TRAITS_DIR}" 2>/dev/null | line_count)"
  if [ "${FOCUSED_TRAIT_COUNT}" -lt 6 ]; then
    TRAIT_DETAIL+=("found ${FOCUSED_TRAIT_COUNT} focused traits, expected >= 6")
  fi
  if ! grep -RE "Composite|StorageEngine|trait .*: .*\\+" "${TRAITS_DIR}" >/dev/null 2>&1; then
    TRAIT_DETAIL+=("composite trait or documented aggregate missing")
  fi
  if grep -R "unimplemented!()" "${TRAITS_DIR}" >/dev/null 2>&1; then
    fail "Focused trait implementation contains unimplemented!()" "Remove stub implementations"
  elif [ "${#TRAIT_DETAIL[@]}" -eq 0 ]; then
    pass "Focused traits and composite posture exist"
  else
    fail "Focused traits incomplete" "$(printf '%s; ' "${TRAIT_DETAIL[@]}")"
  fi
fi

# 4. Registration seam proof exists and the selected posture is implemented.
step 4 "Adapter/backend registration seam is documented"
REGISTRATION_PROOF="${PROOF_DIR}/mba3-registration-seam.md"
if [ ! -f "${REGISTRATION_PROOF}" ]; then
  fail "Registration seam proof missing" "Expected ${REGISTRATION_PROOF}"
else
  REGISTRATION_POSTURE="$(sed -n 's/^posture:[[:space:]]*//p' "${REGISTRATION_PROOF}" | head -n 1)"
  case "${REGISTRATION_POSTURE}" in
    explicit_typed_registry)
      REGISTRATION_DETAIL=()
      if ! grep -Eq '^allowed_boundaries:' "${REGISTRATION_PROOF}"; then
        REGISTRATION_DETAIL+=("allowed_boundaries line missing")
      fi
      if ! grep -q 'PersistenceProvider' "${REGISTRATION_PROOF}"; then
        REGISTRATION_DETAIL+=("PersistenceProvider boundary not documented")
      fi
      if ! grep -q 'TenantProviderBootstrapPlan' "${REGISTRATION_PROOF}"; then
        REGISTRATION_DETAIL+=("TenantProviderBootstrapPlan boundary not documented")
      fi
      if grep -R '^[[:space:]]*inventory[[:space:]]*=' Cargo.toml crates/*/Cargo.toml >/dev/null 2>&1; then
        REGISTRATION_DETAIL+=("direct inventory dependency present despite explicit_typed_registry posture")
      fi
      if [ "${#REGISTRATION_DETAIL[@]}" -eq 0 ]; then
        pass "Explicit typed registration seam is documented"
      else
        fail "Explicit typed registration seam incomplete" "$(printf '%s; ' "${REGISTRATION_DETAIL[@]}")"
      fi
      ;;
    inventory_registration)
      REGISTRATION_DETAIL=()
      if ! grep -R '^[[:space:]]*inventory[[:space:]]*=' Cargo.toml crates/*/Cargo.toml >/dev/null 2>&1; then
        REGISTRATION_DETAIL+=("inventory is not a direct dependency")
      fi
      if ! grep -R "struct AdapterRegistration" crates >/dev/null 2>&1; then
        REGISTRATION_DETAIL+=("AdapterRegistration missing")
      fi
      if ! grep -R "struct BackendRegistration" crates >/dev/null 2>&1 \
         && ! grep -R "struct .*Backend.*Registration" crates >/dev/null 2>&1; then
        REGISTRATION_DETAIL+=("BackendRegistration missing")
      fi
      if [ "${#REGISTRATION_DETAIL[@]}" -eq 0 ]; then
        pass "Inventory registration seam is implemented"
      else
        fail "Inventory registration seam incomplete" "$(printf '%s; ' "${REGISTRATION_DETAIL[@]}")"
      fi
      ;;
    "")
      fail "Registration seam proof missing posture" "Expected posture: explicit_typed_registry or posture: inventory_registration"
      ;;
    *)
      fail "Registration seam proof has unknown posture" "${REGISTRATION_POSTURE}"
      ;;
  esac
fi

# 5. RuntimeHooks exists; every backend implements or opts out.
step 5 "RuntimeHooks owns backend-coupled workers"
HOOK_DETAIL=()
if ! grep -R "trait RuntimeHooks" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1; then
  HOOK_DETAIL+=("RuntimeHooks trait missing")
fi
RUNTIME_HOOKS_PROOF="${PROOF_DIR}/mba4-runtime-hooks.md"
[ ! -f "${RUNTIME_HOOKS_PROOF}" ] && HOOK_DETAIL+=("${RUNTIME_HOOKS_PROOF} missing")
for backend in redb sqlite postgres mysql libsql; do
  if ! grep -Ri "RuntimeHooks for .*${backend}" crates/nimbus-storage/src crates/nimbus-engine/src >/dev/null 2>&1 \
     && ! grep -Ri "no backend-coupled workers.*${backend}\|${backend}.*no backend-coupled workers" crates/nimbus-storage/src crates/nimbus-engine/src "${RUNTIME_HOOKS_PROOF}" >/dev/null 2>&1; then
    HOOK_DETAIL+=("${backend} coverage missing")
  fi
done
if grep -R "run_postgres_provider_hint_worker\|run_mysql_provider_poll_worker\|run_libsql_replica_provider_poll_worker" crates/nimbus-engine/src >/dev/null 2>&1; then
  HOOK_DETAIL+=("engine still names provider-specific worker functions")
fi
if [ "${#HOOK_DETAIL[@]}" -eq 0 ]; then
  pass "RuntimeHooks coverage is complete"
else
  fail "RuntimeHooks coverage incomplete" "$(printf '%s; ' "${HOOK_DETAIL[@]}")"
fi

# 6. Dual-target tests exist per adapter.
step 6 "Dual-target tests exist per adapter"
DUAL_DETAIL=()
for adapter in convex firebase cloud_functions mongodb; do
  dir="tests/dual-target/${adapter}"
  if [ ! -d "${dir}" ]; then
    DUAL_DETAIL+=("${dir} missing")
  elif ! grep -R "NIMBUS_TEST_TARGET" "${dir}" >/dev/null 2>&1; then
    DUAL_DETAIL+=("${dir} lacks NIMBUS_TEST_TARGET")
  fi
done
if [ "${#DUAL_DETAIL[@]}" -eq 0 ]; then
  pass "Dual-target tests exist for all adapters"
else
  fail "Dual-target tests incomplete" "$(printf '%s; ' "${DUAL_DETAIL[@]}")"
fi

# 7. Dual-target nightly workflow exists.
step 7 "Dual-target nightly workflow exists"
DUAL_WORKFLOW=".github/workflows/dual-target-nightly.yml"
if [ -f "${DUAL_WORKFLOW}" ]; then
  WORKFLOW_DETAIL=()
  grep -q "NIMBUS_TEST_TARGET" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("NIMBUS_TEST_TARGET missing")
  grep -q "nimbus" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("nimbus target missing")
  grep -Eq "convex_cloud|firebase_cloud|cloud_functions_cloud|mongodb_cloud|emulator|external|real" "${DUAL_WORKFLOW}" \
    || WORKFLOW_DETAIL+=("external/emulator/real target missing")
  if grep -Eq 'NIMBUS_DUAL_TARGET_DRY_RUN:[[:space:]]*"?1"?' "${DUAL_WORKFLOW}"; then
    WORKFLOW_DETAIL+=("workflow forces every target into dry-run")
  fi
  grep -q "requires_live: true" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("live target matrix flag missing")
  grep -q "secrets.CONVEX_CLOUD_DUAL_TARGET_URL" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("Convex cloud secret wiring missing")
  grep -q "secrets.FIREBASE_CLOUD_DUAL_TARGET_URL" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("Firebase cloud secret wiring missing")
  grep -q "secrets.CLOUD_FUNCTIONS_CLOUD_DUAL_TARGET_URL" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("Cloud Functions secret wiring missing")
  grep -q "secrets.MONGODB_CLOUD_DUAL_TARGET_URI" "${DUAL_WORKFLOW}" || WORKFLOW_DETAIL+=("MongoDB cloud secret wiring missing")
  if [ "${#WORKFLOW_DETAIL[@]}" -eq 0 ]; then
    pass ".github/workflows/dual-target-nightly.yml exists"
  else
    fail "dual-target-nightly.yml incomplete" "$(printf '%s; ' "${WORKFLOW_DETAIL[@]}")"
  fi
else
  fail "dual-target-nightly.yml missing" "MBA5 must add the weekly workflow"
fi

# 8. Auth caching ADR exists.
step 8 "Auth caching ADR exists"
AUTH_ADRS=(docs/private/decisions/[0-9][0-9][0-9]-auth-caching-policy.md)
AUTH_PROOF="${PROOF_DIR}/mba6-auth-caching-adr.md"
if [ -f "${AUTH_ADRS[0]}" ]; then
  AUTH_ADR="${AUTH_ADRS[0]}"
  if [ -f "${AUTH_PROOF}" ]; then
    pass "Auth caching ADR exists at ${AUTH_ADR}"
  else
    fail "Auth caching proof missing" "Expected ${AUTH_PROOF}"
  fi
else
  AUTH_ADR=""
  fail "Auth caching ADR missing" "Expected docs/private/decisions/NNN-auth-caching-policy.md"
fi

# 9. Auth cache references match ADR.
step 9 "Auth cache references are annotated or absent"
if [ -z "${AUTH_ADR}" ]; then
  fail "Cannot audit auth cache references" "Auth caching ADR is missing"
else
  ADR_ID="$(basename "${AUTH_ADR}" .md)"
  AUTH_PATHS=(
    "crates/nimbus-server/src/application_auth.rs"
    "crates/nimbus-server/src/adapters/convex/auth"
    "crates/nimbus-server/src/adapters/firebase"
    "crates/nimbus-server/src/adapters/cloud_functions"
    "crates/nimbus-server/src/adapters/mongodb/auth.rs"
    "crates/nimbus-server/src/tenant_isolation"
  )
  CACHE_HITS=""
  for path in "${AUTH_PATHS[@]}"; do
    if [ -e "${path}" ]; then
      CACHE_HITS="${CACHE_HITS}
$(grep -RniE 'cache|cached|caching|credential store|policy lookup' "${path}" 2>/dev/null || true)"
    fi
  done
  UNANNOTATED="$(printf '%s\n' "${CACHE_HITS}" | grep -v '^$' | grep -v "${ADR_ID}" || true)"
  if [ -z "${UNANNOTATED}" ]; then
    pass "Auth cache references are absent or annotated with ${ADR_ID}"
  else
    fail "Auth cache references lack ADR annotation" "$(printf '%s\n' "${UNANNOTATED}" | head -5 | tr '\n' '; ')"
  fi
fi

# 10. SQL-safety ADRs exist per SQL backend and helpers exist.
step 10 "SQL-safety ADRs exist per SQL backend"
SQL_DETAIL=()
SQL_PROOF="${PROOF_DIR}/mba7-sql-safety-adrs.md"
[ ! -f "${SQL_PROOF}" ] && SQL_DETAIL+=("${SQL_PROOF} missing")
for backend in sqlite postgres mysql libsql; do
  if ! find docs/private/decisions -maxdepth 1 -type f -name "*${backend}*sql*.md" 2>/dev/null | grep -q .; then
    SQL_DETAIL+=("${backend} ADR missing")
  fi
done
if ! grep -R "quote_identifier\|qualified_table\|sqlite_index_name\|data_table_name\|index_table_name" crates/nimbus-storage/src/sqlite crates/nimbus-storage/src/postgres crates/nimbus-storage/src/mysql crates/nimbus-storage/src/libsql >/dev/null 2>&1; then
  SQL_DETAIL+=("SQL identifier helpers missing")
fi
if [ "${#SQL_DETAIL[@]}" -eq 0 ]; then
  pass "SQL-safety ADRs and helper functions are present"
else
  fail "SQL-safety ADR coverage incomplete" "$(printf '%s; ' "${SQL_DETAIL[@]}")"
fi

# 11. Latency budget instrumentation and schema exist.
step 11 "Latency budgets are instrumented and documented"
LATENCY_DETAIL=()
LATENCY_PROOF="${PROOF_DIR}/mba8-latency-budgets.md"
if [ ! -f "docs/private/staging/operating/latency-budgets.md" ]; then
  LATENCY_DETAIL+=("docs/private/staging/operating/latency-budgets.md missing")
fi
if [ ! -f "${LATENCY_PROOF}" ]; then
  LATENCY_DETAIL+=("${LATENCY_PROOF} missing")
elif ! grep -q '^baseline_evidence:' "${LATENCY_PROOF}"; then
  LATENCY_DETAIL+=("baseline_evidence missing")
fi
SEGMENT_COUNT="$(grep -R "LatencySegment\|latency_segment\|segment_timer\|budgeted_segment" crates/nimbus-server/src crates/nimbus-engine/src 2>/dev/null | line_count)"
if [ "${SEGMENT_COUNT}" -lt 5 ]; then
  LATENCY_DETAIL+=("found ${SEGMENT_COUNT} segment markers, expected >= 5")
fi
if [ "${#LATENCY_DETAIL[@]}" -eq 0 ]; then
  pass "Latency budgets are documented and instrumented"
else
  fail "Latency budgets incomplete" "$(printf '%s; ' "${LATENCY_DETAIL[@]}")"
fi

# 12. Trait conventions doc and object-safety audit exist.
step 12 "Trait conventions and object-safety audit exist"
TRAIT_DETAIL=()
if [ ! -f "docs/private/architecture/trait-conventions.md" ]; then
  TRAIT_DETAIL+=("docs/private/architecture/trait-conventions.md missing")
fi
if [ ! -f "${PROOF_DIR}/mba9-trait-conventions.md" ]; then
  TRAIT_DETAIL+=("${PROOF_DIR}/mba9-trait-conventions.md missing")
fi
if [ "${#TRAIT_DETAIL[@]}" -eq 0 ]; then
  pass "Trait conventions doc and audit proof exist"
else
  fail "Trait conventions incomplete" "$(printf '%s; ' "${TRAIT_DETAIL[@]}")"
fi

# 13. Late storage/adapter contracts are implemented and proven.
step 13 "Storage identity, typed keys, consistency, and event capture are implemented"
PHYSICAL_PROOF="${PROOF_DIR}/mba10-table-identity-and-layout.md"
if [ ! -f "${PHYSICAL_PROOF}" ]; then
  fail "Table identity and layout proof missing" "Expected ${PHYSICAL_PROOF}"
else
  TABLE_ID_DETAIL=()
  if ! grep -Eq '^logical_identity: table_id_catalog$' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("logical_identity must be table_id_catalog")
  fi
  if ! grep -Eq 'redb: key_prefix_table_id' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("redb posture missing")
  fi
  if ! grep -Eq 'SQLite: shared_documents_by_table_id' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("SQLite posture missing")
  fi
  if ! grep -Eq 'Postgres: shared_documents_by_table_id' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("Postgres posture missing")
  fi
  if ! grep -Eq 'MySQL: shared_documents_by_table_id' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("MySQL posture missing")
  fi
  if ! grep -Eq 'libSQL: shared_documents_by_table_id' "${PHYSICAL_PROOF}"; then
    TABLE_ID_DETAIL+=("libSQL posture missing")
  fi
  if ! grep -R 'struct TableId\|type TableId' crates/nimbus-core/src crates/nimbus-storage/src >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("TableId type missing")
  fi
  if ! grep -R 'pub table_id: TableId' crates/nimbus-core/src/mutation.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("durable WriteOp does not carry table_id")
  fi
  if ! grep -R 'DURABLE_MUTATION_RECORD_VERSION: u16 = 2' crates/nimbus-core/src/mutation.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("durable mutation record version was not bumped for table_id")
  fi
  if ! grep -R 'table_identities: Vec<TableIdentitySnapshotEntry>' crates/nimbus-storage/src/store/journal_snapshot.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("materialized snapshots do not carry table identities")
  fi
  if ! grep -R 'ensure_table_id_in_write_txn' crates/nimbus-storage/src/store/journal.rs crates/nimbus-storage/src/store/journal_snapshot.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb replay/restore does not preserve journal table ids")
  fi
  if ! grep -R 'ensure_table_id_in_conn' crates/nimbus-storage/src/sqlite >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("SQLite replay/restore does not preserve journal table ids")
  fi
  if ! grep -R 'ensure_table_id_in_session' crates/nimbus-storage/src/postgres >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("Postgres replay does not preserve journal table ids")
  fi
  if ! grep -R 'ensure_table_id_from_session' crates/nimbus-storage/src/mysql >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("MySQL replay does not preserve journal table ids")
  fi
  if ! grep -R 'ensure_remote_table_id' crates/nimbus-storage/src/libsql >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("libSQL replay does not preserve journal table ids")
  fi
  if grep -R '^pub mod table_identity;' crates/nimbus-storage/src/lib.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("table_identity implementation module is still public")
  fi
  if grep -R 'TenantTableCatalog\|TableCatalogEntry\|TableCatalogKey' crates/nimbus-storage/src/lib.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("internal table catalog helpers are still re-exported")
  fi
  if ! grep -R 'TABLE_CATALOG' crates/nimbus-storage/src/store.rs crates/nimbus-storage/src/store/table_catalog.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb table catalog missing")
  fi
  if ! grep -R 'pub fn document_key(table_id: &TableId' crates/nimbus-storage/src/keys.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb document keys are not table_id-keyed")
  fi
  if grep -R 'pub fn document_key(table: &TableName' crates/nimbus-storage/src/keys.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb document key helper still accepts TableName")
  fi
  if ! grep -R 'table_id: &TableId' crates/nimbus-storage/src/index/keyspace.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb index keys are not table_id-keyed")
  fi
  if ! grep -R 'native_documents_and_indexes_are_physically_keyed_by_table_id' crates/nimbus-storage/src/tests >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("redb physical table_id regression test missing")
  fi
  if ! grep -R 'snapshot restore/rebuild must preserve stable table identities' crates/nimbus-storage/src/store/journal_snapshot/tests.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("snapshot table identity preservation test missing")
  fi
  if ! grep -R 'shadow_materializer_keys_documents_by_table_id_and_document_id' crates/nimbus-storage/src/materializer/mod.rs >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("shadow materializer table_id/document_id regression test missing")
  fi
  SQL_STORAGE_PATHS=(
    "crates/nimbus-storage/src/sqlite.rs"
    "crates/nimbus-storage/src/sqlite"
    "crates/nimbus-storage/src/postgres"
    "crates/nimbus-storage/src/mysql"
    "crates/nimbus-storage/src/libsql"
  )
  if ! grep -R 'PRIMARY KEY (table_id, id)' "${SQL_STORAGE_PATHS[@]}" >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("SQL document primary keys are not table_id-keyed")
  fi
  if grep -R 'PRIMARY KEY (table_name, id)' "${SQL_STORAGE_PATHS[@]}" >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("SQL document primary key still uses table_name")
  fi
  if grep -RE 'INSERT INTO documents \([^)]*table_name|WHERE table_name = [?$][0-9]?.*AND id|DELETE FROM documents WHERE table_name' "${SQL_STORAGE_PATHS[@]}" >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("SQL document read/write paths still key rows by table_name")
  fi
  if grep -R 'CASE WHEN table_name' crates/nimbus-storage/src/mysql >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("MySQL generated index columns still reference removed table_name column")
  fi
  if ! grep -R 'resolve_or_create_table_id' "${SQL_STORAGE_PATHS[@]}" >/dev/null 2>&1; then
    TABLE_ID_DETAIL+=("SQL write paths do not resolve/create table ids")
  fi
  TYPED_DOC="docs/private/architecture/storage/typed-key-columns.md"
  TYPED_PROOF="${PROOF_DIR}/mba11-typed-key-columns.md"
  CONSISTENCY_DOC="docs/private/architecture/storage/consistency-routing.md"
  CONSISTENCY_PROOF="${PROOF_DIR}/mba12-consistency-routing.md"
  EVENT_DOC="docs/private/architecture/adapters/event-capture.md"
  EVENT_PROOF="${PROOF_DIR}/mba13-event-capture.md"
  [ ! -f "${TYPED_DOC}" ] && TABLE_ID_DETAIL+=("${TYPED_DOC} missing")
  [ ! -f "${TYPED_PROOF}" ] && TABLE_ID_DETAIL+=("${TYPED_PROOF} missing")
  [ ! -f "${CONSISTENCY_DOC}" ] && TABLE_ID_DETAIL+=("${CONSISTENCY_DOC} missing")
  [ ! -f "${CONSISTENCY_PROOF}" ] && TABLE_ID_DETAIL+=("${CONSISTENCY_PROOF} missing")
  [ ! -f "${EVENT_DOC}" ] && TABLE_ID_DETAIL+=("${EVENT_DOC} missing")
  [ ! -f "${EVENT_PROOF}" ] && TABLE_ID_DETAIL+=("${EVENT_PROOF} missing")
  if [ -f "${TYPED_PROOF}" ] && ! grep -Eq '^current_ordering_coverage:[[:space:]]*string numeric$' "${TYPED_PROOF}"; then
    TABLE_ID_DETAIL+=("typed-key proof must name current string+numeric coverage")
  fi
  if [ -f "${TYPED_PROOF}" ] && ! grep -Eq '^binary_ordering:[[:space:]]*future_contract$' "${TYPED_PROOF}"; then
    TABLE_ID_DETAIL+=("typed-key proof must mark binary ordering as future_contract until FieldType::Binary exists")
  fi
  if [ -f "${CONSISTENCY_PROOF}" ] && ! grep -q "not_applicable" "${CONSISTENCY_PROOF}"; then
    TABLE_ID_DETAIL+=("consistency proof must mark unsupported backends not_applicable")
  fi
  if [ -f "${EVENT_PROOF}" ] && ! grep -q "storage_atomicity" "${EVENT_PROOF}"; then
    TABLE_ID_DETAIL+=("event proof must record storage_atomicity posture")
  fi
  if [ "${#TABLE_ID_DETAIL[@]}" -eq 0 ]; then
    pass "Late storage/adapter contracts are implemented and proven"
  else
    fail "Late storage/adapter contracts incomplete" "$(printf '%s; ' "${TABLE_ID_DETAIL[@]}")"
  fi
fi

# 14. Routing entries exist.
step 14 "Routing entries name this plan"
ROUTING_OK=1
if [ -z "${PLAN_FILE}" ]; then
  fail "Cannot check routing without plan file" "Plan is missing"
  ROUTING_OK=0
else
  PLAN_BASENAME="$(basename "${PLAN_FILE}")"
  if ! grep -q "${PLAN_BASENAME}" docs/private/plans/README.md; then
    fail "docs/private/plans/README.md routing missing" "Expected ${PLAN_BASENAME}"
    ROUTING_OK=0
  fi
  if ! grep -q "${PLAN_BASENAME}" "${AGENTS_MD}"; then
    fail "AGENTS.md routing missing" "Expected ${PLAN_BASENAME}"
    ROUTING_OK=0
  fi
fi
if [ "${ROUTING_OK}" -eq 1 ]; then
  pass "Routing entries exist in docs/private/plans/README.md and AGENTS.md"
fi

# 15. Ledger closed and main CI green evidence recorded.
step 15 "Ledger closed and latest main CI green evidence recorded"
if [ -z "${PLAN_FILE}" ]; then
  fail "Cannot check closeout without plan file" "Plan is missing"
else
  NOT_DONE="$(awk -F'|' '
    /^## Ledger/ { in_ledger = 1; next }
    /^## Completion Gate/ { in_ledger = 0 }
    in_ledger && /^\|[[:space:]]*MBA[0-9]+[[:space:]]*\|/ && $0 !~ /\|[[:space:]]*done[[:space:]]*\|/ {
      item = $2
      gsub(/[[:space:]]/, "", item)
      print item
    }
  ' "${PLAN_FILE}" | paste -sd ' ' -)"
  CLOSEOUT="${PROOF_DIR}/mba14-closeout.md"
  if [ -n "${NOT_DONE}" ]; then
    fail "Ledger rows are not all done" "Pending rows: ${NOT_DONE}"
  elif [ ! -f "${CLOSEOUT}" ]; then
    fail "Closeout proof missing" "Expected ${CLOSEOUT}"
  elif ! grep -q "status=completed" "${CLOSEOUT}" || ! grep -q "conclusion=success" "${CLOSEOUT}"; then
    fail "Closeout proof missing green CI evidence" "Expected status=completed and conclusion=success"
  else
    pass "Ledger is closed and green CI evidence is recorded"
  fi
fi

printf '\n\033[1mSummary:\033[0m %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf ' - %s\n' "${detail}"
  done
  exit 1
fi

exit 0
