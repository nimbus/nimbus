#!/usr/bin/env bash
# Static decision and expected-red contract for the NNC6.1 workload saga.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}" || exit 1

MODE="${1:-decision}"
CONTRACT="scripts/nimbus-network-control-plane/verification-contract.json"
PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1b-workload-saga-vocabulary-store-durable-home.md"
DURABLE_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1d-durable-workload-saga-store.md"
RECOVERY_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e-durable-discovery-recovery-decisions.md"
RECOVERY_STORE_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_STORE_SOURCE:-crates/nimbus-workloads/src/store.rs}"
RECOVERY_COMPUTE_ROOT_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_ROOT_SOURCE:-crates/nimbus-compute/src/workload_saga.rs}"
RECOVERY_COMPUTE_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_SOURCE:-crates/nimbus-compute/src/workload_saga/recovery.rs}"
RECOVERY_PROVISION_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_PROVISION_SOURCE:-crates/nimbus-compute/src/workload_saga/provision_decision.rs}"
RECOVERY_TEARDOWN_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_TEARDOWN_SOURCE:-crates/nimbus-workloads/src/saga/state/teardown.rs}"
RECOVERY_TENANT_ADAPTER_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_TENANT_ADAPTER_SOURCE:-crates/nimbus-server/src/workload_saga_store/tenant_enumeration.rs}"
RECOVERY_PROCESS_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_PROCESS_SOURCE:-crates/nimbus-server/src/workload_saga_store/tests/composition.rs}"
RECOVERY_MATRIX_SOURCE="${NIMBUS_NETWORK_VERIFY_RECOVERY_MATRIX_SOURCE:-crates/nimbus-server/src/workload_saga_store/tests/recovery.rs}"
ERRORS=()

add_error() {
  ERRORS+=("$1")
}

require_file() {
  if [ ! -f "$1" ]; then
    add_error "missing required file: $1"
  fi
}

