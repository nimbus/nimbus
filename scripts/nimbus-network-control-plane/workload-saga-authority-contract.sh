#!/usr/bin/env bash
# Static decision and expected-red contract for the NNC6.1 workload saga.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}" || exit 1

MODE="${1:-decision}"
PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"
PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1b-workload-saga-vocabulary-store-durable-home.md"
DURABLE_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1d-durable-workload-saga-store.md"
RECOVERY_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e-durable-discovery-recovery-decisions.md"
ERRORS=()

add_error() {
  ERRORS+=("$1")
}

require_file() {
  if [ ! -f "$1" ]; then
    add_error "missing required file: $1"
  fi
}

require_plan_text() {
  if ! rg -q -F -- "$1" "${PLAN}"; then
    add_error "plan lacks frozen contract text: $1"
  fi
}

require_source_text() {
  file="$1"
  text="$2"
  label="$3"
  if [ ! -f "${file}" ]; then
    add_error "${label}: missing ${file}"
  elif ! rg -q -F -- "${text}" "${file}"; then
    add_error "${label}: ${file} lacks [${text}]"
  fi
}

forbid_source_text() {
  file="$1"
  pattern="$2"
  label="$3"
  if [ -f "${file}" ] && rg -q -- "${pattern}" "${file}"; then
    add_error "${label}: forbidden source remains in ${file}"
  fi
}

require_exact_count() {
  label="$1"
  expected="$2"
  pattern="$3"
  shift 3
  observed="$(rg -n --glob '*.rs' -- "${pattern}" "$@" 2>/dev/null | wc -l | tr -d ' ')"
  if [ "${observed}" -ne "${expected}" ]; then
    add_error "${label}: expected ${expected}, observed ${observed}"
  fi
}

