#!/usr/bin/env bash
# Static NNC6.4 contract for confirmed workload-provision dispatch and caller cutover.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_PATH="${SCRIPT_DIR}/workload-provision-dispatch-contract.sh"
SELF_TEST_SCRIPT_PATH="${SCRIPT_DIR}/workload-provision-dispatch-self-test.sh"
REPO_ROOT="${NIMBUS_NETWORK_NNC64_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
NNC63B_COMPLETION_CHECKPOINT="${NIMBUS_NETWORK_NNC64_NNC63B_CHECKPOINT:-c42c61fb2d97d037069f3b27b9055d6e58f11d1d}"

WORKLOAD_PROVISION="crates/nimbus-workloads/src/saga/provision.rs"
WORKLOAD_PROVISION_DISPATCH="crates/nimbus-workloads/src/saga/provision/dispatch.rs"
WORKLOAD_PROVISION_DISPATCH_TESTS="crates/nimbus-workloads/src/saga/provision/tests/dispatch.rs"
WORKLOAD_STATE="crates/nimbus-workloads/src/saga/state.rs"
WORKLOAD_STATE_PROVISION="crates/nimbus-workloads/src/saga/state/provision.rs"
WORKLOAD_STATE_PROVISION_TESTS="crates/nimbus-workloads/src/saga/tests/provision_state.rs"
WORKLOAD_STORE="crates/nimbus-workloads/src/store.rs"
COMPUTE_SAGA="crates/nimbus-compute/src/workload_saga.rs"
COMPUTE_DECISION="crates/nimbus-compute/src/workload_saga/provision_decision.rs"
COMPUTE_DISPATCH="crates/nimbus-compute/src/workload_saga/provision_dispatch.rs"
COMPUTE_DISPATCH_TESTS="crates/nimbus-compute/src/workload_saga/provision_dispatch/tests.rs"
COMPUTE_DISPATCHER="crates/nimbus-compute/src/workload_saga/provision_dispatcher.rs"
COMPUTE_DISPATCHER_TESTS="crates/nimbus-compute/src/workload_saga/provision_dispatcher/tests.rs"
COMPUTE_DRIVER="crates/nimbus-compute/src/workload_saga/provision_driver.rs"
COMPUTE_DRIVER_TESTS="crates/nimbus-compute/src/workload_saga/provision_driver/tests.rs"
COMPUTE_PROVISION_PROVIDER="crates/nimbus-compute/src/workload_saga/provision_provider.rs"
COMPUTE_SANDBOX_PROVIDER="crates/nimbus-compute/src/workload_saga/provision_sandbox.rs"
COMPUTE_STATE="crates/nimbus-compute/src/state.rs"
COMPUTE_RESOURCE_PROVISION="crates/nimbus-compute/src/resource_provision.rs"
COMPUTE_RESOURCE_PROVISION_TESTS="crates/nimbus-compute/src/resource_provision/tests.rs"
COMPUTE_SANDBOXES="crates/nimbus-compute/src/sandboxes.rs"
COMPUTE_SERVICES="crates/nimbus-compute/src/services.rs"
SERVER_STATE="crates/nimbus-server/src/state.rs"
SERVER_INGRESS="crates/nimbus-server/src/workload_ingress.rs"
SERVER_CONSTRUCTION="crates/nimbus-server/src/construction.rs"
SERVER_ROUTER="crates/nimbus-server/src/router.rs"
SERVER_WORKLOAD_COMPOSITION="crates/nimbus-server/src/workload_composition.rs"
SERVER_PROVISION_PROCESS_TESTS="crates/nimbus-server/src/workload_saga_store/tests/provision_driver_process.rs"
SERVER_CONVEX_ASYNC="crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs"
SERVER_CONVEX_CONTEXT_READ_ONLY_TESTS="crates/nimbus-server/src/adapters/convex/execution/runtime_backed/invoke/context/read_only_tests.rs"
SERVER_CONVEX_LOOKUP_READ_ONLY_TESTS="crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls/read_only_tests.rs"
SANDBOX_BACKEND="crates/nimbus-sandbox/src/backend.rs"
SANDBOX_PROVISION="crates/nimbus-sandbox/src/provision.rs"
SANDBOX_CONTAINER_PROVIDER="crates/nimbus-sandbox/src/backends/container/runtime.rs"
SANDBOX_CONTAINER_PROVIDER_JOURNAL="crates/nimbus-sandbox/src/backends/container/runtime/provision.rs"
SANDBOX_KRUN_PROVIDER="crates/nimbus-sandbox/src/backends/krun/vm.rs"
SERVICES_REGISTRY="crates/nimbus-services/src/registry.rs"
SERVICES_MANAGER="crates/nimbus-services/src/manager.rs"
SERVICES_ACTIVATION="crates/nimbus-services/src/manager/activation.rs"
SERVICES_START="crates/nimbus-services/src/manager/service_start.rs"
SERVICES_SANDBOXES="crates/nimbus-services/src/manager/sandboxes.rs"
COMPOSE_LIFECYCLE="crates/nimbus-cli/src/compose/lifecycle.rs"
COMPOSE_EXECUTION="crates/nimbus-cli/src/compose/execution.rs"
COMPOSE_LIFECYCLE_TESTS="crates/nimbus-cli/src/compose/tests/lifecycle.rs"
COMPOSE_FORWARDED_TESTS="crates/nimbus-cli/src/compose/tests/forwarded_api.rs"
NIMBUS_MACHINE_API="crates/nimbus-machine/src/api.rs"
MACHINE_ROUTES="crates/nimbus-cli/src/machine/api/routes.rs"
MACHINE_SERVICE="crates/nimbus-cli/src/machine/api/service_workloads.rs"
MACHINE_CLIENT="crates/nimbus-cli/src/machine/client.rs"
MACHINE_STUB_CLIENT="crates/nimbus-cli/src/machine/stub/client.rs"
MACHINE_STUB_BACKEND="crates/nimbus-cli/src/machine/stub/backend.rs"
MACHINE_BACKEND="crates/nimbus-cli/src/machine/backend.rs"
MACHINE_BACKEND_PROVISION="crates/nimbus-cli/src/machine/backend/provision.rs"
MACHINE_PUBLICATION="crates/nimbus-cli/src/machine/publication_authority.rs"
MACHINE_CAPABILITIES="crates/nimbus-cli/src/machine/api/capabilities.rs"
MACHINE_PROVISION_ROUTE_TESTS="crates/nimbus-cli/src/machine/api/tests/provision_phase.rs"
MACHINE_PROVISION_ADAPTER_TESTS="crates/nimbus-cli/src/machine/backend/provision/tests.rs"
NODE_RECONCILER="crates/nimbus-node/src/reconciler.rs"
NODE_HOST_LIFECYCLE="crates/nimbus-node/src/host_lifecycle.rs"
NODE_DIRECT_PROCESS="crates/nimbus-node/src/direct_process.rs"
NODE_SYSTEMD_TRANSIENT="crates/nimbus-node/src/systemd_transient.rs"
NODE_EXECUTOR="crates/nimbus-cli/src/node_workload_executor.rs"
CLI_LIB="crates/nimbus-cli/src/lib.rs"
CLI_START_BOOT="crates/nimbus-cli/src/start/boot.rs"
CLI_DEV_WIRE="crates/nimbus-cli/src/dev/wire.rs"
MACHINE_LOCAL_SERVER="crates/nimbus-cli/src/machine/local_server.rs"
CLOUD_FUNCTIONS_HOST="crates/nimbus-cloud-functions/src/host_bridge.rs"
CLOUD_FUNCTIONS_HTTP="crates/nimbus-cloud-functions/src/http/invocation.rs"
CLOUD_FUNCTIONS_TRIGGER="crates/nimbus-cloud-functions/src/trigger_executor.rs"
CLOUD_FUNCTIONS_READ_ONLY_TESTS="crates/nimbus-cloud-functions/tests/read_only_snapshots.rs"
NETWORK_MANIFEST="crates/nimbus-network/Cargo.toml"
NETWORK_SOURCE="crates/nimbus-network/src/lib.rs"
OWNER_PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"
OWNER_PROOF="docs/private/plans/proof/nimbus-network-control-plane/nnc6.4-atomic-provision-caller-cutover.md"

