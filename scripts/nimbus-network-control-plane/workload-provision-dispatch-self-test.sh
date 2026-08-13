#!/usr/bin/env bash
# Mutation self-test for the NNC6.4 workload-provision dispatch contract.

write_nnc64_fixture() {
  fixture_root="$1"

  mkdir -p \
    "${fixture_root}/crates/nimbus-workloads/src/saga" \
    "${fixture_root}/crates/nimbus-workloads/src/saga/provision/tests" \
    "${fixture_root}/crates/nimbus-workloads/src/saga/tests" \
    "${fixture_root}/crates/nimbus-compute/src/workload_saga/provision_dispatch" \
    "${fixture_root}/crates/nimbus-compute/src/workload_saga/provision_driver" \
    "${fixture_root}/crates/nimbus-compute/src/workload_saga" \
    "${fixture_root}/crates/nimbus-compute/src/resource_provision" \
    "${fixture_root}/crates/nimbus-compute/src" \
    "${fixture_root}/crates/nimbus-server/src/adapters/convex/execution/runtime_backed/invoke/context" \
    "${fixture_root}/crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls" \
    "${fixture_root}/crates/nimbus-server/src/workload_saga_store/tests" \
    "${fixture_root}/crates/nimbus-server/src" \
    "${fixture_root}/crates/nimbus-machine/src" \
    "${fixture_root}/crates/nimbus-sandbox/src" \
    "${fixture_root}/crates/nimbus-sandbox/src/backends/container/runtime" \
    "${fixture_root}/crates/nimbus-sandbox/src/backends/krun" \
    "${fixture_root}/crates/nimbus-services/src/manager" \
    "${fixture_root}/crates/nimbus-services/src" \
    "${fixture_root}/crates/nimbus-cli/src/compose/tests" \
    "${fixture_root}/crates/nimbus-cli/src/machine/stub" \
    "${fixture_root}/crates/nimbus-cli/src/machine/api/tests" \
    "${fixture_root}/crates/nimbus-cli/src/machine/backend/provision" \
    "${fixture_root}/crates/nimbus-node/src" \
    "${fixture_root}/crates/nimbus-cloud-functions/src/http" \
    "${fixture_root}/crates/nimbus-cloud-functions/src" \
    "${fixture_root}/crates/nimbus-cloud-functions/tests" \
    "${fixture_root}/crates/nimbus-network/src" \
    "${fixture_root}/docs/private/plans/proof/nimbus-network-control-plane" \
    "${fixture_root}/docs/private/plans"

  cat >"${fixture_root}/${WORKLOAD_PROVISION}" <<'RUST'
pub enum WorkloadProvisionCommandMode {
    Execute,
    Inspect,
}

pub struct WorkloadProvisionCommandId(String);
const COMMAND_ID_DOMAIN: &str = "nimbus.compute.workload.provision.command.id.v1";

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

fn retry_authorization_is_exact() {
    let _ = absence.confirmed_revision.checked_next() != Some(self.claimed_revision);
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
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
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

  cat >"${fixture_root}/${WORKLOAD_PROVISION_DISPATCH_TESTS}" <<'RUST'
fn dispatch_epoch_and_inspection_wire_reject_unknown_noncanonical_values() {}
fn retry_authorization_wire_rejects_crossed_absence_revision() {}
RUST

  cat >"${fixture_root}/${WORKLOAD_STATE}" <<'RUST'
fn ready_to_initial_dispatch() {}
fn dispatch_to_inspection() {}
fn inspection_to_retry_dispatch() {}
fn dispatch_to_success() {}
fn dispatch_to_definite_failure() {}
RUST

  cat >"${fixture_root}/${WORKLOAD_STATE_PROVISION_TESTS}" <<'RUST'
fn retry_reusing_skipping_or_crossing_absence_transition_is_rejected() {}
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
pub struct ConfirmedWorkloadProvisionCommand {
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    attempt_id: WorkloadProvisionAttemptId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    network_plan_digest: NetworkPlanDigest,
    provider_target: WorkloadProvisionProviderTarget,
    step: WorkloadProvisionStep,
    subjects: WorkloadProvisionSubjects,
}

impl ConfirmedWorkloadProvisionCommand {
    fn from_confirmation() -> Self {
        unreachable!()
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source.source_digest()
    }
}

pub struct WorkloadProvisionCommandResult {
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
}

pub struct ConfirmedWorkloadProvisionTransition {
    confirmed_record: Option<WorkloadSagaRecord>,
}

async fn inspect_confirmed_provision() {
    let _ = self.store.load(key).await?;
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

fn reduce_command_result(_: WorkloadProvisionInspectionResult) {}
fn resolve_ambiguous_confirmation() {}
fn validate_current_source() {}
fn validate_current_provider_report() {}
fn select_exact_provider() {}
fn provider_target() {}
RUST

  cat >"${fixture_root}/${COMPUTE_DRIVER}" <<'RUST'
fn drive_confirmed_provision() {}
RUST

  cat >"${fixture_root}/${COMPUTE_DRIVER_TESTS}" <<'RUST'
fn driver_is_bounded() {}
RUST

  cat >"${fixture_root}/${COMPUTE_DISPATCH_TESTS}" <<'RUST'
fn inspection_absence_authorizes_same_attempt_next_epoch() {}
fn absence_retry_increments_dispatch_epoch_exactly_once() {}
fn retry_without_absence_evidence_is_rejected() {}
fn every_phase_mode_and_command_result_is_exhaustive() {}
fn direct_cas_winner_executes_exact_attempt_once() {}
fn unconfirmed_candidate_cannot_form_provider_command() {}
fn unconfirmed_recovery_candidate_cannot_form_provider_command() {}
fn conflict_exposes_no_candidate_record_or_command() {}
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
fn native_service_and_sandbox_callers_use_compute_dispatch() {}
fn convex_async_activation_uses_compute_dispatch() {}
fn compose_local_and_forwarded_use_compute_dispatch() {}
fn machine_api_and_guest_node_use_fenced_commands() {}
RUST

  cat >"${fixture_root}/${SERVER_CONVEX_CONTEXT_READ_ONLY_TESTS}" <<'RUST'
fn convex_sync_and_invocation_snapshots_are_read_only_for_invocation_snapshot() {}
RUST

  cat >"${fixture_root}/${SERVER_CONVEX_LOOKUP_READ_ONLY_TESTS}" <<'RUST'
fn convex_sync_and_invocation_snapshots_are_read_only_for_sync_present_and_missing_lookups() {}
fn convex_async_activation_uses_compute_dispatch() {}
RUST

  cat >"${fixture_root}/${CLOUD_FUNCTIONS_READ_ONLY_TESTS}" <<'RUST'
fn cloud_functions_snapshots_have_zero_activation_store_or_provider_calls_for_http_and_callable() {}
fn cloud_functions_snapshots_have_zero_activation_store_or_provider_calls_for_trigger_and_unknown_target() {}
RUST

  cat >"${fixture_root}/${SERVER_PROVISION_PROCESS_TESTS}" <<'RUST'
fn fresh_process_reopens_engine_without_snapshot_handoff() {}
RUST

  cat >"${fixture_root}/${COMPUTE_STATE}" <<'RUST'
struct ComputeState {
    workload_provisioner: WorkloadProvisioner,
    provision_capabilities: CapabilityRegistry,
    source_authority: SourceAuthority,
    lifecycle_stores: WorkloadLifecycleStores,
    network_manager: NetworkManager,
}
RUST

  cat >"${fixture_root}/${COMPUTE_RESOURCE_PROVISION}" <<'RUST'
fn provision_with_source_reservation() {}
RUST
  cat >"${fixture_root}/${COMPUTE_SANDBOXES}" <<'RUST'
fn create_sandbox() { provision_standalone_sandbox(); }
RUST
  cat >"${fixture_root}/${COMPUTE_SERVICES}" <<'RUST'
fn start_service() { provision_sandbox_service(); }
RUST
  cat >"${fixture_root}/${COMPUTE_RESOURCE_PROVISION_TESTS}" <<'RUST'
fn native_service_and_sandbox_callers_use_compute_dispatch() {}
RUST

  cat >"${fixture_root}/${SERVER_STATE}" <<'RUST'
pub struct ServerIngressPublicationAdapter;
struct ServerState {
    workload_provisioner: WorkloadProvisioner,
    provision_capabilities: CapabilityRegistry,
    source_authority: SourceAuthority,
    lifecycle_stores: WorkloadLifecycleStores,
    network_manager: NetworkManager,
}
fn attempt_idempotency_journal() {}
fn claim_dispatch_epoch() {}
fn reject_stale_dispatch_epoch() {}
fn adopt_exact_attempt() {}
RUST

  cat >"${fixture_root}/${SERVER_CONVEX_ASYNC}" <<'RUST'
fn invoke_ctx_service_lookup_async_cancellable() {
    let _ = WorkloadProvisionCancellation;
    provision_sandbox_service();
}
RUST

  cat >"${fixture_root}/${SANDBOX_BACKEND}" <<'RUST'
fn attempt_idempotency_journal() {}
fn claim_dispatch_epoch() {}
fn reject_stale_dispatch_epoch() {}
fn adopt_exact_attempt() {}
RUST


  cat >"${fixture_root}/${COMPUTE_SANDBOX_PROVIDER}" <<'RUST'
pub struct ContainerProvisionAdapter;
pub struct KrunProvisionAdapter;
pub struct ForwardedMachineProvisionAdapter;
impl ContainerProvisionAdapter {
    pub fn new(backend: ContainerSandboxBackend) {
        let journal = backend.attempt_idempotency_journal()?;
        let phases = ProviderProvisionPhaseAdapter::new(journal.clone());
    }
}
impl KrunProvisionAdapter {
    pub fn new(backend: KrunSandboxBackend) {
        let journal = backend.attempt_idempotency_journal()?;
        let phases = ProviderProvisionPhaseAdapter::new(journal.clone());
    }
}
RUST

  cat >"${fixture_root}/${COMPUTE_PROVISION_PROVIDER}" <<'RUST'
fn attempt_idempotency_journal() {}
fn claim_dispatch_epoch() {}
fn record_observation() {}
RUST

  cat >"${fixture_root}/${SANDBOX_CONTAINER_PROVIDER}" <<'RUST'
struct ContainerSandboxBackend;
RUST
  cat >"${fixture_root}/${SANDBOX_CONTAINER_PROVIDER_JOURNAL}" <<'RUST'
impl ContainerSandboxBackend {
    pub fn attempt_idempotency_journal(&self) {
        ProviderCommandAttemptJournal::open(
            &self.config.workload_state_root,
            "container-runtime",
        )
    }
}
RUST
  cat >"${fixture_root}/${SANDBOX_KRUN_PROVIDER}" <<'RUST'
struct KrunSandboxBackend;
impl KrunSandboxBackend {
    pub fn attempt_idempotency_journal(&self) {
        ProviderCommandAttemptJournal::open(
            &self.config.workload_state_root,
            "krun-runtime",
        )
    }
}
RUST

  cat >"${fixture_root}/${SANDBOX_PROVISION}" <<'RUST'
fn exact_sandbox_provision_plan() {}
RUST
  cat >"${fixture_root}/${SANDBOX_PROVIDER_COMMAND}" <<'RUST'
pub struct ProviderCommandAttemptJournal;
impl ProviderCommandAttemptJournal {
    fn claim_dispatch_epoch() {}
    fn reject_stale_dispatch_epoch() {}
    fn adopt_exact_attempt() {}
}
RUST

  cat >"${fixture_root}/${SERVICES_REGISTRY}" <<'RUST'
fn use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${SERVICES_MANAGER}" <<'RUST'
fn source_and_projection_only() {}
RUST
  cat >"${fixture_root}/${SERVICES_ACTIVATION}" <<'RUST'
fn no_activation_authority() {}
RUST
  cat >"${fixture_root}/${SERVICES_START}" <<'RUST'
fn use_confirmed_command() {}
RUST
  cat >"${fixture_root}/${SERVICES_SANDBOXES}" <<'RUST'
fn use_confirmed_command() {}
RUST
  cat >"${fixture_root}/${COMPOSE_LIFECYCLE}" <<'RUST'
fn use_compute_dispatch() { resource_provisioner(); let _ = EngineWorkloadSagaStore; }
RUST
  cat >"${fixture_root}/${COMPOSE_EXECUTION}" <<'RUST'
fn exact_compose_provider_realm() {}
RUST
  cat >"${fixture_root}/${COMPOSE_LIFECYCLE_TESTS}" <<'RUST'
fn compose_local_and_forwarded_provision_use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${COMPOSE_FORWARDED_TESTS}" <<'RUST'
fn forwarded_compose_uses_exact_provider() {}
RUST
  cat >"${fixture_root}/${NIMBUS_MACHINE_API}" <<'RUST'
const MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH: &str = "/v1/workloads/provision/phase";
struct MachineApiWorkloadProvisionCommandEnvelope;
RUST
  cat >"${fixture_root}/${MACHINE_ROUTES}" <<'RUST'
fn use_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${MACHINE_SERVICE}" <<'RUST'
fn use_confirmed_command(_: MachineApiWorkloadProvisionCommandEnvelope) {}
RUST
  cat >"${fixture_root}/${MACHINE_CLIENT}" <<'RUST'
fn provision_workload_phase() {}
RUST
  cat >"${fixture_root}/${MACHINE_STUB_CLIENT}" <<'RUST'
fn provision_workload_phase() {}
RUST
  cat >"${fixture_root}/${MACHINE_BACKEND}" <<'RUST'
fn confirmed_machine_publication() {}
RUST
  cat >"${fixture_root}/${MACHINE_BACKEND_PROVISION}" <<'RUST'
pub struct ForwardedMachineProvisionAdapter;
RUST
  cat >"${fixture_root}/${MACHINE_PROVISION_ROUTE_TESTS}" <<'RUST'
fn machine_api_and_guest_node_use_fenced_commands() {}
RUST
  cat >"${fixture_root}/${MACHINE_PROVISION_ADAPTER_TESTS}" <<'RUST'
fn machine_api_and_guest_use_exact_compute_phase_dispatch() {}
fn real_registry_substitution_publishes_and_observes_exact_forwarded_command() {}
RUST
  cat >"${fixture_root}/${MACHINE_PUBLICATION}" <<'RUST'
fn legacy_service_intent_cannot_represent_canonical_command_identity() {}
RUST
  cat >"${fixture_root}/${MACHINE_CAPABILITIES}" <<'RUST'
fn exact_phase_capability() {}
RUST
  cat >"${fixture_root}/${NODE_RECONCILER}" <<'RUST'
fn reconcile_with_compute_dispatch() {}
RUST
  cat >"${fixture_root}/${NODE_HOST_LIFECYCLE}" <<'RUST'
pub trait HostLifecycleBackend {
    fn activate_exact(&self);
}
RUST
  cat >"${fixture_root}/${NODE_DIRECT_PROCESS}" <<'RUST'
fn activate_exact() {}
RUST
  cat >"${fixture_root}/${NODE_SYSTEMD_TRANSIENT}" <<'RUST'
fn activate_exact() {}
RUST
  cat >"${fixture_root}/${NODE_EXECUTOR}" <<'RUST'
fn removed_hidden_executor() {}
RUST
  cat >"${fixture_root}/${CLI_LIB}" <<'RUST'
fn no_hidden_executor_command() {}
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
proof of 50 passed mutations.
MARKDOWN
  cat >"${fixture_root}/${OWNER_PROOF}" <<'MARKDOWN'
# NNC6.4 provider dispatch proof

The provider dispatch contract reports 40 checks. Mutation testing reports
50 passed and 0 failed.
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
    'retry-crosses-absence-revision|same-attempt-monotonic-retry'
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
    'random-parent-attempt-id|legacy-deletion-path-dependency-effect-contract'
    'missing-forwarded-command-proof|positive-and-read-only-caller-census'
    'cloud-functions-effect|positive-and-read-only-caller-census'
    'missing-container-provider-connector|provider-local-attempt-idempotency'
    'missing-krun-provider-connector|provider-local-attempt-idempotency'
  )

  if [ "${#mutation_cases[@]}" -ne 50 ]; then
    printf 'NNC6.4 provider dispatch contract self-test expected 50 mutations, observed %d\n' \
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

  if [ "${passed}" -ne 50 ] || [ "${failed}" -ne 0 ]; then
    printf 'NNC6.4 provider dispatch contract self-test: %d passed, %d failed\n' \
      "${passed}" "${failed}" >&2
    return 1
  fi

  printf 'NNC6.4 provider dispatch contract self-test: 50 passed, 0 failed\n'
}