verify_decision_contract() {
  require_file "${PLAN}"
  require_file "${PROOF}"

  if [ -f "${PLAN}" ]; then
    require_plan_text '_nimbus._workload_sagas'
    require_plan_text 'Engine::begin_mutation_execution_unit'
    require_plan_text 'nimbus-workloads -> nimbus-network'
    require_plan_text 'nimbus-compute -> nimbus-workloads'
    require_plan_text 'WorkloadSagaStore'
    require_plan_text 'WorkloadSagaId'
    require_plan_text 'WorkloadSagaRevision'
    require_plan_text 'WorkloadDesiredDigest'
    require_plan_text 'WorkloadExecutionId'
    require_plan_text 'successorIntent'
    require_plan_text 'complete semantic transition payload'
    require_plan_text 'canonical unsigned decimal'
    require_plan_text 'cleanup_pending'
  fi

  require_exact_count \
    "desired store implementation count" 0 \
    'impl DesiredWorkloadStore for' crates
  require_exact_count \
    "service-manager constructor census" 52 \
    'ServiceManager::new\(' crates
  require_exact_count \
    "service-manager desired-write census" 0 \
    '\.upsert_desired_workload\(' crates/nimbus-services/src/manager

  workloads_dependents="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let input = "";
        process.stdin.on("data", chunk => input += chunk);
        process.stdin.on("end", () => {
          const metadata = JSON.parse(input);
          const dependents = metadata.packages
            .filter(item => item.dependencies.some(dependency => dependency.name === "nimbus-workloads"))
            .map(item => item.name)
            .sort();
          process.stdout.write(JSON.stringify(dependents));
        });
      '
  )"
  metadata_status=$?
  expected_workloads_dependents='["nimbus-bridge","nimbus-cli","nimbus-compute","nimbus-node","nimbus-server","nimbus-services","nimbus-system","nimbus-testing"]'
  if [ "${metadata_status}" -ne 0 ]; then
    add_error "cargo metadata could not resolve nimbus-workloads reverse dependencies"
  elif [ "${workloads_dependents}" != "${expected_workloads_dependents}" ]; then
    add_error "nimbus-workloads reverse dependencies: expected ${expected_workloads_dependents}, observed ${workloads_dependents}"
  fi

  product_authority_paths="$(
    rg -l 'InMemoryDesiredWorkloadStore' crates --glob '*.rs' |
      sort |
      awk '
        $0 != "crates/nimbus-workloads/src/desired.rs" &&
        $0 != "crates/nimbus-workloads/src/lib.rs" &&
        $0 !~ /\/tests\// &&
        $0 !~ /\/tests\.rs$/
      '
  )"
  expected_product_authority_paths=""
  if [ "${product_authority_paths}" != "${expected_product_authority_paths}" ]; then
    add_error "product in-memory authority paths changed: expected [${expected_product_authority_paths}], observed [${product_authority_paths}]"
  fi

  desired_read_calls="$(
    rg --with-filename --no-line-number -o \
      '(\.(desired_workload|snapshot_desired_workloads|restore_desired_workloads)\(|DesiredWorkloadStore>?::(desired_workload|snapshot_desired_workloads|restore_desired_workloads)\(|controller\.(store|store_mut|snapshot|restore)\(|WorkloadController::(store|store_mut|snapshot|restore)\()' \
      crates \
      --glob '*.rs' \
      --glob '!crates/nimbus-workloads/src/desired.rs' \
      --glob '!**/tests.rs' \
      --glob '!**/tests/**' 2>&1
  )"
  desired_read_status=$?
  expected_desired_read_calls=""
  if [ "${desired_read_status}" -gt 1 ]; then
    add_error "desired-workload read-surface scan failed with status ${desired_read_status}: ${desired_read_calls}"
  else
    desired_read_calls="$(printf '%s\n' "${desired_read_calls}" | LC_ALL=C sort)"
    if [ "${desired_read_calls}" != "${expected_desired_read_calls}" ]; then
      add_error "desired-workload production read calls changed: expected [${expected_desired_read_calls}], observed [${desired_read_calls}]"
    fi
  fi

  forbidden_network="$(rg -n \
    'WorkloadSaga(Store|Record|Phase|Revision|Transition)|_workload_sagas' \
    crates/nimbus-network/src 2>/dev/null || true)"
  if [ -n "${forbidden_network}" ]; then
    add_error "nimbus-network contains workload-saga authority: ${forbidden_network}"
  fi

  forbidden_system="$(rg -n \
    'WorkloadSaga(Store|Record|Phase|Revision|Transition)|_workload_sagas' \
    crates/nimbus-system/src 2>/dev/null || true)"
  if [ -n "${forbidden_system}" ]; then
    add_error "nimbus-system contains workload-saga authority: ${forbidden_system}"
  fi

  if rg -q '^nimbus-engine[[:space:]]*=' crates/nimbus-workloads/Cargo.toml; then
    add_error "nimbus-workloads must not depend on nimbus-engine"
  fi

  network_edges="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let input = "";
        process.stdin.on("data", chunk => input += chunk);
        process.stdin.on("end", () => {
          const metadata = JSON.parse(input);
          const package = metadata.packages.find(item => item.name === "nimbus-network");
          if (!package) process.exit(2);
          const workspace = new Set(metadata.packages.map(item => item.name));
          const edges = package.dependencies
            .filter(item => workspace.has(item.name))
            .map(item => item.name)
            .sort();
          process.stdout.write(JSON.stringify(edges));
        });
      '
  )"
  metadata_status=$?
  if [ "${metadata_status}" -ne 0 ]; then
    add_error "cargo metadata could not resolve the nimbus-network dependency contract"
  elif [ "${network_edges}" != '["nimbus-core"]' ]; then
    add_error "nimbus-network workspace edges must equal [\"nimbus-core\"], observed ${network_edges}"
  fi
}

verify_target_implementation() {
  [ -f crates/nimbus-workloads/src/saga.rs ] ||
    add_error "missing workloads-owned saga vocabulary"
  [ -f crates/nimbus-workloads/src/store.rs ] ||
    add_error "missing workloads-owned saga store port"
  rg -q '^nimbus-network[[:space:]]*=' crates/nimbus-workloads/Cargo.toml ||
    add_error "missing nimbus-workloads -> nimbus-network dependency"
  rg -q '^nimbus-workloads[[:space:]]*=' crates/nimbus-compute/Cargo.toml ||
    add_error "missing nimbus-compute -> nimbus-workloads dependency"
  if rg -q 'InMemoryDesiredWorkloadStore' \
    crates/nimbus-services/src/manager/types.rs crates/nimbus-cli/src/workload_boot.rs; then
    add_error "production in-memory desired-workload authority remains"
  fi
  if ! rg -q '_workload_sagas' crates/nimbus-server/src; then
    add_error "missing server-owned _workload_sagas adapter"
  elif ! rg -q 'begin_mutation_execution_unit' crates/nimbus-server/src; then
    add_error "server saga adapter does not use MutationExecutionUnit"
  fi
  if rg -q 'start_service_async' crates/nimbus-services/src/manager/registry.rs; then
    add_error "runtime lazy activation still bypasses the compute saga"
  fi
}