NNC64_ERRORS=()
NNC64_CHECKS=0

add_error() {
  NNC64_ERRORS+=("$1")
}

pass_check() {
  NNC64_CHECKS=$((NNC64_CHECKS + 1))
}

source_without_comments() {
  node - "$1" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const source = fs.existsSync(path) ? fs.readFileSync(path, "utf8") : "";
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
}

raw_source() {
  if [ -f "$1" ]; then
    command cat -- "$1"
  fi
}

source_without_comments_or_strings() {
  # The JavaScript program is intentionally single-quoted so the shell cannot
  # interpolate source-code tokens while Node reads the audited Rust on stdin.
  # shellcheck disable=SC2016
  node -e '
let source = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { source += chunk; });
process.stdin.on("end", () => {
  let output = "";
  let index = 0;
  const blank = character => character === "\n" ? "\n" : " ";
  while (index < source.length) {
    if (source.startsWith("//", index)) {
      while (index < source.length && source[index] !== "\n") {
        output += " ";
        index += 1;
      }
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 0;
      do {
        if (source.startsWith("/*", index)) {
          depth += 1;
          output += "  ";
          index += 2;
        } else if (source.startsWith("*/", index)) {
          depth -= 1;
          output += "  ";
          index += 2;
        } else {
          output += blank(source[index]);
          index += 1;
        }
      } while (index < source.length && depth > 0);
      continue;
    }
    const raw = source.slice(index).match(/^(?:b?r)(#+)?"/);
    if (raw) {
      const hashes = raw[1] || "";
      const end = `"${hashes}`;
      output += " ".repeat(raw[0].length);
      index += raw[0].length;
      while (index < source.length && !source.startsWith(end, index)) {
        output += blank(source[index]);
        index += 1;
      }
      output += " ".repeat(end.length);
      index += end.length;
      continue;
    }
    const prefix = source.startsWith("b\"", index) ? 2 : source[index] === "\"" ? 1 : 0;
    if (prefix > 0) {
      output += " ".repeat(prefix);
      index += prefix;
      while (index < source.length) {
        const character = source[index];
        output += blank(character);
        index += 1;
        if (character === "\\" && index < source.length) {
          output += blank(source[index]);
          index += 1;
        } else if (character === "\"") {
          break;
        }
      }
      continue;
    }
    output += source[index];
    index += 1;
  }
  process.stdout.write(output);
});
'
}

extract_rust_item() {
  marker="$1"
  node -e '
    let source = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", chunk => { source += chunk; });
    process.stdin.on("end", () => {
      const marker = process.argv[1];
      const start = source.indexOf(marker);
      const open = source.indexOf("{", start);
      if (start < 0 || open < 0) return;
      let depth = 0;
      for (let index = open; index < source.length; index += 1) {
        if (source[index] === "{") depth += 1;
        if (source[index] === "}") depth -= 1;
        if (depth === 0) {
          process.stdout.write(source.slice(start, index + 1));
          return;
        }
      }
    });
  ' "${marker}"
}

check_literals() {
  label="$1"
  source="$2"
  shift 2
  starting_errors="${#NNC64_ERRORS[@]}"
  for literal in "$@"; do
    if ! printf '%s\n' "${source}" | rg -q -F "${literal}"; then
      add_error "${label}: missing ${literal}"
    fi
  done
  if [ "${#NNC64_ERRORS[@]}" -eq "${starting_errors}" ]; then
    pass_check
  fi
}

check_exact_variants() {
  label="$1"
  source="$2"
  marker="$3"
  expected="$4"
  block="$(printf '%s' "${source}" | extract_rust_item "${marker}")"
  variants="$(printf '%s\n' "${block}" |
    sed -nE 's/^[[:space:]]{4}([A-Z][A-Za-z0-9_]*)[[:space:]]*([{(,]).*/\1/p' |
    sort | tr '\n' ' ')"
  if [ "${variants}" != "${expected}" ]; then
    add_error "${label}: expected exactly ${expected}(observed ${variants:-none})"
  else
    pass_check
  fi
}

replace_once() {
  variable="$1"
  old="$2"
  new="$3"
  value="${!variable}"
  if [[ "${value}" != *"${old}"* ]]; then
    add_error "mutation ${NIMBUS_NETWORK_NNC64_TEST_MUTATION:-unknown} did not change its fixture"
    return
  fi
  printf -v "${variable}" '%s' "${value/"${old}"/"${new}"}"
}

replace_nth() {
  variable="$1"
  old="$2"
  new="$3"
  occurrence="$4"
  value="${!variable}"
  remaining="${value}"
  rebuilt=""
  for ((index = 1; index <= occurrence; index += 1)); do
    if [[ "${remaining}" != *"${old}"* ]]; then
      add_error "mutation ${NIMBUS_NETWORK_NNC64_TEST_MUTATION:-unknown} did not find occurrence ${occurrence}"
      return
    fi
    prefix="${remaining%%"${old}"*}"
    remaining="${remaining#*"${old}"}"
    rebuilt+="${prefix}"
    if [ "${index}" -eq "${occurrence}" ]; then
      rebuilt+="${new}${remaining}"
      printf -v "${variable}" '%s' "${rebuilt}"
      return
    fi
    rebuilt+="${old}"
  done
}

append_source() {
  variable="$1"
  addition="$2"
  value="${!variable}"
  printf -v "${variable}" '%s\n%s\n' "${value}" "${addition}"
}

# Several inputs are consumed through the required-input indirection and
# mutation harness rather than a lexical reference.
# shellcheck disable=SC2034
load_sources() {
  workload_provision_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_PROVISION}")
$(source_without_comments "${REPO_ROOT}/${WORKLOAD_PROVISION_DISPATCH}")"
  workload_provision_tests_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_PROVISION_DISPATCH_TESTS}")"
  workload_state_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STATE}")
$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STATE_PROVISION}")"
  workload_state_tests_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STATE_PROVISION_TESTS}")"
  workload_store_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STORE}")"
  compute_saga_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_SAGA}")"
  compute_decision_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DECISION}")"
  compute_dispatch_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCH}")
$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCHER}")
$(source_without_comments "${REPO_ROOT}/${COMPUTE_DRIVER}")"
  compute_dispatch_tests_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCH_TESTS}")
$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCHER_TESTS}")
$(source_without_comments "${REPO_ROOT}/${COMPUTE_DRIVER_TESTS}")
$(source_without_comments "${REPO_ROOT}/${SERVER_PROVISION_PROCESS_TESTS}")"
  caller_proof_source="${compute_dispatch_tests_source}
$(source_without_comments "${REPO_ROOT}/${SERVER_CONVEX_CONTEXT_READ_ONLY_TESTS}")
$(source_without_comments "${REPO_ROOT}/${SERVER_CONVEX_LOOKUP_READ_ONLY_TESTS}")
$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_READ_ONLY_TESTS}")"
  compute_phase_provider_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_PROVISION_PROVIDER}")"
  compute_provider_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_SANDBOX_PROVIDER}")"
  compute_state_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_STATE}")"
  compute_resource_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_RESOURCE_PROVISION}")"
  compute_sandboxes_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_SANDBOXES}")"
  compute_services_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_SERVICES}")"
  native_caller_tests_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_RESOURCE_PROVISION_TESTS}")"
  server_state_source="$(source_without_comments "${REPO_ROOT}/${SERVER_STATE}")
