#!/usr/bin/env bash
# Mutation self-test for the NNC6.4 workload-provision dispatch contract.

write_nnc64_fixture() {
  fixture_root="$1"

  mkdir -p \
    "${fixture_root}/crates/nimbus-workloads/src/saga" \
    "${fixture_root}/crates/nimbus-compute/src/workload_saga/provision_dispatch" \
    "${fixture_root}/crates/nimbus-compute/src/workload_saga" \
    "${fixture_root}/crates/nimbus-compute/src" \
    "${fixture_root}/crates/nimbus-server/src" \
    "${fixture_root}/crates/nimbus-sandbox/src" \
    "${fixture_root}/crates/nimbus-services/src/manager" \
    "${fixture_root}/crates/nimbus-services/src" \
    "${fixture_root}/crates/nimbus-cli/src/compose" \
    "${fixture_root}/crates/nimbus-cli/src/machine/api" \
    "${fixture_root}/crates/nimbus-node/src" \
    "${fixture_root}/crates/nimbus-cloud-functions/src/http" \
    "${fixture_root}/crates/nimbus-cloud-functions/src" \
    "${fixture_root}/crates/nimbus-network/src" \
    "${fixture_root}/docs/private/plans/proof/nimbus-network-control-plane" \
    "${fixture_root}/docs/private/plans"

  cat >"${fixture_root}/${WORKLOAD_PROVISION}" <<'RUST'
pub struct WorkloadProvisionDispatchEpoch(u64);
pub struct WorkloadExecutionProviderId;

pub enum WorkloadProvisionProviderTarget {
    Network {
        role: NetworkCapabilityRole,
        provider_id: NetworkProviderId,
        provider_source_digest: NetworkCapabilitySourceDigest,
    },
    Execution {
        provider_id: WorkloadExecutionProviderId,
        provider_source_digest: WorkloadProvisionSourceDigest,
    },
}

pub struct WorkloadProvisionDispatchClaim {
    provider_target: WorkloadProvisionProviderTarget,
}

pub enum WorkloadProvisionDispatchAuthorization {
    Initial,
    RetryAfterAbsence,
}

pub enum WorkloadProvisionInspectionResult {
    Absent,
    Ambiguous,
    DefiniteFailure,
    InProgress,
    Succeeded,
}

pub struct WorkloadProvisionAbsenceEvidence {
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    step: WorkloadProvisionStep,
    evidence: WorkloadOwnerEvidenceDigest,
}

pub enum WorkloadProvisionDisposition {
    Ready,
    DispatchPending,
    InspectionRequired,
    DefiniteFailure,
}
RUST

  cat >"${fixture_root}/${WORKLOAD_STATE}" <<'RUST'
fn ready_to_initial_dispatch() {}
fn dispatch_to_inspection() {}
fn inspection_to_retry_dispatch() {}
fn dispatch_to_success() {}
fn dispatch_to_definite_failure() {}
RUST

  cat >"${fixture_root}/${WORKLOAD_STORE}" <<'RUST'
pub trait WorkloadSagaStore {}
RUST

  cat >"${fixture_root}/${COMPUTE_SAGA}" <<'RUST'
pub struct WorkloadSagaCoordinator;
RUST

  cat >"${fixture_root}/${COMPUTE_DECISION}" <<'RUST'
pub fn decide_workload_provision() {}
RUST

  cat >"${fixture_root}/${COMPUTE_DISPATCH}" <<'RUST'
pub enum WorkloadProvisionCommandMode {
    Execute,
    Inspect,
}

pub struct WorkloadProvisionCommandId(String);

pub struct ConfirmedWorkloadProvisionCommand {
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    attempt_id: WorkloadProvisionAttemptId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: NetworkPlanDigest,
    provider_target: WorkloadProvisionProviderTarget,
    step: WorkloadProvisionStep,
    subjects: WorkloadProvisionSubjects,
}

impl ConfirmedWorkloadProvisionCommand {
    fn from_confirmation() -> Self {
        unreachable!()
    }
}

pub struct WorkloadProvisionCommandResult {
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
}

pub enum WorkloadSagaConfirmation {
    AppliedByThisCall,
    ConfirmedAfterAmbiguity,
    ConfirmedReplay,
    Conflict,
    UnresolvedAmbiguity,
}

pub trait NetworkReservationCapability {}
pub trait WorkloadPreparationCapability {}
pub trait NetworkAttachmentCapability {}
pub trait WorkloadActivationCapability {}
pub trait WorkloadReadinessCapability {}
pub trait IngressPublicationCapability {}

pub enum WorkloadProvisionStep {
    ReserveNetwork,
    PrepareWorkload,
    AttachNetwork,
    InspectActivationPrerequisites,
    ActivateWorkload,
    InspectWorkloadReadiness,
    Publish,
    ObservePublication,
}

const COMMAND_ID_DOMAIN: &str = "nimbus.compute.workload.provision.command.id.v1";

fn reduce_command_result(_: WorkloadProvisionInspectionResult) {}
fn resolve_ambiguous_confirmation() {}
fn validate_current_source() {}
fn validate_current_provider_report() {}
fn select_exact_provider() {}
fn provider_target() {}
RUST

  cat >"${fixture_root}/${COMPUTE_DISPATCH_TESTS}" <<'RUST'
fn inspection_absence_authorizes_same_attempt_next_epoch() {}
fn absence_retry_increments_dispatch_epoch_exactly_once() {}
fn retry_without_absence_evidence_is_rejected() {}
fn every_phase_mode_and_command_result_is_exhaustive() {}
fn direct_cas_winner_executes_exact_attempt_once() {}
fn unconfirmed_candidate_cannot_form_provider_command() {}
fn confirmed_replay_inspects_without_execute() {}
fn ambiguous_cas_confirmation_inspects_without_execute() {}
fn unresolved_cas_ambiguity_emits_no_command() {}
fn one_fresh_read_after_ambiguous_command_cas() {}
fn ambiguous_successor_cas_reads_before_later_decision() {}
fn current_source_mismatch_rejects_before_attempt_cas() {}
fn provider_report_digest_mismatch_rejects_before_effect() {}
fn network_steps_bind_exact_selected_role_provider_and_digest() {}
fn prepare_and_activate_bind_execution_provider_without_network_role() {}
fn resource_free_network_steps_fabricate_no_provider_target() {}
fn reserve_command_mapping_is_exact() {}
fn prepare_command_mapping_is_exact() {}
fn attach_command_mapping_is_exact() {}
fn activation_prerequisite_command_mapping_is_exact() {}
fn activate_command_mapping_is_exact() {}
fn workload_readiness_command_mapping_is_exact() {}
fn prepare_attach_and_activate_cannot_publish() {}
fn publication_observation_command_mapping_is_exact() {}
fn withheld_and_prepare_only_emit_no_provider_command() {}
fn definite_failure_never_dispatches_a_later_step() {}
fn ambiguous_publication_inspects_before_retry() {}
fn inspection_in_progress_never_retries() {}
fn concurrent_dispatchers_create_one_provider_effect() {}
fn crash_after_dispatch_cas_before_effect_inspects() {}
fn crash_after_effect_before_result_cas_inspects() {}
fn fresh_process_reopens_engine_without_snapshot_handoff() {}
fn native_service_and_sandbox_callers_use_compute_dispatch() {}
fn convex_async_activation_uses_compute_dispatch() {}
fn compose_local_and_forwarded_use_compute_dispatch() {}
fn machine_api_and_guest_node_use_fenced_commands() {}
fn convex_sync_and_invocation_snapshots_are_read_only() {}
fn cloud_functions_snapshots_have_zero_activation_store_or_provider_calls() {}
RUST

  cat >"${fixture_root}/${COMPUTE_STATE}" <<'RUST'
struct ComputeState {
    provision_capabilities: WorkloadProvisionDispatcher,
    source_authority: SourceAuthority,
    saga_store: SagaStore,
    network_manager: NetworkManager,
}
RUST

  cat >"${fixture_root}/${SERVER_STATE}" <<'RUST'
pub struct ServerIngressPublicationAdapter;
struct ServerState {
    provision_capabilities: WorkloadProvisionDispatcher,
    source_authority: SourceAuthority,
    saga_store: SagaStore,
    network_manager: NetworkManager,
}
fn attempt_idempotency_journal() {}
fn claim_dispatch_epoch() {}
fn reject_stale_dispatch_epoch() {}
fn adopt_exact_attempt() {}
RUST

  cat >"${fixture_root}/${SANDBOX_BACKEND}" <<'RUST'
pub struct ContainerProvisionAdapter;
pub struct KrunProvisionAdapter;
pub struct ForwardedMachineProvisionAdapter;
fn attempt_idempotency_journal() {}
fn claim_dispatch_epoch() {}
fn reject_stale_dispatch_epoch() {}
fn adopt_exact_attempt() {}
RUST

  cat >"${fixture_root}/${SERVICES_REGISTRY}" <<'RUST'
fn use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${SERVICES_START}" <<'RUST'
fn use_confirmed_command() {}
RUST
  cat >"${fixture_root}/${SERVICES_SANDBOXES}" <<'RUST'
fn use_confirmed_command() {}
RUST
  cat >"${fixture_root}/${COMPOSE_LIFECYCLE}" <<'RUST'
fn use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${MACHINE_ROUTES}" <<'RUST'
fn use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${MACHINE_SERVICE}" <<'RUST'
fn use_confirmed_command() {}
RUST
  cat >"${fixture_root}/${NODE_RECONCILER}" <<'RUST'
fn reconcile_with_compute_dispatch() {}
RUST

  cat >"${fixture_root}/${CLOUD_FUNCTIONS_HOST}" <<'RUST'
fn snapshot_for_tenant() {}
RUST
  cat >"${fixture_root}/${CLOUD_FUNCTIONS_HTTP}" <<'RUST'
fn snapshot_for_tenant() {}
RUST
  cat >"${fixture_root}/${CLOUD_FUNCTIONS_TRIGGER}" <<'RUST'
fn snapshot_for_tenant() {}
RUST

  cat >"${fixture_root}/${NETWORK_MANIFEST}" <<'TOML'
[package]
name = "nimbus-network"

[dependencies]
nimbus-core.workspace = true
serde.workspace = true
TOML
  cat >"${fixture_root}/${NETWORK_SOURCE}" <<'RUST'
pub struct NetworkAttachmentId(String);
RUST

  cat >"${fixture_root}/${OWNER_PLAN}" <<'MARKDOWN'
# Nimbus network control-plane plan

NNC6.4 owns provider dispatch. Its contract closes with 40 checks and a self-test
proof of 48 passed mutations.
MARKDOWN
  cat >"${fixture_root}/${OWNER_PROOF}" <<'MARKDOWN'
# NNC6.4 provider dispatch proof

The provider dispatch contract reports 40 checks. Mutation testing reports
48 passed and 0 failed.
MARKDOWN
}