verify_operational_identity_cutover() {
  if rg -q 'TenantWorkloadGeneration' crates --glob '*.rs'; then
    add_error "legacy TenantWorkloadGeneration remains"
  fi
  if rg -q 'TenantWorkloadId' crates --glob '*.rs'; then
    add_error "node-owned TenantWorkloadId remains"
  fi
  if rg -q 'DesiredWorkloadStore' crates --glob '*.rs'; then
    add_error "DesiredWorkloadStore remains"
  fi
  if rg -q 'InMemoryDesiredWorkloadStore' crates --glob '*.rs'; then
    add_error "InMemoryDesiredWorkloadStore remains"
  fi
  if rg -q 'DesiredWorkloadSnapshot' crates --glob '*.rs'; then
    add_error "DesiredWorkloadSnapshot remains"
  fi
  if rg -q 'WorkloadController' crates --glob '*.rs'; then
    add_error "WorkloadController remains"
  fi

  if rg -q \
    'desired_workloads|desired_workload_snapshot|record_desired_service_workload|\.upsert_desired_workload\(' \
    crates/nimbus-services/src/manager.rs \
    crates/nimbus-services/src/manager; then
    add_error "ServiceManager desired-state field, snapshot, or write authority remains"
  fi

  if ! rg -q -F 'desired_workloads: Vec<DesiredWorkload>' \
    crates/nimbus-cli/src/workload_boot.rs ||
    ! rg -q -F 'pub(crate) fn desired_workloads(&self) -> &[DesiredWorkload]' \
      crates/nimbus-cli/src/workload_boot.rs; then
    add_error "CLI planner is not a pure ordered Vec<DesiredWorkload>"
  fi

  if ! rg -q -F 'execution_id: WorkloadExecutionId' \
    crates/nimbus-node/src/host_lifecycle.rs ||
    ! rg -q -F 'let execution_id = spec.execution_id()?' \
      crates/nimbus-node/src/host_lifecycle.rs ||
    ! rg -q -F 'let execution_id = status.execution_id();' \
      crates/nimbus-system/src/records/mod.rs ||
    ! rg -q -F '"executionId": execution_id.as_str()' \
      crates/nimbus-system/src/records/mod.rs; then
    add_error "host lifecycle and observed status do not carry WorkloadExecutionId"
  fi

  if ! rg -q -F 'generation: WorkloadGeneration' \
    crates/nimbus-workloads/src/tenant.rs ||
    ! rg -q -F '"observedGeneration": status.observed_generation().to_string()' \
      crates/nimbus-system/src/records/mod.rs ||
    ! rg -q -F 'string("observedGeneration", true)' \
      crates/nimbus-system/src/schema.rs; then
    add_error "tenant spec and observed status do not use lossless WorkloadGeneration"
  fi

  if rg -q \
    'sha2\.workspace|sanitize_unit_component|for_integration_test|NIMBUS_WORKLOAD_ID|nimbus-tw_' \
    crates/nimbus-node/Cargo.toml crates/nimbus-node/src crates/nimbus-node/tests \
    --glob '*.rs' --glob 'Cargo.toml'; then
    add_error "legacy node identity derivation, selector, or unit convention remains"
  fi

  assigned_node_fences="$({
    rg -n -F 'ensure_assigned_node_matches(&self.node_id' \
      crates/nimbus-node/src/reconciler.rs 2>/dev/null || true
  } | wc -l | tr -d ' ')"
  if [ "${assigned_node_fences}" -ne 2 ]; then
    add_error "node reconcile and inspect need exactly two pre-effect assigned-node fences, observed ${assigned_node_fences}"
  fi

  product_saga_store_implementations="$({
    while IFS= read -r source; do
      sed '/^#\[cfg(test)\]/,$d' "${source}" |
        rg -n 'impl WorkloadSagaStore for' || true
    done < <(
      rg -l 'impl WorkloadSagaStore for' crates \
        --glob '*.rs' --glob '!**/tests.rs' --glob '!**/tests/**' 2>/dev/null || true
    )
  } | wc -l | tr -d ' ')"
  if [ "${product_saga_store_implementations}" -ne 1 ]; then
    add_error "production saga-store implementation count: expected 1, observed ${product_saga_store_implementations}"
  fi

  product_saga_coordinator_constructions="$({
    rg -n 'WorkloadSagaCoordinator::new\(' crates \
      --glob '*.rs' --glob '!**/tests.rs' --glob '!**/tests/**' 2>/dev/null || true
  } | wc -l | tr -d ' ')"
  if [ "${product_saga_coordinator_constructions}" -ne 1 ]; then
    add_error "production saga coordinator construction count: expected 1, observed ${product_saga_coordinator_constructions}"
  fi
}