$(source_without_comments "${REPO_ROOT}/${SERVER_INGRESS}")"
  server_composition_source="$(source_without_comments "${REPO_ROOT}/${SERVER_CONSTRUCTION}")
$(source_without_comments "${REPO_ROOT}/${SERVER_ROUTER}")
$(source_without_comments "${REPO_ROOT}/${SERVER_WORKLOAD_COMPOSITION}")"
  convex_async_source="$(source_without_comments "${REPO_ROOT}/${SERVER_CONVEX_ASYNC}")"
  convex_lookup_tests_source="$(source_without_comments "${REPO_ROOT}/${SERVER_CONVEX_LOOKUP_READ_ONLY_TESTS}")"
  sandbox_backend_source="$(source_without_comments "${REPO_ROOT}/${SANDBOX_BACKEND}")"
  sandbox_provision_source="$(source_without_comments "${REPO_ROOT}/${SANDBOX_PROVISION}")"
  sandbox_container_provider_source="$(source_without_comments "${REPO_ROOT}/${SANDBOX_CONTAINER_PROVIDER}")
$(source_without_comments "${REPO_ROOT}/${SANDBOX_CONTAINER_PROVIDER_JOURNAL}")"
  sandbox_krun_provider_source="$(source_without_comments "${REPO_ROOT}/${SANDBOX_KRUN_PROVIDER}")"
  services_registry_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_REGISTRY}")"
  services_manager_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_MANAGER}")
$(source_without_comments "${REPO_ROOT}/${SERVICES_ACTIVATION}")"
  services_start_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_START}")"
  services_sandboxes_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_SANDBOXES}")"
  compose_lifecycle_source="$(source_without_comments "${REPO_ROOT}/${COMPOSE_LIFECYCLE}")
$(source_without_comments "${REPO_ROOT}/${COMPOSE_EXECUTION}")"
  compose_caller_tests_source="$(source_without_comments "${REPO_ROOT}/${COMPOSE_LIFECYCLE_TESTS}")
$(source_without_comments "${REPO_ROOT}/${COMPOSE_FORWARDED_TESTS}")"
  nimbus_machine_api_source="$(source_without_comments "${REPO_ROOT}/${NIMBUS_MACHINE_API}")"
  machine_routes_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_ROUTES}")"
  machine_service_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_SERVICE}")"
  machine_parent_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_CLIENT}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_STUB_CLIENT}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_STUB_BACKEND}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_BACKEND}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_BACKEND_PROVISION}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_PUBLICATION}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_CAPABILITIES}")"
  machine_guest_caller_tests_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_PROVISION_ROUTE_TESTS}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_PROVISION_ADAPTER_TESTS}")"
  node_reconciler_source="$(source_without_comments "${REPO_ROOT}/${NODE_RECONCILER}")"
  node_host_lifecycle_source="$(source_without_comments "${REPO_ROOT}/${NODE_HOST_LIFECYCLE}")"
  node_provider_source="$(source_without_comments "${REPO_ROOT}/${NODE_DIRECT_PROCESS}")
$(source_without_comments "${REPO_ROOT}/${NODE_SYSTEMD_TRANSIENT}")"
  node_executor_source="$(source_without_comments "${REPO_ROOT}/${NODE_EXECUTOR}")
$(source_without_comments "${REPO_ROOT}/${CLI_LIB}")"
  process_composition_caller_source="$(source_without_comments "${REPO_ROOT}/${CLI_START_BOOT}")
$(source_without_comments "${REPO_ROOT}/${CLI_DEV_WIRE}")
$(source_without_comments "${REPO_ROOT}/${MACHINE_LOCAL_SERVER}")"
  cloud_functions_host_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_HOST}")"
  cloud_functions_http_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_HTTP}")"
  cloud_functions_trigger_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_TRIGGER}")"
  network_manifest_source="$(source_without_comments "${REPO_ROOT}/${NETWORK_MANIFEST}")"
  network_source="$(source_without_comments "${REPO_ROOT}/${NETWORK_SOURCE}")"
  owner_plan_source="$(source_without_comments "${REPO_ROOT}/${OWNER_PLAN}")"
  owner_proof_source="$(source_without_comments "${REPO_ROOT}/${OWNER_PROOF}")"
  legacy_mutation_source=""
}

