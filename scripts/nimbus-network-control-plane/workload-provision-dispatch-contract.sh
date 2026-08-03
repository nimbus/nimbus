#!/usr/bin/env bash
# Static NNC6.4 contract for confirmed workload-provision dispatch and caller cutover.

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_PATH="${SCRIPT_DIR}/workload-provision-dispatch-contract.sh"
SELF_TEST_SCRIPT_PATH="${SCRIPT_DIR}/workload-provision-dispatch-self-test.sh"
REPO_ROOT="${NIMBUS_NETWORK_NNC64_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
NNC63B_COMPLETION_CHECKPOINT="${NIMBUS_NETWORK_NNC64_NNC63B_CHECKPOINT:-c42c61fb2d97d037069f3b27b9055d6e58f11d1d}"

WORKLOAD_PROVISION="crates/nimbus-workloads/src/saga/provision.rs"
WORKLOAD_STATE="crates/nimbus-workloads/src/saga/state.rs"
WORKLOAD_STORE="crates/nimbus-workloads/src/store.rs"
COMPUTE_SAGA="crates/nimbus-compute/src/workload_saga.rs"
COMPUTE_DECISION="crates/nimbus-compute/src/workload_saga/provision_decision.rs"
COMPUTE_DISPATCH="crates/nimbus-compute/src/workload_saga/provision_dispatch.rs"
COMPUTE_DISPATCH_TESTS="crates/nimbus-compute/src/workload_saga/provision_dispatch/tests.rs"
COMPUTE_STATE="crates/nimbus-compute/src/state.rs"
SERVER_STATE="crates/nimbus-server/src/state.rs"
SANDBOX_BACKEND="crates/nimbus-sandbox/src/backend.rs"
SERVICES_REGISTRY="crates/nimbus-services/src/registry.rs"
SERVICES_START="crates/nimbus-services/src/manager/service_start.rs"
SERVICES_SANDBOXES="crates/nimbus-services/src/manager/sandboxes.rs"
COMPOSE_LIFECYCLE="crates/nimbus-cli/src/compose/lifecycle.rs"
MACHINE_ROUTES="crates/nimbus-cli/src/machine/api/routes.rs"
MACHINE_SERVICE="crates/nimbus-cli/src/machine/api/service_workloads.rs"
NODE_RECONCILER="crates/nimbus-node/src/reconciler.rs"
CLOUD_FUNCTIONS_HOST="crates/nimbus-cloud-functions/src/host_bridge.rs"
CLOUD_FUNCTIONS_HTTP="crates/nimbus-cloud-functions/src/http/invocation.rs"
CLOUD_FUNCTIONS_TRIGGER="crates/nimbus-cloud-functions/src/trigger_executor.rs"
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

append_source() {
  variable="$1"
  addition="$2"
  value="${!variable}"
  printf -v "${variable}" '%s\n%s\n' "${value}" "${addition}"
}

load_sources() {
  workload_provision_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_PROVISION}")"
  workload_state_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STATE}")"
  workload_store_source="$(source_without_comments "${REPO_ROOT}/${WORKLOAD_STORE}")"
  compute_saga_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_SAGA}")"
  compute_decision_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DECISION}")"
  compute_dispatch_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCH}")"
  compute_dispatch_tests_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_DISPATCH_TESTS}")"
  compute_state_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_STATE}")"
  server_state_source="$(source_without_comments "${REPO_ROOT}/${SERVER_STATE}")"
  sandbox_backend_source="$(source_without_comments "${REPO_ROOT}/${SANDBOX_BACKEND}")"
  services_registry_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_REGISTRY}")"
  services_start_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_START}")"
  services_sandboxes_source="$(source_without_comments "${REPO_ROOT}/${SERVICES_SANDBOXES}")"
  compose_lifecycle_source="$(source_without_comments "${REPO_ROOT}/${COMPOSE_LIFECYCLE}")"
  machine_routes_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_ROUTES}")"
  machine_service_source="$(source_without_comments "${REPO_ROOT}/${MACHINE_SERVICE}")"
  node_reconciler_source="$(source_without_comments "${REPO_ROOT}/${NODE_RECONCILER}")"
  cloud_functions_host_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_HOST}")"
  cloud_functions_http_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_HTTP}")"
  cloud_functions_trigger_source="$(source_without_comments "${REPO_ROOT}/${CLOUD_FUNCTIONS_TRIGGER}")"
  network_manifest_source="$(source_without_comments "${REPO_ROOT}/${NETWORK_MANIFEST}")"
  network_source="$(source_without_comments "${REPO_ROOT}/${NETWORK_SOURCE}")"
  owner_plan_source="$(source_without_comments "${REPO_ROOT}/${OWNER_PLAN}")"
  owner_proof_source="$(source_without_comments "${REPO_ROOT}/${OWNER_PROOF}")"
}