verify_durable_store_contract() {
  require_file "${DURABLE_PROOF}"

  require_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'pub(crate) struct EngineWorkloadSagaStore' \
    "server-owned durable saga adapter"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'impl WorkloadSagaStore for EngineWorkloadSagaStore' \
    "server-owned store-port implementation"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'begin_mutation_execution_unit' \
    "Engine execution-unit mutation path"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'AtomicWrite::Set' \
    "whole-record CAS write"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'commit()' \
    "single Engine commit point"

  require_source_text \
    crates/nimbus-server/src/workload_saga_store/schema.rs \
    '_workload_sagas' \
    "reserved workload-saga table"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/schema.rs \
    'by_recovery' \
    "immutable-identity recovery index"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/schema.rs \
    '"recoveryEligible", "sagaId"' \
    "eligible immutable saga cursor index"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/schema.rs \
    'reconcile_index_metadata' \
    "no-churn logical schema comparison"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/schema.rs \
    'prepare_exact_schema' \
    "exact schema bootstrap"
  forbid_source_text \
    crates/nimbus-system/src/schema.rs \
    '_workload_sagas' \
    "workload-saga table must remain outside SystemTable"

  require_source_text \
    crates/nimbus-server/src/workload_saga_store/codec.rs \
    'encode_workload_saga_record' \
    "strict physical encoder"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/codec.rs \
    'decode_workload_saga_record' \
    "strict physical decoder"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/codec.rs \
    'validate_physical_shape' \
    "closed physical codec"
  require_source_text \
    crates/nimbus-workloads/src/saga/state.rs \
    '#[serde(deny_unknown_fields, rename_all = "camelCase")]' \
    "closed portable saga codec"

  require_source_text \
    crates/nimbus-workloads/src/saga.rs \
    'pub const WORKLOAD_SAGA_RECOVERY_ORDER' \
    "workloads-owned recovery order"
  require_source_text \
    crates/nimbus-workloads/src/saga.rs \
    'pub const fn recovery_order' \
    "workloads-owned recovery rank"
  require_source_text \
    crates/nimbus-workloads/src/store.rs \
    'Self::new(record.saga_id().clone())' \
    "immutable recovery cursor identity"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/recovery.rs \
    'value: Value::String(cursor.saga_id().as_str().to_owned())' \
    "server immutable recovery cursor fence"
  require_source_text \
    crates/nimbus-server/src/workload_saga_store/recovery.rs \
    'requires_recovery' \
    "portable recovery eligibility"

  require_source_text \
    crates/nimbus-compute/src/state.rs \
    'pub enum ComputeWorkloadComposition' \
    "explicit compute composition profile"
  require_source_text \
    crates/nimbus-compute/src/state.rs \
    'ProtocolOnly' \
    "protocol-only compute profile"
  require_source_text \
    crates/nimbus-compute/src/state.rs \
    'Managed {' \
    "managed compute profile"
  require_source_text \
    crates/nimbus-compute/src/state.rs \
    'workload_saga_coordinator: Option<Arc<WorkloadSagaCoordinator>>' \
    "retained sole workload-saga coordinator"
  require_source_text \
    crates/nimbus-compute/src/workload_saga.rs \
    'resolve_ambiguous_commit' \
    "fresh-read ambiguity resolver"

  require_exact_count \
    "product saga-store implementation count" 1 \
    'impl WorkloadSagaStore for EngineWorkloadSagaStore' \
    crates/nimbus-server/src
  product_saga_coordinator_constructions="$({
    for source in \
      crates/nimbus-compute/src/state.rs \
      crates/nimbus-compute/src/workload_saga.rs \
      crates/nimbus-server/src/state.rs \
      crates/nimbus-server/src/router.rs \
      crates/nimbus-server/src/workload_saga_store.rs; do
      if [ -f "${source}" ]; then
        sed '/^#\[cfg(test)\]/,$d' "${source}"
      fi
    done
  } | rg -c 'WorkloadSagaCoordinator::new\(' || true)"
  product_saga_coordinator_constructions="${product_saga_coordinator_constructions:-0}"
  if [ "${product_saga_coordinator_constructions}" -ne 1 ]; then
    add_error "production saga coordinator construction count: expected 1, observed ${product_saga_coordinator_constructions}"
  fi

  require_source_text \
    crates/nimbus-core/src/types.rs \
    'pub fn is_nimbus_reserved(&self) -> bool' \
    "canonical reserved-tenant predicate"
  for reserved_consumer in \
    crates/nimbus-system/src/identity.rs \
    crates/nimbus-cloud-functions/src/http/tenant_binding.rs \
    crates/nimbus-convex/src/silo_auth.rs \
    crates/nimbus-convex/src/tenancy.rs \
    crates/nimbus-dynamodb/src/tenant.rs \
    crates/nimbus-firebase/src/project_tenant_registry.rs \
    crates/nimbus-kv/src/server.rs \
    crates/nimbus-mongodb/src/credential_registry.rs \
    crates/nimbus-mongodb/src/commands/tenant.rs \
    crates/nimbus-s3/src/auth.rs; do
    require_source_text \
      "${reserved_consumer}" \
      '.is_nimbus_reserved()' \
      "canonical reserved-tenant consumer"
  done

  forbid_source_text \
    crates/nimbus-server/src/workload_saga_store.rs \
    'nimbus_storage|TcpListener|UdpSocket|nimbus_(network|sandbox|services|proxy|egress)' \
    "durable saga adapter effect boundary"
  forbid_source_text \
    crates/nimbus-server/src/workload_saga_store/recovery.rs \
    'nimbus_storage|TcpListener|UdpSocket|nimbus_(network|sandbox|services|proxy|egress)' \
    "durable recovery adapter effect boundary"

  if rg -q 'InMemoryWorkloadSagaStore' crates \
    --glob '*.rs' --glob '!**/tests.rs' --glob '!**/tests/**'; then
    add_error "production in-memory workload-saga store is forbidden"
  fi
}