apply_test_mutation() {
  case "${NIMBUS_NETWORK_NNC64_TEST_MUTATION:-}" in
    '') ;;
    missing-command-vocabulary)
      replace_once workload_provision_source 'pub enum WorkloadProvisionCommandMode' 'pub enum RemovedCommandMode'
      ;;
    extra-command-mode)
      replace_once workload_provision_source $'    Inspect,\n}' $'    Inspect,\n    Cancel,\n}'
      ;;
    forgeable-command-constructor)
      replace_once compute_dispatch_source '    fn from_confirmation(' '    pub fn from_confirmation('
      ;;
    missing-confirmed-transition-id)
      replace_once compute_dispatch_source 'transition_id: WorkloadSagaTransitionId' 'removed_transition_id: ()'
      ;;
    missing-confirmed-revision)
      replace_once compute_dispatch_source 'confirmed_revision: WorkloadSagaRevision' 'removed_confirmed_revision: ()'
      ;;
    missing-attempt-id)
      replace_once compute_dispatch_source 'attempt_id: WorkloadProvisionAttemptId' 'removed_attempt_id: ()'
      ;;
    missing-generation)
      replace_once compute_dispatch_source 'generation: WorkloadGeneration' 'removed_generation: ()'
      ;;
    missing-desired-digest)
      replace_once compute_dispatch_source 'desired_digest: WorkloadDesiredDigest' 'removed_desired_digest: ()'
      ;;
    missing-provider-id)
      replace_once workload_provision_source 'provider_id: NetworkProviderId' 'removed_network_provider_id: ()'
      ;;
    missing-provider-source-digest)
      replace_once workload_provision_source 'provider_source_digest: NetworkCapabilitySourceDigest' 'removed_network_provider_source_digest: ()'
      ;;
    missing-subject-fence)
      replace_once compute_dispatch_source 'subjects: WorkloadProvisionSubjects' 'removed_subjects: ()'
      ;;
    missing-command-id-domain)
      replace_once workload_provision_source 'nimbus.compute.workload.provision.command.id.v1' 'removed-command-id-domain'
      ;;
    missing-dispatch-epoch)
      replace_once workload_provision_source 'pub struct WorkloadProvisionDispatchEpoch' 'pub struct RemovedDispatchEpoch'
      ;;
    result-loses-command-fence)
      replace_once compute_dispatch_source \
        $'pub struct WorkloadProvisionCommandResult {\n    command_id: WorkloadProvisionCommandId' \
        $'pub struct WorkloadProvisionCommandResult {\n    removed_result_command_id: ()'
      ;;
    unknown-inspection-result)
      replace_once workload_provision_source $'    Succeeded,\n}' $'    Succeeded,\n    Unknown,\n}'
      ;;
    missing-inspection-absent)
      replace_once workload_provision_source '    Absent,' '    RemovedAbsent,'
      ;;
    missing-inspection-in-progress)
      replace_once workload_provision_source '    InProgress,' '    RemovedInProgress,'
      ;;
    retry-changes-attempt-id)
      replace_once compute_dispatch_tests_source 'inspection_absence_authorizes_same_attempt_next_epoch' 'inspection_absence_changes_attempt_id'
      ;;
    retry-reuses-dispatch-epoch)
      replace_once compute_dispatch_tests_source 'absence_retry_increments_dispatch_epoch_exactly_once' 'absence_retry_reuses_dispatch_epoch'
      ;;
    retry-crosses-absence-revision)
      replace_once workload_provision_tests_source 'retry_authorization_wire_rejects_crossed_absence_revision' 'retry_authorization_wire_accepts_crossed_absence_revision'
      ;;
    fixed-revision-offset-retry)
      append_source workload_state_source 'fn fixed_retry_history() { let after_three = after_two.checked_next(); }'
      ;;
    ambiguous-cas-executes)
      append_source compute_dispatch_source 'fn bad() { let _ = (WorkloadSagaConfirmation::ConfirmedAfterAmbiguity, WorkloadProvisionCommandMode::Execute); }'
      ;;
    unchanged-cas-executes)
      append_source compute_dispatch_source 'fn bad() { let _ = (WorkloadSagaConfirmation::ConfirmedReplay, WorkloadProvisionCommandMode::Execute); }'
      ;;
    execute-before-attempt-cas)
      replace_once compute_dispatch_tests_source 'unconfirmed_recovery_candidate_cannot_form_provider_command' 'unconfirmed_recovery_candidate_forms_provider_command'
      ;;
    source-mismatch-effects)
      replace_once compute_dispatch_tests_source 'current_source_mismatch_rejects_before_attempt_cas' 'current_source_mismatch_dispatches'
      ;;
    provider-report-mismatch-effects)
      replace_once compute_dispatch_tests_source 'provider_report_digest_mismatch_rejects_before_effect' 'provider_report_digest_mismatch_dispatches'
      ;;
    missing-reserve-command)
      replace_once compute_dispatch_tests_source 'reserve_command_mapping_is_exact' 'reserve_command_mapping_is_missing'
      ;;
    missing-prepare-command)
      replace_once compute_dispatch_tests_source 'prepare_command_mapping_is_exact' 'prepare_command_mapping_is_missing'
      ;;
    missing-attach-command)
      replace_once compute_dispatch_tests_source 'attach_command_mapping_is_exact' 'attach_command_mapping_is_missing'
      ;;
    missing-prerequisite-inspection)
      replace_once compute_dispatch_tests_source 'activation_prerequisite_command_mapping_is_exact' 'activation_prerequisite_mapping_is_missing'
      ;;
    missing-activate-command)
      replace_once compute_dispatch_tests_source 'activate_command_mapping_is_exact' 'activate_command_mapping_is_missing'
      ;;
    missing-readiness-inspection)
      replace_once compute_dispatch_tests_source 'workload_readiness_command_mapping_is_exact' 'workload_readiness_mapping_is_missing'
      ;;
    publish-before-ready)
      replace_once compute_dispatch_tests_source 'prepare_attach_and_activate_cannot_publish' 'prepare_attach_and_activate_publish'
      ;;
    missing-publication-observation)
      replace_once compute_dispatch_tests_source 'publication_observation_command_mapping_is_exact' 'publication_observation_mapping_is_missing'
      ;;
    definite-failure-emits-later-command)
      replace_once compute_dispatch_tests_source 'definite_failure_never_dispatches_a_later_step' 'definite_failure_dispatches_later'
      ;;
    ambiguous-retries-without-inspection)
      replace_once compute_dispatch_tests_source 'ambiguous_publication_inspects_before_retry' 'ambiguous_publication_retries_directly'
      ;;
    in-progress-retries)
      replace_once compute_dispatch_tests_source 'inspection_in_progress_never_retries' 'inspection_in_progress_retries'
      ;;
    concurrent-provider-duplicate)
      replace_once compute_dispatch_tests_source 'concurrent_dispatchers_create_one_provider_effect' 'concurrent_dispatchers_duplicate_provider_effect'
      ;;
    missing-effect-crash-cut)
      replace_once compute_dispatch_tests_source 'crash_after_effect_before_result_cas_inspects' 'missing_effect_result_crash_cut'
      ;;
    fresh-process-snapshot-handoff)
      replace_once compute_dispatch_tests_source 'fresh_process_reopens_engine_without_snapshot_handoff' 'fresh_process_receives_snapshot_handoff'
      ;;
    duplicate-coordinator)
      append_source compute_saga_source 'pub struct WorkloadSagaCoordinator;'
      ;;
    duplicate-store)
      append_source workload_store_source 'pub trait WorkloadSagaStore {}'
      ;;
    god-provider-trait)
      append_source compute_dispatch_source 'pub trait NetworkProvider {}'
      ;;
    network-effect-interface)
      append_source network_source 'pub trait NetworkEffectProvider { fn bind(&self); }'
      ;;
    portable-provider-handle)
      append_source workload_provision_source 'struct PortableProviderLeak { provider_handle: String }'
      ;;
    random-parent-attempt-id)
      append_source legacy_mutation_source 'fn random_parent_attempt() { let _ = Ulid::new(); }'
      ;;
    missing-forwarded-command-proof)
      replace_once machine_guest_caller_tests_source \
        'real_registry_substitution_publishes_and_observes_exact_forwarded_command' \
        'forwarded_parent_drops_canonical_command_identity'
      ;;
    cloud-functions-effect)
      append_source cloud_functions_host_source 'fn activate() { ensure_service_binding_for_decision_async(); }'
      ;;
    missing-container-provider-connector)
      replace_nth compute_provider_source \
        'backend.attempt_idempotency_journal()?' \
        'backend.removed_container_attempt_journal_connector()?' 1
      append_source compute_provider_source \
        'fn decoy_container_connector() { let _ = "backend.attempt_idempotency_journal()?"; }'
      ;;
    missing-krun-provider-connector)
      replace_nth compute_provider_source \
        'backend.attempt_idempotency_journal()?' \
        'backend.removed_krun_attempt_journal_connector()?' 2
      append_source compute_provider_source \
        'fn decoy_krun_connector() { let _ = "backend.attempt_idempotency_journal()?"; }'
      ;;
    *) add_error "unknown NNC6.4 test mutation: ${NIMBUS_NETWORK_NNC64_TEST_MUTATION}" ;;
  esac
}

verify_contract() {
  load_sources
  apply_test_mutation

  confirmed_command_block="$(printf '%s\n' "${compute_dispatch_source}" |
    extract_rust_item 'pub struct ConfirmedWorkloadProvisionCommand')"
  confirmed_command_impl="$(printf '%s\n' "${compute_dispatch_source}" |
    extract_rust_item 'impl ConfirmedWorkloadProvisionCommand')"
  command_result_block="$(printf '%s\n' "${compute_dispatch_source}" |
    extract_rust_item 'pub struct WorkloadProvisionCommandResult')"
  absence_evidence_block="$(printf '%s\n' "${workload_provision_source}" |
    extract_rust_item 'pub struct WorkloadProvisionAbsenceEvidence')"
  dispatch_claim_block="$(printf '%s\n' "${workload_provision_source}" |
    extract_rust_item 'pub struct WorkloadProvisionDispatchClaim')"
  provider_target_block="$(printf '%s\n' "${workload_provision_source}" |
    extract_rust_item 'pub enum WorkloadProvisionProviderTarget')"

  required_errors="${#NNC64_ERRORS[@]}"
  for required in \
    workload_provision_source workload_state_source workload_store_source \
    compute_saga_source compute_decision_source compute_state_source server_state_source \
    compute_phase_provider_source compute_provider_source sandbox_backend_source sandbox_provision_source \
    sandbox_container_provider_source sandbox_krun_provider_source \
    services_registry_source compose_lifecycle_source \
    node_reconciler_source cloud_functions_host_source network_manifest_source \
    owner_plan_source; do
    if [ -z "${!required}" ]; then
      add_error "required-inputs-and-tools: missing or empty ${required}"
    fi
  done
  for tool in git node rg awk sed; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      add_error "required-inputs-and-tools: missing ${tool}"
    fi
  done
  if [ "${#NNC64_ERRORS[@]}" -eq "${required_errors}" ]; then pass_check; fi

  check_literals "routing-proof-and-completion-baseline" "${owner_plan_source}
