import {
  maskNonCode,
  withoutCfgTestItems,
} from "./source-contract-scanner.mjs";

export const COMPOSE_DOWN_TESTS = [
  "compose_down_local_uses_engine_saga_and_compute_teardown",
  "compose_down_forwarded_uses_engine_saga_and_exact_machine_phases",
  "compose_down_unresolved_submission_makes_zero_provider_calls",
  "compose_down_replay_is_idempotent_and_reports_durable_outcome",
  "compose_down_crossed_or_stale_identity_fails_before_provider_effects",
  "compose_down_ambiguous_result_reopens_with_inspection_only",
  "compose_down_process_reopen_resumes_same_attempt_without_duplicate_effect",
  "compose_down_partial_sibling_failure_preserves_completed_and_unissued_services",
  "compose_down_cancellation_after_submission_is_replayable",
];

export const PHYSICAL_MACHINE_STOP_TESTS = [
  "machine_stop_rejects_active_workload_saga_authority",
  "standalone_machine_stop_fails_closed_without_engine_drain_authority",
  "machine_stop_exact_empty_fence_precedes_publication_and_vmm_effects",
  "machine_stop_active_authority_makes_zero_publication_ssh_vmm_or_state_effects",
  "machine_stop_stale_or_crossed_machine_generation_makes_zero_effects",
  "machine_stop_ambiguous_unavailable_or_corrupt_authority_fails_closed",
  "machine_stop_reopen_rediscovers_active_durable_authority",
  "machine_stop_and_concurrent_admission_linearize_at_one_fence",
  "machine_workload_desire_commit_holds_admission_guard_through_engine_cas",
  "machine_stop_barrier_waits_for_inflight_engine_desire_commit",
  "machine_restart_cannot_bypass_active_workload_fence",
  "machine_os_restart_cannot_bypass_active_workload_fence",
  "stopped_machine_with_active_durable_authority_returns_typed_conflict",
  "machine_stop_ignores_observed_projection_and_address_identity",
];

export const BEHAVIOR_TESTS = [
  "service_stop_persists_then_observes_complete_teardown_order",
  "sandbox_stop_persists_then_observes_complete_teardown_order",
  "force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects",
  "definition_delete_keeps_source_and_sessions_until_recorded_teardown",
  "definition_delete_fences_and_joins_inflight_provision_before_removing_source",
  "late_provision_result_after_force_delete_is_retired_before_definition_removal",
  ...COMPOSE_DOWN_TESTS,
  "parent_publication_withdraws_before_guest_stop_and_releases_after_exact_absence",
  "crossed_machine_teardown_fences_fail_before_effects",
  ...PHYSICAL_MACHINE_STOP_TESTS,
  "final_ingress_withdrawal_cancels_joins_closes_and_settles_exact_leases",
  "ingress_settlement_failure_retains_cleanup_and_blocks_recorded",
  "tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete",
  "failed_service_start_enters_durable_compensation_without_caller_stop",
  "failed_sandbox_start_enters_durable_compensation_without_caller_stop",
  "restart_result_is_settled_before_withdrawal_committed",
];

export const NATIVE_SOURCE_RETIREMENT_TESTS = [
  "service_stop_persists_then_observes_complete_teardown_order",
  "sandbox_stop_persists_then_observes_complete_teardown_order",
  "native_stop_without_teardown_composition_fails_before_source_or_effect",
  "native_stop_unresolved_submission_makes_zero_provider_calls",
  "service_stop_joins_inflight_provision_and_retires_late_success",
  "sandbox_stop_joins_inflight_provision_and_retires_late_success",
  "definition_delete_keeps_source_and_sessions_until_recorded_teardown",
  "definition_delete_fences_and_joins_inflight_provision_before_removing_source",
  "force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects",
  "late_provision_result_after_force_delete_is_retired_before_definition_removal",
  "definition_delete_cleanup_pending_keeps_definition_observation_and_sessions",
  "definition_delete_cancellation_after_submission_is_replayable",
  "service_start_after_recorded_stop_uses_next_lifecycle_generation",
  "sandbox_start_after_recorded_stop_uses_next_lifecycle_generation",
  "source_generation_remains_stable_across_stop_and_later_start",
  "session_binding_rejects_a_later_execution_with_the_same_source_generation",
  "concurrent_start_and_stop_linearize_at_the_source_fence",
  "active_restart_settles_before_withdrawal_committed",
  "generation_overflow_fails_before_source_store_or_provider_effect",
  "missing_saga_with_provider_observation_fails_closed",
  "service_stop_fences_start_before_its_first_saga_commit",
  "sandbox_stop_fences_start_before_its_first_saga_commit",
  "definition_delete_fences_start_before_its_first_saga_commit",
];