verify_recovery_decision_contract() {
  require_file "${RECOVERY_PROOF}"

  if ! rg -q 'pub struct WorkloadSagaTenantCursor' \
      crates/nimbus-workloads/src/store.rs ||
    ! rg -q 'pub struct WorkloadSagaTenantPageRequest' \
      crates/nimbus-workloads/src/store.rs ||
    ! rg -q 'pub struct WorkloadSagaTenantPage' \
      crates/nimbus-workloads/src/store.rs ||
    ! rg -q 'fn list_for_tenant' crates/nimbus-workloads/src/store.rs; then
    add_error "missing tenant-scoped workload-saga paging"
  fi

  if ! rg -q 'pub enum WorkloadSagaAction' \
    crates/nimbus-compute/src/workload_saga.rs \
    crates/nimbus-compute/src/workload_saga 2>/dev/null; then
    add_error "missing pure compute workload-saga action selector"
  fi

  if ! rg -q 'plan_recoverable_page' \
    crates/nimbus-compute/src/workload_saga.rs \
    crates/nimbus-compute/src/workload_saga 2>/dev/null; then
    add_error "missing bounded compute recovery decision reader"
  fi

  if ! rg -q 'fresh_process_reopens_engine_and_plans_every_workload_saga_phase_without_snapshot_handoff' \
    crates/nimbus-server/src/workload_saga_store/tests 2>/dev/null; then
    add_error "missing distinct-process all-phase recovery proof"
  fi
}

case "${MODE}" in
  decision)
    verify_decision_contract
    ;;
  implementation)
    verify_target_implementation
    ;;
  cutover)
    verify_operational_identity_cutover
    ;;
  durable-store)
    verify_durable_store_contract
    ;;
  recovery-decisions)
    verify_recovery_decision_contract
    ;;
  *)
    printf 'usage: %s [decision|implementation|cutover|durable-store|recovery-decisions]\n' "$0" >&2
    exit 2
    ;;
esac

if [ "${#ERRORS[@]}" -ne 0 ]; then
  for error in "${ERRORS[@]}"; do
    printf 'FAIL workload-saga-authority %s\n' "${error}"
  done
  printf 'Summary: 0 passed, %d failed\n' "${#ERRORS[@]}"
  exit 1
fi

if [ "${MODE}" = "decision" ]; then
  printf '%s\n' \
    'Census: reverse-dependencies=8 store-implementations=0 product-in-memory-authorities=0 production-upserts=0 manager-constructors=52 recovery-readers=0'
fi
printf 'PASS workload-saga-authority %s\n' "${MODE}"
printf 'Summary: 1 passed, 0 failed\n'