${owner_proof_source}" 'NNC6.4' 'provider dispatch' '40 checks' '50 passed'

  pin_errors="${#NNC64_ERRORS[@]}"
  if [ "${NIMBUS_NETWORK_NNC64_TEST_PINNED_HISTORY:-}" = "present" ]; then
    :
  elif ! git -C "${REPO_ROOT}" cat-file -e "${NNC63B_COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "nnc63b-completion-checkpoint-pin: missing ${NNC63B_COMPLETION_CHECKPOINT}"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${pin_errors}" ]; then pass_check; fi

  check_exact_variants "confirmed-command-closed-vocabulary" "${workload_provision_source}" \
    'pub enum WorkloadProvisionCommandMode' 'Execute Inspect '

  constructor_errors="${#NNC64_ERRORS[@]}"
  if ! printf '%s\n' "${confirmed_command_impl}" |
    rg -q -F 'fn from_confirmation('; then
    add_error "confirmed-command-private-construction: missing private confirmation constructor"
  fi
  if printf '%s\n' "${confirmed_command_impl}" |
    rg -q 'pub([[:space:]]*\([^)]*\))?[[:space:]]+fn[[:space:]]+(new|from_confirmation)[[:space:]]*\('; then
    add_error "confirmed-command-private-construction: command constructor is publicly forgeable"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${constructor_errors}" ]; then pass_check; fi

  check_literals "confirmed-command-record-identity" "${confirmed_command_block}" \
    'pub struct ConfirmedWorkloadProvisionCommand' 'key: WorkloadSagaKey' \
    'saga_id: WorkloadSagaId' 'attempt_id: WorkloadProvisionAttemptId'

  check_literals "confirmed-command-revision-transition-fence" "${confirmed_command_block}" \
    'issuing_revision: WorkloadSagaRevision' 'confirmed_revision: WorkloadSagaRevision' \
    'transition_id: WorkloadSagaTransitionId'

  check_literals "confirmed-command-generation-digest-fence" "${confirmed_command_block}
${confirmed_command_impl}" \
    'generation: WorkloadGeneration' 'desired_digest: WorkloadDesiredDigest' \
    'source: WorkloadProvisionSourceEvidence' 'network_plan_digest: NetworkPlanDigest' \
    'pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest' \
    'self.source.source_digest()'

  provider_fence_errors="${#NNC64_ERRORS[@]}"
  provider_target_variants="$(printf '%s\n' "${provider_target_block}" |
    sed -nE 's/^[[:space:]]{4}([A-Z][A-Za-z0-9_]*)[[:space:]]*([{(,]).*/\1/p' |
    sort | tr '\n' ' ')"
  if [ "${provider_target_variants}" != 'Execution Network ' ]; then
    add_error "confirmed-command-provider-subject-fence: expected exact Execution/Network provider target vocabulary"
  fi
  for literal in \
    'role: NetworkCapabilityRole' \
    'provider_id: NetworkProviderId' \
    'provider_source_digest: NetworkCapabilitySourceDigest' \
    'provider_id: WorkloadExecutionProviderId' \
    'provider_source_digest: WorkloadProvisionSourceDigest'; do
    if ! printf '%s\n' "${provider_target_block}" | rg -q -F "${literal}"; then
      add_error "confirmed-command-provider-subject-fence: missing ${literal}"
    fi
  done
  for literal in \
    'provider_target: WorkloadProvisionProviderTarget' \
    'step: WorkloadProvisionStep' 'subjects: WorkloadProvisionSubjects'; do
    if ! printf '%s\n' "${confirmed_command_block}" | rg -q -F "${literal}"; then
      add_error "confirmed-command-provider-subject-fence: missing ${literal}"
    fi
  done
  if [ "${#NNC64_ERRORS[@]}" -eq "${provider_fence_errors}" ]; then pass_check; fi

  check_literals "confirmed-command-domain-separated-id" "${workload_provision_source}" \
    'pub struct WorkloadProvisionCommandId' \
    'nimbus.compute.workload.provision.command.id.v1'

  dispatch_fence_errors="${#NNC64_ERRORS[@]}"
  for literal in \
    'pub struct WorkloadProvisionDispatchEpoch' 'pub struct WorkloadProvisionDispatchClaim' \
    'pub enum WorkloadProvisionDispatchAuthorization' 'Initial' 'RetryAfterAbsence'; do
    if ! printf '%s\n' "${workload_provision_source}" | rg -q -F "${literal}"; then
      add_error "dispatch-epoch-and-authorization: missing ${literal}"
    fi
  done
  if ! printf '%s\n' "${dispatch_claim_block}" |
    rg -q -F 'provider_target: WorkloadProvisionProviderTarget'; then
    add_error "dispatch-epoch-and-authorization: dispatch claim lacks exact provider target"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${dispatch_fence_errors}" ]; then pass_check; fi

  check_literals "effect-result-command-correlation" "${command_result_block}" \
    'pub struct WorkloadProvisionCommandResult' 'command_id: WorkloadProvisionCommandId' \
    'attempt_id: WorkloadProvisionAttemptId' 'dispatch_epoch: WorkloadProvisionDispatchEpoch' \
    'provider_target: WorkloadProvisionProviderTarget'

  check_exact_variants "inspection-result-closed-vocabulary" "${workload_provision_source}" \
    'pub enum WorkloadProvisionInspectionResult' \
    'Absent Ambiguous DefiniteFailure InProgress Succeeded '
  if ! printf '%s\n' "${workload_provision_tests_source}" |
    rg -q -F 'dispatch_epoch_and_inspection_wire_reject_unknown_noncanonical_values'; then
    add_error "inspection-result-closed-vocabulary: strict wire proof missing"
  fi

  check_literals "absence-evidence-complete-fence" "${absence_evidence_block}" \
    'pub struct WorkloadProvisionAbsenceEvidence' 'attempt_id: WorkloadProvisionAttemptId' \
    'dispatch_epoch: WorkloadProvisionDispatchEpoch' \
    'confirmed_revision: WorkloadSagaRevision' \
    'transition_id: WorkloadSagaTransitionId' \
    'provider_target: WorkloadProvisionProviderTarget' \
    'step: WorkloadProvisionStep' 'evidence: WorkloadOwnerEvidenceDigest'

  portable_errors="${#NNC64_ERRORS[@]}"
  for seam in 'Ready' 'DispatchPending' 'InspectionRequired' 'DefiniteFailure'; do
    if ! printf '%s\n' "${workload_provision_source}" | rg -q -F "${seam}"; then
      add_error "portable-disposition-retry-state: missing ${seam}"
    fi
  done
  if printf '%s\n' "${workload_provision_source}" |
    rg -q 'provider_handle|socket_handle|assigned_ip'; then
    add_error "portable-disposition-retry-state: portable provider handle or address leaked"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${portable_errors}" ]; then pass_check; fi

  state_errors="${#NNC64_ERRORS[@]}"
  for seam in \
    'ready_to_initial_dispatch' 'dispatch_to_inspection' \
    'inspection_to_retry_dispatch' 'dispatch_to_success' 'dispatch_to_definite_failure'; do
    if ! printf '%s\n' "${workload_state_source}" | rg -q -F "${seam}"; then
      add_error "explicit-disposition-transition-graph: missing ${seam}"
    fi
  done
  if printf '%s\n' "${workload_state_source}" | rg -q -F 'let after_three ='; then
    add_error "explicit-disposition-transition-graph: fixed revision-offset retry remains"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${state_errors}" ]; then pass_check; fi

  check_literals "same-attempt-monotonic-retry" "${workload_provision_source}