apply_test_mutation() {
  case "${NIMBUS_NETWORK_NNC64_TEST_MUTATION:-}" in
    '') ;;
    missing-command-vocabulary)
      replace_once compute_dispatch_source 'pub enum WorkloadProvisionCommandMode' 'pub enum RemovedCommandMode'
      ;;
    extra-command-mode)
      replace_once compute_dispatch_source $'    Inspect,\n}' $'    Inspect,\n    Cancel,\n}'
      ;;
    forgeable-command-constructor)
      append_source compute_dispatch_source 'pub fn new() -> ConfirmedWorkloadProvisionCommand { unreachable!() }'
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
      replace_once compute_dispatch_source 'nimbus.compute.workload.provision.command.id.v1' 'removed-command-id-domain'
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
    retry-lacks-absence-evidence)
      replace_once compute_dispatch_tests_source 'retry_without_absence_evidence_is_rejected' 'retry_without_absence_evidence_is_allowed'
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
      replace_once compute_dispatch_tests_source 'unconfirmed_candidate_cannot_form_provider_command' 'unconfirmed_candidate_forms_provider_command'
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
    old-provision-authority-remains)
      append_source sandbox_backend_source 'pub trait SandboxBackend { fn start(&self); }'
      ;;
    caller-family-bypass)
      replace_once compute_dispatch_tests_source 'machine_api_and_guest_node_use_fenced_commands' 'machine_api_and_guest_node_bypass_compute'
      ;;
    cloud-functions-effect)
      append_source cloud_functions_host_source 'fn activate() { ensure_service_binding_for_decision_async(); }'
      ;;
    *) add_error "unknown NNC6.4 test mutation: ${NIMBUS_NETWORK_NNC64_TEST_MUTATION}" ;;
  esac
}