run_self_test() {
  fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-nnc64.XXXXXX")"
  trap 'rm -rf "${fixture_root}"' RETURN
  write_nnc64_fixture "${fixture_root}"

  baseline_output="$(
    NIMBUS_NETWORK_NNC64_ROOT="${fixture_root}" \
      NIMBUS_NETWORK_NNC64_TEST_PINNED_HISTORY=present \
      bash "${SCRIPT_PATH}" --check 2>&1
  )"
  baseline_status=$?
  if [ "${baseline_status}" -ne 0 ] ||
    [ "${baseline_output}" != 'NNC6.4 provider dispatch contract: 40 checks passed' ]; then
    printf 'NNC6.4 provider dispatch contract self-test baseline failed:\n%s\n' \
      "${baseline_output}" >&2
    return 1
  fi

  mutation_cases=(
    'missing-command-vocabulary|confirmed-command-closed-vocabulary'
    'extra-command-mode|confirmed-command-closed-vocabulary'
    'forgeable-command-constructor|confirmed-command-private-construction'
    'missing-confirmed-transition-id|confirmed-command-revision-transition-fence'
    'missing-confirmed-revision|confirmed-command-revision-transition-fence'
    'missing-attempt-id|confirmed-command-record-identity'
    'missing-generation|confirmed-command-generation-digest-fence'
    'missing-desired-digest|confirmed-command-generation-digest-fence'
    'missing-provider-id|confirmed-command-provider-subject-fence'
    'missing-provider-source-digest|confirmed-command-provider-subject-fence'
    'missing-subject-fence|confirmed-command-provider-subject-fence'
    'missing-command-id-domain|confirmed-command-domain-separated-id'
    'missing-dispatch-epoch|dispatch-epoch-and-authorization'
    'result-loses-command-fence|effect-result-command-correlation'
    'unknown-inspection-result|inspection-result-closed-vocabulary'
    'missing-inspection-absent|inspection-result-closed-vocabulary'
    'missing-inspection-in-progress|inspection-result-closed-vocabulary'
    'retry-changes-attempt-id|same-attempt-monotonic-retry'
    'retry-reuses-dispatch-epoch|same-attempt-monotonic-retry'
    'retry-lacks-absence-evidence|same-attempt-monotonic-retry'
    'fixed-revision-offset-retry|explicit-disposition-transition-graph'
    'ambiguous-cas-executes|direct-winner-only-execute'
    'unchanged-cas-executes|direct-winner-only-execute'
    'execute-before-attempt-cas|direct-winner-only-execute'
    'source-mismatch-effects|current-source-before-dispatch'
    'provider-report-mismatch-effects|current-provider-report-before-dispatch'
    'missing-reserve-command|reserve-command-mapping'
    'missing-prepare-command|prepare-command-mapping'
    'missing-attach-command|attach-command-mapping'
    'missing-prerequisite-inspection|activation-prerequisite-command-mapping'
    'missing-activate-command|activate-command-mapping'
    'missing-readiness-inspection|workload-readiness-command-mapping'
    'publish-before-ready|publish-observe-and-nonpublish-mapping'
    'missing-publication-observation|publish-observe-and-nonpublish-mapping'
    'definite-failure-emits-later-command|definite-failure-and-ambiguity-behavior'
    'ambiguous-retries-without-inspection|definite-failure-and-ambiguity-behavior'
    'in-progress-retries|definite-failure-and-ambiguity-behavior'
    'concurrent-provider-duplicate|crash-concurrency-and-fresh-process-proof'
    'missing-effect-crash-cut|crash-concurrency-and-fresh-process-proof'
    'fresh-process-snapshot-handoff|crash-concurrency-and-fresh-process-proof'
    'duplicate-coordinator|single-store-single-coordinator'
    'duplicate-store|single-store-single-coordinator'
    'god-provider-trait|small-real-capability-seams'
    'network-effect-interface|legacy-deletion-path-dependency-effect-contract'
    'portable-provider-handle|portable-disposition-retry-state'
    'old-provision-authority-remains|legacy-deletion-path-dependency-effect-contract'
    'caller-family-bypass|positive-and-read-only-caller-census'
    'cloud-functions-effect|positive-and-read-only-caller-census'
  )

  if [ "${#mutation_cases[@]}" -ne 48 ]; then
    printf 'NNC6.4 provider dispatch contract self-test expected 48 mutations, observed %d\n' \
      "${#mutation_cases[@]}" >&2
    return 1
  fi

  passed=0
  failed=0
  for mutation_case in "${mutation_cases[@]}"; do
    mutation="${mutation_case%%|*}"
    expected_label="${mutation_case#*|}"
    mutation_output="$(
      NIMBUS_NETWORK_NNC64_ROOT="${fixture_root}" \
        NIMBUS_NETWORK_NNC64_TEST_PINNED_HISTORY=present \
        NIMBUS_NETWORK_NNC64_TEST_MUTATION="${mutation}" \
        bash "${SCRIPT_PATH}" --check 2>&1
    )"
    mutation_status=$?

    if [ "${mutation_status}" -eq 0 ]; then
      printf 'mutation %s was not rejected\n' "${mutation}" >&2
      failed=$((failed + 1))
    elif printf '%s\n' "${mutation_output}" |
      rg -q -F "mutation ${mutation} did not change its fixture"; then
      printf 'mutation %s was a no-op:\n%s\n' "${mutation}" "${mutation_output}" >&2
      failed=$((failed + 1))
    elif ! printf '%s\n' "${mutation_output}" |
      rg -q -F "NNC6.4 provider dispatch contract failure: ${expected_label}:"; then
      printf 'mutation %s missed %s:\n%s\n' \
        "${mutation}" "${expected_label}" "${mutation_output}" >&2
      failed=$((failed + 1))
    else
      passed=$((passed + 1))
    fi
  done

  if [ "${passed}" -ne 48 ] || [ "${failed}" -ne 0 ]; then
    printf 'NNC6.4 provider dispatch contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failed}" >&2
    return 1
  fi

  printf 'NNC6.4 provider dispatch contract self-test: 48 passed, 0 failed\n'
}