${workload_provision_tests_source}
${workload_state_tests_source}
${compute_dispatch_tests_source}" \
    'absence.confirmed_revision.checked_next() != Some(self.claimed_revision)' \
    'retry_authorization_wire_rejects_crossed_absence_revision' \
    'retry_reusing_skipping_or_crossing_absence_transition_is_rejected' \
    'inspection_absence_authorizes_same_attempt_next_epoch' \
    'absence_retry_increments_dispatch_epoch_exactly_once' \
    'retry_without_absence_evidence_is_rejected'

  check_literals "exhaustive-command-result-reducer" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'reduce_command_result' 'WorkloadProvisionInspectionResult' \
    'every_phase_mode_and_command_result_is_exhaustive'

  check_literals "cas-confirmation-provenance" "${compute_dispatch_source}" \
    'pub enum WorkloadSagaConfirmation' 'AppliedByThisCall' \
    'ConfirmedAfterAmbiguity' 'ConfirmedReplay' 'Conflict' 'UnresolvedAmbiguity'

  winner_errors="${#NNC64_ERRORS[@]}"
  check_source="$(printf '%s\n' "${compute_dispatch_source}" | source_without_comments_or_strings)"
  if ! printf '%s\n' "${compute_dispatch_tests_source}" |
    rg -q -F 'direct_cas_winner_executes_exact_attempt_once'; then
    add_error "direct-winner-only-execute: missing direct-winner proof"
  fi
  if printf '%s\n' "${check_source}" |
    rg -q 'Confirmed(AfterAmbiguity|Replay).{0,120}WorkloadProvisionCommandMode::Execute'; then
    add_error "direct-winner-only-execute: replay or ambiguous confirmation can execute"
  fi
  if ! printf '%s\n' "${compute_dispatch_tests_source}" |
    rg -q -F 'unconfirmed_recovery_candidate_cannot_form_provider_command'; then
    add_error "direct-winner-only-execute: unconfirmed recovery candidate proof missing"
  fi
  if ! printf '%s\n' "${compute_dispatch_source}" |
    rg -q -F 'confirmed_record: Option<WorkloadSagaRecord>'; then
    add_error "direct-winner-only-execute: conflict can expose an unconfirmed candidate as durable"
  fi
  if ! printf '%s\n' "${compute_dispatch_source}" |
    rg -q -F 'self.store.load(key).await?'; then
    add_error "direct-winner-only-execute: recovery inspection does not load durable truth"
  fi
  if ! printf '%s\n' "${compute_dispatch_tests_source}" |
    rg -q -F 'conflict_exposes_no_candidate_record_or_command'; then
    add_error "direct-winner-only-execute: conflict candidate exposure proof missing"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${winner_errors}" ]; then pass_check; fi

  check_literals "replay-and-ambiguity-inspect-only" "${compute_dispatch_tests_source}" \
    'confirmed_replay_inspects_without_execute' \
    'ambiguous_cas_confirmation_inspects_without_execute' \
    'unresolved_cas_ambiguity_emits_no_command'

  check_literals "bounded-ambiguous-store-read" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'resolve_ambiguous_confirmation' \
    'one_fresh_read_after_ambiguous_command_cas' \
    'ambiguous_successor_cas_reads_before_later_decision'

  check_literals "current-source-before-dispatch" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'validate_current_source' \
    'current_source_mismatch_rejects_before_attempt_cas'

  check_literals "current-provider-report-before-dispatch" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'validate_current_provider_report' \
    'provider_report_digest_mismatch_rejects_before_effect'

  registry_errors="${#NNC64_ERRORS[@]}"
  for seam in \
    'select_exact_provider' 'provider_target()' \
    'network_steps_bind_exact_selected_role_provider_and_digest' \
    'prepare_and_activate_bind_execution_provider_without_network_role' \
    'resource_free_network_steps_fabricate_no_provider_target'; do
    if ! printf '%s\n%s\n' \
      "${compute_dispatch_source}" "${compute_dispatch_tests_source}" |
      rg -q -F "${seam}"; then
      add_error "exact-provider-registry-routing: missing ${seam}"
    fi
  done
  if printf '%s\n' "${compute_dispatch_source}" |
    rg -q '\.(first|next)\([[:space:]]*\)|iter\(\)\.next\('; then
    add_error "exact-provider-registry-routing: first-available provider fallback remains"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${registry_errors}" ]; then pass_check; fi

  capability_errors="${#NNC64_ERRORS[@]}"
  for seam in \
    'trait NetworkReservationCapability' 'trait WorkloadPreparationCapability' \
    'trait NetworkAttachmentCapability' 'trait WorkloadActivationCapability' \
    'trait WorkloadReadinessCapability' 'trait IngressPublicationCapability' \
    'ContainerProvisionAdapter' 'KrunProvisionAdapter' \
    'ForwardedMachineProvisionAdapter' 'ServerIngressPublicationAdapter'; do
    if ! printf '%s\n%s\n%s\n%s\n' \
      "${compute_dispatch_source}" "${compute_provider_source}" "${server_state_source}" \
      "${machine_parent_source}" |
      rg -q -F "${seam}"; then
      add_error "small-real-capability-seams: missing ${seam}"
    fi
  done
  if printf '%s\n' "${compute_dispatch_source}" | rg -q 'trait[[:space:]]+NetworkProvider'; then
    add_error "small-real-capability-seams: god NetworkProvider trait exists"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${capability_errors}" ]; then pass_check; fi

  provider_journal_errors="${#NNC64_ERRORS[@]}"
  for literal in attempt_idempotency_journal claim_dispatch_epoch record_observation; do
    if ! printf '%s\n' "${compute_phase_provider_source}" | rg -q -F "${literal}"; then
      add_error "provider-local-attempt-idempotency: shared phase adapter missing ${literal}"
    fi
  done
  for literal in claim_dispatch_epoch reject_stale_dispatch_epoch adopt_exact_attempt; do
    if ! printf '%s\n' "${sandbox_provision_source}" | rg -q -F "${literal}"; then
      add_error "provider-local-attempt-idempotency: sandbox journal missing ${literal}"
    fi
  done
  container_adapter_impl="$(printf '%s\n' "${compute_provider_source}" |
    extract_rust_item 'impl ContainerProvisionAdapter')"
  krun_adapter_impl="$(printf '%s\n' "${compute_provider_source}" |
    extract_rust_item 'impl KrunProvisionAdapter')"
  container_journal_connector="$(printf '%s\n' "${sandbox_container_provider_source}" |
    extract_rust_item 'pub fn attempt_idempotency_journal(')"
  krun_journal_connector="$(printf '%s\n' "${sandbox_krun_provider_source}" |
    extract_rust_item 'pub fn attempt_idempotency_journal(')"
  if ! printf '%s\n' "${container_adapter_impl}" |
    rg -q -F 'ProviderProvisionPhaseAdapter::new(backend.attempt_idempotency_journal()?)'; then
    add_error "provider-local-attempt-idempotency: Container adapter does not open its backend journal"
  fi
  if ! printf '%s\n' "${krun_adapter_impl}" |
    rg -q -F 'ProviderProvisionPhaseAdapter::new(backend.attempt_idempotency_journal()?)'; then
    add_error "provider-local-attempt-idempotency: Krun adapter does not open its backend journal"
  fi
  for provider_connector in \
    "container|${container_journal_connector}|container-runtime" \
    "krun|${krun_journal_connector}|krun-runtime"; do
    provider="${provider_connector%%|*}"
    connector_and_namespace="${provider_connector#*|}"
    connector="${connector_and_namespace%|*}"
    namespace="${provider_connector##*|}"
    for literal in \
      'ProviderProvisionAttemptJournal::open' 'config.workload_state_root' "${namespace}"; do
      if ! printf '%s\n' "${connector}" | rg -q -F "${literal}"; then
        add_error "provider-local-attempt-idempotency: ${provider} backend journal connector missing ${literal}"
      fi
    done
  done
  if [ "${#NNC64_ERRORS[@]}" -eq "${provider_journal_errors}" ]; then pass_check; fi

  authority_errors="${#NNC64_ERRORS[@]}"
  authority_source="${workload_store_source}
