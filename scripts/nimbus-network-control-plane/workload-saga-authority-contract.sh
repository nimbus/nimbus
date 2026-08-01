#!/usr/bin/env bash
# Static decision and expected-red contract for the NNC6.1 workload saga.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}" || exit 1

MODE="${1:-decision}"
PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"
PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.1b-workload-saga-vocabulary-store-durable-home.md"
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
    "desired store implementation count" 1 \
    'impl DesiredWorkloadStore for' crates
  require_exact_count \
    "service-manager constructor census" 54 \
    'ServiceManager::new\(' crates
  require_exact_count \
    "service-manager desired-write census" 3 \
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
  expected_workloads_dependents='["nimbus-bridge","nimbus-cli","nimbus-node","nimbus-server","nimbus-services","nimbus-system","nimbus-testing"]'
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
  expected_product_authority_paths="$(
    printf '%s\n' \
      'crates/nimbus-cli/src/workload_boot.rs' \
      'crates/nimbus-services/src/manager/types.rs'
  )"
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
  expected_desired_read_calls="$(
    printf '%s\n' \
      'crates/nimbus-cli/src/workload_boot.rs:controller.snapshot(' \
      'crates/nimbus-services/src/manager.rs:.snapshot_desired_workloads('
  )"
  if [ "${desired_read_status}" -gt 1 ]; then
    add_error "desired-workload read-surface scan failed with status ${desired_read_status}: ${desired_read_calls}"
  else
    desired_read_calls="$(printf '%s\n' "${desired_read_calls}" | LC_ALL=C sort)"
    if [ "${desired_read_calls}" != "${expected_desired_read_calls}" ]; then
      add_error "desired-workload production read calls changed: expected [${expected_desired_read_calls}], observed [${desired_read_calls}]"
    fi
  fi

  if ! rg -q -F 'desired_workloads: InMemoryDesiredWorkloadStore' \
    crates/nimbus-services/src/manager/types.rs; then
    add_error "current ServiceManagerState in-memory authority changed without NNC6.1c ledger update"
  fi
  if ! rg -q -F 'WorkloadController::new(InMemoryDesiredWorkloadStore::default())' \
    crates/nimbus-cli/src/workload_boot.rs; then
    add_error "current CLI in-memory planner changed without NNC6.1c ledger update"
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

case "${MODE}" in
  decision)
    verify_decision_contract
    ;;
  implementation)
    verify_target_implementation
    ;;
  *)
    printf 'usage: %s [decision|implementation]\n' "$0" >&2
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
    'Census: reverse-dependencies=7 store-implementations=1 product-in-memory-authorities=2 production-upserts=3 manager-constructors=54 recovery-readers=0'
fi
printf 'PASS workload-saga-authority %s\n' "${MODE}"
printf 'Summary: 1 passed, 0 failed\n'