verify_contract() {
  load_sources
  apply_test_mutation

  confirmed_command_block="$(printf '%s\n' "${compute_dispatch_source}" |
    extract_rust_item 'pub struct ConfirmedWorkloadProvisionCommand')"
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
    sandbox_backend_source services_registry_source compose_lifecycle_source \
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
${owner_proof_source}" 'NNC6.4' 'provider dispatch' '40 checks' '48 passed'

  pin_errors="${#NNC64_ERRORS[@]}"
  if [ "${NIMBUS_NETWORK_NNC64_TEST_PINNED_HISTORY:-}" = "present" ]; then
    :
  elif ! git -C "${REPO_ROOT}" cat-file -e "${NNC63B_COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "nnc63b-completion-checkpoint-pin: missing ${NNC63B_COMPLETION_CHECKPOINT}"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${pin_errors}" ]; then pass_check; fi

  check_exact_variants "confirmed-command-closed-vocabulary" "${compute_dispatch_source}" \
    'pub enum WorkloadProvisionCommandMode' 'Execute Inspect '

  constructor_errors="${#NNC64_ERRORS[@]}"
  if ! printf '%s\n' "${compute_dispatch_source}" |
    rg -q -F 'fn from_confirmation('; then
    add_error "confirmed-command-private-construction: missing private confirmation constructor"
  fi
  if printf '%s\n' "${compute_dispatch_source}" |
    rg -q 'pub[[:space:]]+fn[[:space:]]+(new|from_confirmation)[[:space:]]*\('; then
    add_error "confirmed-command-private-construction: command constructor is publicly forgeable"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${constructor_errors}" ]; then pass_check; fi

  check_literals "confirmed-command-record-identity" "${confirmed_command_block}" \
    'pub struct ConfirmedWorkloadProvisionCommand' 'key: WorkloadSagaKey' \
    'saga_id: WorkloadSagaId' 'attempt_id: WorkloadProvisionAttemptId'

  check_literals "confirmed-command-revision-transition-fence" "${confirmed_command_block}" \
    'issuing_revision: WorkloadSagaRevision' 'confirmed_revision: WorkloadSagaRevision' \
    'transition_id: WorkloadSagaTransitionId'

  check_literals "confirmed-command-generation-digest-fence" "${confirmed_command_block}" \
    'generation: WorkloadGeneration' 'desired_digest: WorkloadDesiredDigest' \
    'source_digest: WorkloadProvisionSourceDigest' 'network_plan_digest: NetworkPlanDigest'

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

  check_literals "confirmed-command-domain-separated-id" "${compute_dispatch_source}" \
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

  check_literals "absence-evidence-complete-fence" "${absence_evidence_block}" \
    'pub struct WorkloadProvisionAbsenceEvidence' 'attempt_id: WorkloadProvisionAttemptId' \
    'dispatch_epoch: WorkloadProvisionDispatchEpoch' \
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

  check_literals "same-attempt-monotonic-retry" "${compute_dispatch_tests_source}" \
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
    rg -q -F 'unconfirmed_candidate_cannot_form_provider_command'; then
    add_error "direct-winner-only-execute: unconfirmed candidate proof missing"
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
    if ! printf '%s\n%s\n%s\n' \
      "${compute_dispatch_source}" "${sandbox_backend_source}" "${server_state_source}" |
      rg -q -F "${seam}"; then
      add_error "small-real-capability-seams: missing ${seam}"
    fi
  done
  if printf '%s\n' "${compute_dispatch_source}" | rg -q 'trait[[:space:]]+NetworkProvider'; then
    add_error "small-real-capability-seams: god NetworkProvider trait exists"
  fi
  if [ "${#NNC64_ERRORS[@]}" -eq "${capability_errors}" ]; then pass_check; fi

  check_literals "provider-local-attempt-idempotency" "${sandbox_backend_source}
${server_state_source}" 'attempt_idempotency_journal' 'claim_dispatch_epoch' \
    'reject_stale_dispatch_epoch' 'adopt_exact_attempt'

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
    'WorkloadProvisionDispatcher' 'saga_store:' 'network_manager:'

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
  for seam in \
    'native_service_and_sandbox_callers_use_compute_dispatch' \
    'convex_async_activation_uses_compute_dispatch' \
    'compose_local_and_forwarded_use_compute_dispatch' \
    'machine_api_and_guest_node_use_fenced_commands' \
    'convex_sync_and_invocation_snapshots_are_read_only' \
    'cloud_functions_snapshots_have_zero_activation_store_or_provider_calls'; do
    if ! printf '%s\n' "${compute_dispatch_tests_source}" | rg -q -F "${seam}"; then
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
  legacy_source="${sandbox_backend_source}
${compute_decision_source}
${services_registry_source}
${services_start_source}
${services_sandboxes_source}
${compose_lifecycle_source}
${machine_routes_source}
${machine_service_source}
${node_reconciler_source}"
  legacy_code="$(printf '%s\n' "${legacy_source}" | source_without_comments_or_strings)"
  legacy_hits="$(printf '%s\n' "${legacy_code}" |
    rg -n 'start_service_launch|ensure_service_binding_for_decision_async|trait[[:space:]]+SandboxBackend[^}]*fn[[:space:]]+start|\.start[[:space:]]*\(' |
    awk 'BEGIN { separator = "" } { printf "%s%s", separator, $0; separator = "; " }' || true)"
  if [ -n "${legacy_hits}" ]; then
    add_error "legacy-deletion-path-dependency-effect-contract: legacy provision authority remains: ${legacy_hits}"
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