${compute_saga_source}"
  coordinator_count="$(printf '%s\n' "${authority_source}" |
    rg -o 'pub[[:space:]]+struct[[:space:]]+WorkloadSagaCoordinator' |
    awk 'END { print NR + 0 }')"
  store_count="$(printf '%s\n' "${authority_source}" |
    rg -o 'pub[[:space:]]+trait[[:space:]]+WorkloadSagaStore' |
    awk 'END { print NR + 0 }')"
  if [ "${coordinator_count}" -ne 1 ]; then
    add_error "single-store-single-coordinator: expected one coordinator, observed ${coordinator_count}"
  fi
  if [ "${store_count}" -ne 1 ]; then
    add_error "single-store-single-coordinator: expected one store, observed ${store_count}"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${authority_errors}" ]; then pass_check; fi

  check_literals "managed-compute-required-dispatch-composition" "${compute_state_source}
${server_state_source}" 'provision_capabilities:' 'source_authority:' \
    'WorkloadProvisioner' 'saga_store:' 'network_manager:'

  check_literals "reserve-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'ReserveNetwork' 'NetworkReservationCapability' \
    'reserve_command_mapping_is_exact'

  check_literals "prepare-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'PrepareWorkload' 'WorkloadPreparationCapability' \
    'prepare_command_mapping_is_exact'

  check_literals "attach-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'AttachNetwork' 'NetworkAttachmentCapability' \
    'attach_command_mapping_is_exact'

  check_literals "activation-prerequisite-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'InspectActivationPrerequisites' \
    'activation_prerequisite_command_mapping_is_exact'

  check_literals "activate-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'ActivateWorkload' 'WorkloadActivationCapability' \
    'activate_command_mapping_is_exact'

  check_literals "workload-readiness-command-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'InspectWorkloadReadiness' 'WorkloadReadinessCapability' \
    'workload_readiness_command_mapping_is_exact'

  check_literals "publish-observe-and-nonpublish-mapping" "${compute_dispatch_source}
${compute_dispatch_tests_source}" 'Publish' 'ObservePublication' 'IngressPublicationCapability' \
    'prepare_attach_and_activate_cannot_publish' \
    'publication_observation_command_mapping_is_exact' \
    'withheld_and_prepare_only_emit_no_provider_command'

  check_literals "definite-failure-and-ambiguity-behavior" "${compute_dispatch_tests_source}" \
    'definite_failure_never_dispatches_a_later_step' \
    'ambiguous_publication_inspects_before_retry' \
    'inspection_in_progress_never_retries'

  check_literals "crash-concurrency-and-fresh-process-proof" "${compute_dispatch_tests_source}" \
    'concurrent_dispatchers_create_one_provider_effect' \
    'crash_after_dispatch_cas_before_effect_inspects' \
    'crash_after_effect_before_result_cas_inspects' \
    'fresh_process_reopens_engine_without_snapshot_handoff'

  caller_errors="${#NNC64_ERRORS[@]}"
  native_caller_source="${compute_resource_source}
${compute_sandboxes_source}
${compute_services_source}"
  for seam in 'provision_with_source_reservation' 'provision_standalone_sandbox' \
    'provision_sandbox_service'; do
    if ! printf '%s\n' "${native_caller_source}" | rg -q -F "${seam}"; then
      add_error "positive-and-read-only-caller-census: native caller path missing ${seam}"
    fi
  done
  if ! printf '%s\n' "${native_caller_tests_source}" |
    rg -q 'fn[[:space:]]+native_service_and_sandbox_callers_use_compute_dispatch[[:space:]]*\('; then
    add_error "positive-and-read-only-caller-census: missing native behavior test"
  fi
  for seam in 'provision_sandbox_service' 'WorkloadProvisionCancellation'; do
    if ! printf '%s\n' "${convex_async_source}" | rg -q -F "${seam}"; then
      add_error "positive-and-read-only-caller-census: Convex async path missing ${seam}"
    fi
  done
  if ! printf '%s\n' "${convex_lookup_tests_source}" |
    rg -q 'fn[[:space:]]+convex_async_activation_uses_compute_dispatch[[:space:]]*\('; then
    add_error "positive-and-read-only-caller-census: missing Convex async behavior test"
  fi
  for seam in 'resource_provisioner' 'EngineWorkloadSagaStore'; do
    if ! printf '%s\n' "${compose_lifecycle_source}" | rg -q -F "${seam}"; then
      add_error "positive-and-read-only-caller-census: Compose path missing ${seam}"
    fi
  done
  if ! printf '%s\n' "${compose_caller_tests_source}" |
    rg -q 'fn[[:space:]]+compose_local_and_forwarded_use_compute_dispatch[[:space:]]*\('; then
    add_error "positive-and-read-only-caller-census: missing Compose behavior test"
  fi
  for seam in 'MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH' \
    'MachineApiWorkloadProvisionCommandEnvelope' 'ForwardedMachineProvisionAdapter'; do
    if ! printf '%s\n%s\n%s\n' "${nimbus_machine_api_source}" \
      "${machine_service_source}" "${machine_parent_source}" | rg -q -F "${seam}"; then
      add_error "positive-and-read-only-caller-census: Machine/guest path missing ${seam}"
    fi
  done
  for proof in \
    'machine_api_and_guest_node_use_fenced_commands' \
    'machine_api_and_guest_use_exact_compute_phase_dispatch' \
    'real_registry_substitution_publishes_and_observes_exact_forwarded_command'; do
    if ! printf '%s\n' "${machine_guest_caller_tests_source}" |
      rg -q "fn[[:space:]]+${proof}[[:space:]]*\\("; then
      add_error "positive-and-read-only-caller-census: missing Machine/guest proof ${proof}"
    fi
  done
  if ! printf '%s\n' "${machine_parent_source}" |
    rg -q 'fn[[:space:]]+legacy_service_intent_cannot_represent_canonical_command_identity[[:space:]]*\('; then
    add_error "positive-and-read-only-caller-census: missing parent-journal canonical identity proof"
  fi
  for seam in 'convex_sync_and_invocation_snapshots_are_read_only' \
    'cloud_functions_snapshots_have_zero_activation_store_or_provider_calls'; do
    if ! printf '%s\n' "${caller_proof_source}" | rg -q -F "${seam}"; then
      add_error "positive-and-read-only-caller-census: missing ${seam}"
    fi
  done
  cloud_source="${cloud_functions_host_source}
${cloud_functions_http_source}
${cloud_functions_trigger_source}"
  cloud_code="$(printf '%s\n' "${cloud_source}" | source_without_comments_or_strings)"
  if ! printf '%s\n' "${cloud_source}" | rg -q -F 'snapshot_for_tenant'; then
    add_error "positive-and-read-only-caller-census: Cloud Functions snapshot is missing"
  fi
  if printf '%s\n' "${cloud_code}" |
    rg -q 'ensure_service_binding|submit_intent|WorkloadProvisionDispatcher|SandboxBackend'; then
    add_error "positive-and-read-only-caller-census: Cloud Functions gained workload effects"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${caller_errors}" ]; then pass_check; fi

  deletion_errors="${#NNC64_ERRORS[@]}"
  legacy_source="$(raw_source "${REPO_ROOT}/${SANDBOX_BACKEND}")