export const FORWARDED_MACHINE_TESTS = {
  registry: [
    "real_forwarded_teardown_registry_runs_all_five_phases_through_compute_cas",
    "real_forwarded_teardown_registry_inspects_all_five_phases_without_fallback",
  ],
  lifecycle: [
    "forwarded_parent_sibling_matrix_retains_complete_batch_until_exact_absence",
    "forwarded_parent_release_requires_exact_guest_and_provider_absence",
    "forwarded_zero_listener_teardown_runs_all_five_phases_without_synthetic_port",
    "dead_pre_checkpoint_provider_batches_retain_and_unadopted_release_replays",
  ],
  recovery: [
    "forwarded_parent_response_loss_recovers_with_exact_inspect_before_retry",
    "forwarded_two_realm_fresh_process_matrix_recovers_every_frozen_cut",
    "inspected_absence_invalidates_a_delayed_started_token_before_io",
  ],
};

const testSource = (name) =>
  maskNonCode(`
#[test]
fn ${name}() {
    let observed = teardown_trace();
    let expected = expected_teardown_trace();
    assert_eq!(observed, expected);
}
`);

export function greenTeardownFixture() {
  const testEntries = BEHAVIOR_TESTS.map((name) => ({
    file: `__fixture__/${name}.rs`,
    source: testSource(name),
  }));
  const nativeTestsByFile = new Map([
    [
      "crates/nimbus-compute/src/resource_retirement/tests/native.rs",
      NATIVE_SOURCE_RETIREMENT_TESTS.filter(
        (name) =>
          ![
            "definition_delete_keeps_source_and_sessions_until_recorded_teardown",
            "definition_delete_fences_and_joins_inflight_provision_before_removing_source",
            "force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects",
            "late_provision_result_after_force_delete_is_retired_before_definition_removal",
            "definition_delete_cleanup_pending_keeps_definition_observation_and_sessions",
            "definition_delete_cancellation_after_submission_is_replayable",
            "session_binding_rejects_a_later_execution_with_the_same_source_generation",
            "concurrent_start_and_stop_linearize_at_the_source_fence",
          ].includes(name),
      ),
    ],
    [
      "crates/nimbus-server/src/tests/service_manager/definition_retirement.rs",
      NATIVE_SOURCE_RETIREMENT_TESTS.slice(6, 12),
    ],
    [
      "crates/nimbus-services/src/manager/tests/sessions.rs",
      [
        "session_binding_rejects_a_later_execution_with_the_same_source_generation",
      ],
    ],
    [
      "crates/nimbus-compute/src/workload_provisioner/tests.rs",
      ["concurrent_start_and_stop_linearize_at_the_source_fence"],
    ],
  ]);
  for (const [file, names] of nativeTestsByFile) {
    testEntries.push({
      file,
      source: names.map(testSource).join("\n"),
    });
  }
  testEntries.push({
    file: "crates/nimbus-compute/src/workload_saga/teardown_driver/tests.rs",
    source: testSource("teardown_driver_records_exact_five_step_order"),
  });
  const forwardedTestEntries = new Map();
  for (const names of Object.values(FORWARDED_MACHINE_TESTS)) {
    for (const name of names) {
      const file =
        name ===
        "forwarded_two_realm_fresh_process_matrix_recovers_every_frozen_cut"
          ? "crates/nimbus-cli/src/machine/backend/provision/tests/teardown_substitution/process_recovery.rs"
          : name ===
              "dead_pre_checkpoint_provider_batches_retain_and_unadopted_release_replays"
            ? "crates/nimbus-network/src/port_lease/lifetime/batch_reservation/tests.rs"
            : name ===
                "inspected_absence_invalidates_a_delayed_started_token_before_io"
              ? "crates/nimbus-sandbox/src/provider_command/tests/async_current_claim.rs"
              : "crates/nimbus-cli/src/machine/backend/provision/tests/teardown_substitution.rs";
      const tests = forwardedTestEntries.get(file) ?? [];
      tests.push(testSource(name));
      forwardedTestEntries.set(file, tests);
    }
  }
  for (const [file, tests] of forwardedTestEntries) {
    testEntries.push({ file, source: tests.join("\n") });
  }
  return {
    workloads: withoutCfgTestItems(`
pub enum WorkloadSagaPhase {
    WithdrawalCommitted,
    Withdrawn,
    Drained,
    WorkloadStopped,
    NetworkDetached,
    NetworkReleased,
    Recorded,
}
pub struct WorkloadEffectReferences;
pub enum WorkloadTerminalObservation {
    PublicationAbsent,
    ExecutionDrained,
    ExecutionStopped,
    NetworkDetached,
    NetworkReleased,
}
pub enum WorkloadTeardownCause { StoppedSuccessor, FailedProvision }
pub enum WorkloadTeardownStep {
    WithdrawPublication,
    DrainExecution,
    StopExecution,
    DetachNetwork,
    ReleaseNetwork,
}
pub struct WorkloadTeardownSubjects;
pub struct WorkloadTeardownAttemptId(String);
pub struct WorkloadTeardownDispatchEpoch(u64);
pub struct WorkloadTeardownClaim;
pub struct WorkloadTeardownResult;
pub enum WorkloadTeardownDisposition {
    DispatchPending,
    InspectionRequired,
    DefiniteFailure,
    Succeeded,
}
fn decide_teardown() {
    WorkloadTeardownDecision::RestartSettlementPending;
    WorkloadSagaPhase::NetworkReleased; ProposedWorkloadTeardownTransition::RecordTerminal;
}
fn teardown_step_for_phase() {
    WorkloadSagaPhase::WithdrawalCommitted; WorkloadTeardownStep::WithdrawPublication;
    WorkloadSagaPhase::Withdrawn; WorkloadTeardownStep::DrainExecution;
    WorkloadSagaPhase::Drained; WorkloadTeardownStep::StopExecution;
    WorkloadSagaPhase::WorkloadStopped; WorkloadTeardownStep::DetachNetwork;
    WorkloadSagaPhase::NetworkDetached; WorkloadTeardownStep::ReleaseNetwork;
}
`),
    compute: withoutCfgTestItems(`
fn materialize_teardown_candidate() {
    claim_teardown();
    record_resource_free_teardown_step();
    record_terminal_teardown();
}
struct ConfirmedWorkloadTeardownCommand {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    mode: WorkloadTeardownCommandMode,
    claim: WorkloadTeardownClaim,
}
impl ConfirmedWorkloadTeardownCommand {
    fn from_confirmation() {
        WorkloadSagaConfirmation::AppliedByThisCall;
        WorkloadTeardownCommandMode::Execute;
    }
    fn attempt_id() -> WorkloadTeardownAttemptId {}
    fn dispatch_epoch() -> WorkloadTeardownDispatchEpoch {}
    fn provider_target() -> WorkloadTeardownProviderTarget {}
    fn subjects() -> WorkloadTeardownSubjects {}
}
fn apply_teardown_result() {
    authenticate_confirmed_record();
    authenticate_command_result();
    apply_teardown_effect_result();
    apply_teardown_inspection_result();
}
fn confirm_teardown_transition() {
    confirm_transition();
    WorkloadSagaConfirmation::AppliedByThisCall;
}
impl WorkloadTeardownDriver {
    async fn drive() {
        decide_teardown();
        materialize_teardown_candidate();
        confirm_teardown_transition();
    }
}
fn submit_service_teardown() {}
fn submit_sandbox_teardown() {}
fn compose_native_teardown_runtime() { WorkloadTeardownRuntime; }
fn fence_and_join_inflight_provision() {}
fn retire_late_provision_result() {}
fn settle_issued_restart_before_native_teardown() {}
fn submit_compose_teardown() {}
fn wait_for_teardown_outcome() {}
fn list_tenant_sagas() {}
fn drive_tenant_teardown() {}
fn require_all_recorded_before_finish_tenant_delete() {}
fn compensate_definite_provision_failure() { WorkloadTeardownCause::FailedProvision; }
fn inspect_ambiguous_provision_before_compensation() {}
fn retain_cancellation_after_submission() {}
fn settle_issued_restart_before_teardown() {}
fn retain_late_restart_result() {}
fn enter_withdrawal_committed_after_restart_settlement() {}
`),
    services: withoutCfgTestItems(`
fn claim_service_definition_retirement() {}
fn finalize_service_definition_after_recorded() {}
fn project_recorded_service_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}
fn project_recorded_sandbox_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}
`),
    nativeCallers: withoutCfgTestItems(`
fn submit_service_teardown() {}
fn submit_sandbox_teardown() {}
fn claim_service_definition_retirement() {}
fn finalize_service_definition_after_recorded() {}
`),
    nativeSourceRetirement: withoutCfgTestItems(`
fn finalize_unstarted_source_stop() {}
fn finalize_unstarted_service_definition_deletion() {}
fn finalize_service_definition_after_recorded() {}
`),
    provisionSettlementTests: maskNonCode(`
#[test]
fn service_stop_joins_inflight_provision_and_retires_late_success() {
    let service_source_claim = install_source_claim_signal();
    wait_for_source_claim(&service_source_claim, "service");
    assert!(service_source_is_fenced());
}
#[test]
fn sandbox_stop_joins_inflight_provision_and_retires_late_success() {
    let sandbox_source_claim = install_source_claim_signal();
    wait_for_source_claim(&sandbox_source_claim, "sandbox");
    assert!(sandbox_source_is_fenced());
}
`),
    provisionSettlementSupport: maskNonCode(`
async fn wait_for_source_claim(&self, entered: &Semaphore, source: &str) {
    self.wait_for_signal(entered, source).await;
}
async fn wait_for_signal(&self, entered: &Semaphore, diagnostic: &str) {
    tokio::time::timeout(Duration::from_secs(2), entered.acquire())
        .await
        .expect(diagnostic)
        .expect("source-claim signal should remain open");
}
`),
    computeState: withoutCfgTestItems(`
pub enum ComputeWorkloadComposition {
    ProtocolOnly,
    Managed {
        execution_provider_id: WorkloadExecutionProviderId,
        teardown_capabilities: Option<Box<ExactWorkloadTeardownCapabilityRealm>>,
    },
}
impl ComputeState {
    pub fn from_config() {
        teardown_capabilities.map(|capabilities| {
            let capabilities = capabilities.into_registry_for(
                &capability_selection,
                &execution_provider_id,
            );
            WorkloadTeardownRuntime::new(capabilities)
        });
    }
}
`),
    serverComposition: withoutCfgTestItems(`
pub struct ServerWorkloadProviders {
    teardown_capabilities: Option<WorkloadTeardownCapabilityRegistry>,
}
impl ServerWorkloadProviders {
    pub fn new() -> Self { Self { teardown_capabilities: None } }
    pub fn with_teardown_capabilities(mut self, teardown_capabilities: WorkloadTeardownCapabilityRegistry) -> Self {
        self.teardown_capabilities = Some(teardown_capabilities);
        self
    }
}
struct ServerWorkloadComposition {
    execution_provider_id: WorkloadExecutionProviderId,
    teardown_capabilities: Option<ExactWorkloadTeardownCapabilityRealm>,
}
impl ServerWorkloadComposition {
    fn new() {
        let teardown_capabilities = ExactWorkloadTeardownCapabilityRealm::new(
            raw_teardown_capabilities,
        );
        Ok(Self { teardown_capabilities, execution_provider_id });
    }
    fn into_managed_compute() {
        ComputeWorkloadComposition::Managed {
            execution_provider_id: self.execution_provider_id,
            teardown_capabilities: self.teardown_capabilities.map(Box::new),
        };
    }
}
`),
    localComposition: withoutCfgTestItems(`
fn into_workload_composition() {
    let execution_teardown = KrunTeardownAdapter::new();
    let attachment_teardown = KrunAttachmentTeardownAdapter::new();
    let ingress_teardown = IngressTeardownCapabilities::new(ServerIngressPublicationAdapter);
    let teardown = WorkloadTeardownCapabilityRegistry::new(
        [attachment_teardown.capabilities()],
        [execution_teardown.capabilities()],
        [ingress_teardown],
    );
    ServerWorkloadProviders::new().with_teardown_capabilities(teardown);
}
`),
    httpServices: withoutCfgTestItems(`
async fn service_lifecycle_route() { service_lifecycle(&tenant_context); }
async fn delete_service_definition() { delete_service_definition(&tenant_context); }
`),
    httpSandboxes: withoutCfgTestItems(`
async fn stop_sandbox() { stop_sandbox(&authorization.tenant_context); }
`),
    computeServices: withoutCfgTestItems(`
async fn service_lifecycle(tenant_context: &TenantIsolationContext) { submit_service_teardown(tenant_context); }
async fn delete_service_definition(tenant_context: &TenantIsolationContext) { submit_definition_teardown(tenant_context); }
`),
    computeSandboxes: withoutCfgTestItems(`
async fn stop_sandbox(tenant_context: &TenantIsolationContext) { submit_sandbox_teardown(tenant_context); }
`),
    resourceRetirement: withoutCfgTestItems(`
fn compose_native_teardown_runtime() { WorkloadTeardownRuntime; }
fn fence_and_join_inflight_provision() {}
fn retire_late_provision_result() {}
fn settle_issued_restart_before_native_teardown() {}
`),
    serviceDefinitions: withoutCfgTestItems(`
fn claim_service_definition_retirement() {}
fn finalize_service_definition_after_recorded() {}
`),
    serviceProjections: withoutCfgTestItems(`
fn project_recorded_service_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}
fn project_recorded_sandbox_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}
`),
    server: withoutCfgTestItems(`
trait FinalIngressWithdrawalCapability {
    fn execute_exact_final_withdrawal();
    fn inspect_exact_final_withdrawal();
}
fn cancel_and_join_ingress_workers() {}
fn close_exact_ingress_routes() {}
fn settle_exact_listener_leases() {}
fn prove_exact_ingress_absence() {}
fn propagate_listener_settlement_failure() {}
`),
    composeCommand: withoutCfgTestItems(`
async fn run_compose_command(persistence_config: &EnginePersistenceConfig) {
    run_compose_down(command, persistence_config).await;
}
async fn run_compose_down(persistence_config: &EnginePersistenceConfig) {
    let engine = Engine::new_with_persistence_config(persistence_config.clone()).await;
    retire_compose_services(Arc::clone(&engine), prepared).await;
    engine.quiesce().await;
}
`),
    composeRetirement: withoutCfgTestItems(`
async fn retire_compose_services(engine: Arc<Engine>, prepared: PreparedComposeProvision) {
    let saga_store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let runtime = prepared.activate(Arc::clone(&engine), Arc::clone(&saga_store));
    let retirer = runtime.resource_retirer();
    let outcome = retirer.submit_service_teardown(tenant_context, service_name).await;
    match outcome.disposition() {
        WorkloadTeardownDisposition::Recorded => {
            let execution = outcome.terminal_execution_reference();
            return ComposeServiceRetirementOutcome::recorded(
                outcome.disposition(),
                execution,
            );
        }
        _ => return Err(ComposeRetirementIncomplete),
    }
}
`),
    forwardedCanonicalComposition: withoutCfgTestItems(`
fn prepare_forwarded_workload_profile(backend: &ForwardedMachineApiSandboxBackend) -> PreparedForwardedWorkloadProfile {
    let registrations = backend.teardown_capabilities();
    let teardown = WorkloadTeardownCapabilityRegistry::new(
        registrations.attachment,
        registrations.execution,
        registrations.ingress,
    );
    PreparedForwardedWorkloadProfile::new(
        ServerWorkloadProviders::new().with_teardown_capabilities(teardown),
    )
}
`),
    forwardedServerComposition: withoutCfgTestItems(`
fn compose_forwarded_server(backend: &ForwardedMachineApiSandboxBackend) -> PreparedForwardedWorkloadProfile {
    prepare_forwarded_workload_profile(backend)
}
`),
    forwardedComposeComposition: withoutCfgTestItems(`
fn compose_forwarded_foreground(backend: &ForwardedMachineApiSandboxBackend) -> PreparedForwardedWorkloadProfile {
    prepare_forwarded_workload_profile(backend)
}
`),
    exactGuestTeardown: withoutCfgTestItems(`
struct MachineApiWorkloadTeardownCommandEnvelope;
fn build_remote_request() {}
fn validate_source_and_target() {}
fn validate_subjects() {}
fn validate_retirement_order() {}
fn validate() {
    validate_source_and_target();
    validate_subjects();
    validate_retirement_order();
}
async fn dispatch() {
    let validated = self.validate();
    self.execute(validated).await;
}
async fn execute(validated: ValidatedForwardedMachineTeardown) {
    let remote_request = validated.remote_request.clone();
    claim_execute_started(validated);
    execute_started_claim_async(validated, || match remote_request {
        Some(request) => remote_result(&request),
        None => local_withdrawal_result(),
    });
}
fn remote_result(request: &MachineApiWorkloadTeardownPhaseRequest) {
    client.teardown_workload_phase_prepared(request);
}
fn settle_parent_release() {
    release_parent_batch_after_guest_release();
}
`),
    coarseGuestApi: withoutCfgTestItems(``),
    physicalDesireAdmissions: withoutCfgTestItems(`
async fn submit_intent() {
    with_machine_desire_admission_guard(|| async {
        self.commit_loaded(loaded.as_ref(), next.clone()).await
    }).await;
}
async fn compare_and_swap_restart_admission() {
    with_machine_desire_admission_guard(|| async {
        self.commit_loaded(Some(&current), candidate.clone()).await
    }).await;
}
`),
    physicalStopDecision: withoutCfgTestItems(`
pub enum MachineWorkloadStopDecision {
    EmptyWithFence(ConfirmedMachineStopAuthorization),
    ActiveWorkloadTeardownRequired,
    AuthorityUnavailable,
    Ambiguous,
    Corrupt,
    Stale,
    Crossed,
}
async fn authorize_machine_stop() {
    let fence = claim_machine_stop_admission_barrier();
    let after = list_machine_workload_authority_from_engine();
    let decision = classify_machine_stop_authority(fence, after);
    if decision.requires_active_conflict() {
        clear_unchanged_effect_free_machine_stop_barrier(fence);
    }
    decision
}
`),
    physicalStopDecisionStoreAdapter: withoutCfgTestItems(`
fn list_machine_authority_from_engine_adapter() {}
`),
    physicalStopProvider: withoutCfgTestItems(`
struct MachineStopAdmissionBarrier;
fn claim_machine_stop_admission_barrier() {
    self.mutate(|body| {
        authenticate_exact_machine_incarnation(body);
        persist_machine_stop_admission_barrier(body);
    });
}
async fn with_machine_desire_admission_guard(operation: impl AsyncOperation) {
    self.mutate_async(|body| async move {
        authenticate_no_machine_stop_barrier(body);
        let result = operation().await;
        settle_machine_desire_admission_guard(body, &result);
        result
    }).await
}
fn authenticate_forwarded_workload_admission_barrier() {
    self.mutate(|body| {
        authenticate_no_machine_stop_barrier(body);
    });
}
fn retain_ambiguous_machine_stop_barrier() {}
`),
    physicalProviderAdmissions: withoutCfgTestItems(`
fn execute_exact_phase() {
    authenticate_forwarded_workload_admission_barrier();
    claim_dispatch_epoch_started();
    self.phases.execute();
}
fn publish() {
    authenticate_forwarded_workload_admission_barrier();
    reserve_parent_batch();
    commit_before_machine_api();
    provision_workload_phase();
}
fn execute_restart_phase() {
    authenticate_forwarded_workload_admission_barrier();
    claim_restart_dispatch_started();
    self.restart_phases.execute();
}
`),
    physicalStopEffects: withoutCfgTestItems(`
pub(super) fn stop_machine(authorization: ConfirmedMachineStopAuthorization) {
    authorization.authenticate_exact_machine_generation();
    withdraw_machine_publications();
    withdraw_machine_ssh_port();
    stop_provider_machine();
    stop_pid();
    stop_exact_process();
    super::write_json_file();
}
`),
    physicalStopStandalone: withoutCfgTestItems(`
fn run_machine_stop() {
    let authorization = authorize_machine_stop();
    stop_machine_with_authorization(authorization);
}
`),
    physicalStopServer: withoutCfgTestItems(`
fn stop_machine<'a>() {
    let authorization = authorize_machine_stop();
    stop_machine_with_authorization(authorization);
}
fn restart_machine<'a>() {
    let authorization = authorize_machine_stop();
    stop_machine_with_authorization(authorization);
}
`),
    physicalStopOs: withoutCfgTestItems(`
fn restart_bootc_machine() {
    let authorization = authorize_machine_stop();
    stop_machine_with_authorization(authorization);
}
fn apply_machine_os_change() {
    let authorization = authorize_machine_stop();
    stop_machine_with_authorization(authorization);
}
`),
    cli: withoutCfgTestItems(`
fn register_local_krun_teardown_capabilities() {
    KrunTeardownAdapter;
    KrunAttachmentTeardownAdapter;
    ServerIngressPublicationAdapter;
}
struct ForwardedMachineTeardownRegistrations;
const PROVIDER_JOURNAL_NAMESPACE: &str = "forwarded-machine-teardown";
fn registrations() {
    NetworkAttachmentTeardownCapabilities::new();
    WorkloadExecutionTeardownCapabilities::new();
    IngressTeardownCapabilities::new();
}
fn forwarded_request_lifecycle() {
    claim_execute_started();
    execute_started_claim_async();
    adopt_inspect();
    inspect_current_claim_async_and_publish();
}
fn parent_forwarding_lifecycle() {
    begin_parent_publication_withdrawal();
    record_parent_publication_withdrawn_retained();
    begin_parent_publication_release();
    record_parent_publication_released();
}
`),
    sandbox: withoutCfgTestItems(`
struct ProviderCommandCurrentExecution;
impl ProviderCommandCurrentExecution {
    fn authenticates() {}
}
fn claim_dispatch_epoch_started() {}
fn claim_dispatch_epoch_after_inspected_absence_started() {}
fn execute_started_claim_async() {}
`),
    network: withoutCfgTestItems(`
pub struct NetworkAttachmentId(String);
fn retain_provider_managed_batch_after_confirmed_absence() {}
fn release_retained_provider_managed_batch_after_confirmed_absence() {}
`),
    tests: testEntries.map((entry) => entry.source).join("\n"),
    testEntries,
    plan: `
NNC6.5 A1-A24 NNCV035 candidate-frozen Sol/xhigh/fast
persist withdrawal -> withdraw -> drain -> stop -> detach -> release -> record
zero stop effects; Definition/source/session removal waits for safe lifecycle progression
`,
    auditItemComplete: false,
    auditItemCompleteCheckpoint: null,
    currentChangedPaths: [
      "docs/private/plans/README.md",
      "docs/private/plans/nimbus-network-control-plane-plan.md",
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.5-teardown-choreography-substitution-audit.md",
      "scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs",
      "scripts/nimbus-network-control-plane/workload-teardown-contract.sh",
      "scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs",
      "scripts/nimbus-network-control-plane/workload-teardown-test-assertion.mjs",
      "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
      "scripts/verify-nimbus-network-control-plane.sh",
      "scripts/verify-nimbus-network-source-contract.mjs",
    ],
    historicalAuditChangedPaths: [
      "docs/private/plans/README.md",
      "docs/private/plans/nimbus-network-control-plane-plan.md",
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.5-teardown-choreography-substitution-audit.md",
      "scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs",
      "scripts/nimbus-network-control-plane/workload-teardown-contract.sh",
      "scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs",
      "scripts/nimbus-network-control-plane/workload-teardown-test-assertion.mjs",
      "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
      "scripts/verify-nimbus-network-control-plane.sh",
      "scripts/verify-nimbus-network-source-contract.mjs",
    ],
  };
}