require_contract_text() {
  if ! rg -q -F -- "$1" "${CONTRACT}"; then
    add_error "stable verification contract lacks frozen text: $1"
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
  require_file "${CONTRACT}"
  require_file "${PROOF}"

  if [ -f "${CONTRACT}" ]; then
    require_contract_text '_nimbus._workload_sagas'
    require_contract_text 'Engine::begin_mutation_execution_unit'
    require_contract_text 'nimbus-workloads -> nimbus-network'
    require_contract_text 'nimbus-compute -> nimbus-workloads'
    require_contract_text 'WorkloadSagaStore'
    require_contract_text 'WorkloadSagaId'
    require_contract_text 'WorkloadSagaRevision'
    require_contract_text 'WorkloadDesiredDigest'
    require_contract_text 'WorkloadExecutionId'
    require_contract_text 'successorIntent'
    require_contract_text 'complete semantic transition payload'
    require_contract_text 'canonical unsigned decimal'
    require_contract_text 'cleanup_pending'
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
    'pub struct EngineWorkloadSagaStore' \
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
    crates/nimbus-server/src/lib.rs \
    'pub use workload_saga_store::EngineWorkloadSagaStore;' \
    "public server-owned durable saga adapter"
  require_source_text \
    crates/nimbus-cli/src/compose/lifecycle.rs \
    'EngineWorkloadSagaStore::new(Arc::clone(&engine))' \
    "Compose durable saga-store adapter"
  require_source_text \
    crates/nimbus-cli/src/compose/provision.rs \
    '.into_foreground_runtime(saga_store)' \
    "Compose compute-owned foreground coordinator handoff"

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
    crates/nimbus-compute/src/workload_saga/provision_dispatch.rs \
    'resolve_ambiguous_confirmation' \
    "fresh-read provision ambiguity resolver"
  require_exact_count \
    "ambiguous provision fresh-read count" 1 \
    'self\.store\.load\(next\.key\(\)\)\.await\?' \
    crates/nimbus-compute/src/workload_saga/provision_dispatch.rs
  for ambiguity_outcome in \
    ConfirmedAfterAmbiguity UnresolvedAmbiguity Conflict; do
    require_source_text \
      crates/nimbus-compute/src/workload_saga/provision_dispatch.rs \
      "WorkloadSagaConfirmation::${ambiguity_outcome}" \
      "closed ambiguous provision outcome ${ambiguity_outcome}"
  done

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
  require_file "${RECOVERY_PROVISION_SOURCE}"
  require_file "${RECOVERY_TEARDOWN_SOURCE}"

  if ! rg -q 'pub struct WorkloadSagaTenantCursor' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'key: WorkloadSagaKey' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'pub struct WorkloadSagaTenantPageRequest' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'validate_for_tenant' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'pub struct WorkloadSagaTenantPage' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'crossed-tenant record' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q -F 'workload saga tenant page is duplicated, identity-unsorted, or cursor-regressing' \
      "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'claiming more records must fill its requested limit' "${RECOVERY_STORE_SOURCE}" ||
    ! rg -q 'fn list_for_tenant' "${RECOVERY_STORE_SOURCE}"; then
    add_error "missing tenant-scoped workload-saga paging"
  fi

  if ! rg -q -F 'pub enum WorkloadSagaAction' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'pub fn for_record' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'WorkloadSagaAction::Provision(decision)' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'WorkloadProvisionDecision::plan(record)?' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'WorkloadSagaAction::Teardown(decision)' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'let decision = record.decide_teardown()?;' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'teardown_target_phase(record, &decision)' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'PromoteSuccessor' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'WorkloadSagaAction::Quiescent' "${RECOVERY_COMPUTE_SOURCE}"; then
    add_error "missing pure compute workload-saga action selector"
  fi

  if rg -q 'WorkloadSagaAction::(WithdrawPublication|DrainWorkload|StopWorkload|DetachNetwork|ReleaseNetwork|RecordTerminalEvidence|InspectCleanup|AdvanceWithoutEffect)' \
    "${RECOVERY_COMPUTE_SOURCE}"; then
    add_error "compute recovery selector retains raw teardown action authority"
  fi

  for step in \
    ReserveNetwork PrepareWorkload AttachNetwork \
    InspectActivationPrerequisites ActivateWorkload \
    InspectWorkloadReadiness Publish ObservePublication; do
    if ! rg -q "WorkloadProvisionStep::${step}" "${RECOVERY_PROVISION_SOURCE}"; then
      add_error "compute workload-saga provision matrix omits ${step}"
    fi
  done

  for action in Provision Teardown PromoteSuccessor Quiescent; do
    if ! rg -q "WorkloadSagaAction::${action}" "${RECOVERY_COMPUTE_SOURCE}"; then
      add_error "compute workload-saga action matrix omits ${action}"
    fi
  done

  if ! rg -q -F 'pub fn decide_teardown' "${RECOVERY_TEARDOWN_SOURCE}" ||
    ! rg -q -F 'WorkloadTeardownDisposition::DefiniteFailure { claim, failure, .. }' \
      "${RECOVERY_TEARDOWN_SOURCE}" ||
    ! rg -q -F 'WorkloadTeardownDecision::CleanupPending {' "${RECOVERY_TEARDOWN_SOURCE}" ||
    ! rg -q -F 'claim: claim.clone()' "${RECOVERY_TEARDOWN_SOURCE}" ||
    ! rg -q -F 'failure: failure.clone()' "${RECOVERY_TEARDOWN_SOURCE}"; then
    add_error "portable teardown reducer does not retain exact cleanup evidence"
  fi
  for step in \
    WithdrawPublication DrainExecution StopExecution DetachNetwork ReleaseNetwork; do
    if ! rg -q "WorkloadTeardownStep::${step}" "${RECOVERY_TEARDOWN_SOURCE}"; then
      add_error "portable workload-saga teardown matrix omits ${step}"
    fi
  done

  if ! rg -q -F 'plan_recoverable_page' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'self.store.list_recoverable(request).await?' "${RECOVERY_COMPUTE_SOURCE}" ||
    ! rg -q -F 'WorkloadSagaDecision::for_record(record)?' "${RECOVERY_COMPUTE_SOURCE}"; then
    add_error "missing bounded compute recovery decision reader"
  fi

  if rg -q 'compare_and_swap|commit_loaded|TcpListener|UdpSocket|SandboxBackend|ServiceManager|LocalNetworkManager|std::time|rand::' \
      "${RECOVERY_COMPUTE_SOURCE}" "${RECOVERY_PROVISION_SOURCE}"; then
    add_error "pure compute recovery decision seam gained mutation, effect, or ambient-input authority"
  fi

  if ! rg -q -F 'request.validate_for_tenant' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'field: "tenantId"' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'field: "workloadId"' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'FilterOp::Gt' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'OrderDirection::Asc' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'saturating_add(1)' "${RECOVERY_TENANT_ADAPTER_SOURCE}" ||
    ! rg -q -F 'PrincipalContext::system' "${RECOVERY_TENANT_ADAPTER_SOURCE}"; then
    add_error "server tenant-scoped saga query is not exact, indexed, and limit-plus-one bounded"
  fi

  if ! rg -q 'fresh_process_reopens_engine_and_plans_every_workload_saga_phase_without_snapshot_handoff' \
      "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'SubprocessCrashCutHarness' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'run_crash_cut_child' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'run_crash_recovery_child' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'killed-at-boundary-and-reaped' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'assert_ne!' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'matrix-30-[0-9a-f]{64}' "${RECOVERY_PROCESS_SOURCE}" ||
    ! rg -q 'PROCESS_MATRIX_EXPECTATIONS' "${RECOVERY_MATRIX_SOURCE}" ||
    ! rg -q 'process-successor-from-observed' "${RECOVERY_MATRIX_SOURCE}" ||
    ! rg -q 'process-cleanup-network' "${RECOVERY_MATRIX_SOURCE}" ||
    ! rg -q 'process-recorded-quiescent' "${RECOVERY_MATRIX_SOURCE}"; then
    add_error "missing distinct-process all-phase recovery proof"
  fi

  if rg -q 'recordSnapshot|snapshotHandoff|serializedRecord|RECORD_SNAPSHOT|SNAPSHOT_HANDOFF_PAYLOAD|SERIALIZED_RECORD' \
      "${RECOVERY_PROCESS_SOURCE}"; then
    add_error "distinct-process recovery proof contains a record or snapshot handoff"
  fi

  if ! rg -q 'pub use recovery::' "${RECOVERY_COMPUTE_ROOT_SOURCE}"; then
    add_error "compute recovery decisions are not exported from their canonical owner module"
  fi

  if rg -q '^nimbus-testing[[:space:]]*=' crates/nimbus-network/Cargo.toml; then
    add_error "nimbus-network must not depend on nimbus-testing"
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