$(raw_source "${REPO_ROOT}/${SANDBOX_CONTAINER_PROVIDER}")
$(raw_source "${REPO_ROOT}/${SANDBOX_KRUN_PROVIDER}")
$(raw_source "${REPO_ROOT}/${COMPUTE_DECISION}")
$(raw_source "${REPO_ROOT}/${SERVICES_REGISTRY}")
$(raw_source "${REPO_ROOT}/${SERVICES_MANAGER}")
$(raw_source "${REPO_ROOT}/${SERVICES_ACTIVATION}")
$(raw_source "${REPO_ROOT}/${SERVICES_START}")
$(raw_source "${REPO_ROOT}/${SERVICES_SANDBOXES}")
$(raw_source "${REPO_ROOT}/${COMPOSE_LIFECYCLE}")
$(raw_source "${REPO_ROOT}/${NIMBUS_MACHINE_API}")
$(raw_source "${REPO_ROOT}/${MACHINE_ROUTES}")
$(raw_source "${REPO_ROOT}/${MACHINE_SERVICE}")
$(raw_source "${REPO_ROOT}/${MACHINE_CLIENT}")
$(raw_source "${REPO_ROOT}/${MACHINE_STUB_CLIENT}")
$(raw_source "${REPO_ROOT}/${MACHINE_STUB_BACKEND}")
$(raw_source "${REPO_ROOT}/${MACHINE_BACKEND}")
$(raw_source "${REPO_ROOT}/${MACHINE_BACKEND_PROVISION}")
$(raw_source "${REPO_ROOT}/${MACHINE_PUBLICATION}")
$(raw_source "${REPO_ROOT}/${MACHINE_CAPABILITIES}")
$(raw_source "${REPO_ROOT}/${NODE_RECONCILER}")
$(raw_source "${REPO_ROOT}/${NODE_HOST_LIFECYCLE}")
$(raw_source "${REPO_ROOT}/${NODE_DIRECT_PROCESS}")
$(raw_source "${REPO_ROOT}/${NODE_SYSTEMD_TRANSIENT}")
$(raw_source "${REPO_ROOT}/${NODE_EXECUTOR}")
$(raw_source "${REPO_ROOT}/${CLI_LIB}")
${legacy_mutation_source}"
  legacy_code="$(printf '%s\n' "${legacy_source}" | source_without_comments_or_strings)"
  legacy_hits="$(printf '%s\n' "${legacy_code}" |
    rg -n 'start_service_launch|start_sync|finish_start|ensure_service_binding(_for_decision)?_async|activations_in_progress|ActivationClaim|MACHINE_API_SERVICE_SANDBOX_(IMAGE|BUILD)_START_PATH|MachineApiServiceSandbox(Image|Build)StartRequest|MachineApiServiceSandboxStartResponse|start_service_sandbox_from_(image|build)|machine_api_start_(image|build)_service_sandbox|admit_workload_spec|NodeWorkloadCoordinator::new|reconcile_running|NodeWorkloadExecutor|Ulid::new|fn[[:space:]]+start[[:space:]]*(<[^>]+>)?[[:space:]]*\(' |
    awk 'BEGIN { separator = "" } { printf "%s%s", separator, $0; separator = "; " }' || true)"
  if [ -n "${legacy_hits}" ]; then
    add_error "legacy-deletion-path-dependency-effect-contract: legacy provision authority remains: ${legacy_hits}"
  fi
  product_bypass_source="$(raw_source "${REPO_ROOT}/${COMPUTE_SANDBOXES}")
$(raw_source "${REPO_ROOT}/${COMPUTE_SERVICES}")
$(raw_source "${REPO_ROOT}/${COMPUTE_STATE}")
$(raw_source "${REPO_ROOT}/${SERVER_CONVEX_ASYNC}")"
  product_bypass_code="$(printf '%s\n' "${product_bypass_source}" |
    source_without_comments_or_strings)"
  product_bypass_hits="$(printf '%s\n' "${product_bypass_code}" |
    rg -n 'create_sandbox_resource_for_context_async|inspect_sandbox_resource_async|start_service_for_context_async|restart_service_for_context_async|ensure_service_binding(_for_decision)?_async|teardown_tenant_async' |
    awk 'BEGIN { separator = "" } { printf "%s%s", separator, $0; separator = "; " }' || true)"
  if [ -n "${product_bypass_hits}" ]; then
    add_error "legacy-deletion-path-dependency-effect-contract: product caller bypass remains: ${product_bypass_hits}"
  fi
  server_shim_source="$(raw_source "${REPO_ROOT}/${SERVER_CONSTRUCTION}")
$(raw_source "${REPO_ROOT}/${SERVER_ROUTER}")
$(raw_source "${REPO_ROOT}/${SERVER_WORKLOAD_COMPOSITION}")
$(raw_source "${REPO_ROOT}/${CLI_START_BOOT}")
$(raw_source "${REPO_ROOT}/${CLI_DEV_WIRE}")
$(raw_source "${REPO_ROOT}/${MACHINE_LOCAL_SERVER}")"
  server_shim_code="$(printf '%s\n' "${server_shim_source}" |
    source_without_comments_or_strings)"
  server_shim_hits="$(printf '%s\n' "${server_shim_code}" |
    rg -n 'ServeOptions::new|RouterOptions::new|fn[[:space:]]+with_service_manager[[:space:]]*\(' |
    awk 'BEGIN { separator = "" } { printf "%s%s", separator, $0; separator = "; " }' || true)"
  if [ -n "${server_shim_hits}" ]; then
    add_error "legacy-deletion-path-dependency-effect-contract: incomplete server composition shim remains: ${server_shim_hits}"
  fi
  network_code="$(printf '%s\n' "${network_source}" | source_without_comments_or_strings)"
  if printf '%s\n' "${network_code}" |
    rg -q 'trait[[:space:]].*(Effect|Provider)|TcpListener|TcpStream|UdpSocket|SandboxBackend'; then
    add_error "legacy-deletion-path-dependency-effect-contract: nimbus-network gained an effect interface"
  fi
  if printf '%s\n' "${network_manifest_source}" |
    rg -q '^nimbus-(?!core)[A-Za-z0-9_-]*[[:space:]]*=' --pcre2; then
    add_error "legacy-deletion-path-dependency-effect-contract: nimbus-network gained a forbidden workspace dependency"
  fi
  if printf '%s\n%s\n' "${workload_provision_source}" "${compute_dispatch_source}" |
    rg -q 'serde\([^)]*alias|legacyProvision|compatibility'; then
    add_error "legacy-deletion-path-dependency-effect-contract: compatibility path exists"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${deletion_errors}" ]; then pass_check; fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC64_ERRORS=()
  NNC64_CHECKS=0
  verify_contract
  if [ "${#NNC64_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC64_ERRORS[@]}"; do
      printf 'NNC6.4 provider dispatch contract failure: %s\n' "${error}" >&2
    done
    printf 'NNC6.4 provider dispatch contract: %d checks passed, %d failed\n' \
      "${NNC64_CHECKS}" "$((40 - NNC64_CHECKS))" >&2
    return 1
  fi
  if [ "${NNC64_CHECKS}" -ne 40 ]; then
    printf 'NNC6.4 provider dispatch contract failure: expected 40 checks, observed %d\n' \
      "${NNC64_CHECKS}" >&2
    return 1
  fi
  printf 'NNC6.4 provider dispatch contract: 40 checks passed\n'
}

# shellcheck source=scripts/nimbus-network-control-plane/workload-provision-dispatch-self-test.sh
. "${SELF_TEST_SCRIPT_PATH}"

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
