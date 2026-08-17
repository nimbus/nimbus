import {
  maskNonCode,
  withoutCfgTestItems,
} from "./source-contract-scanner.mjs";

function joinSources(sources) {
  return sources.map((entry) => entry.source).join("\n");
}

function fixtureTestEntries(entries) {
  return Object.entries(entries).map(([file, source]) => ({
    file,
    source: maskNonCode(
      source.replace(
        /^(\s*)fn\s+([a-zA-Z0-9_]+)\(\)\s*\{\}/gmu,
        "$1#[test]\n$1fn $2() { assert_eq!(observed, expected); }",
      ),
    ),
  }));
}

export function greenFixture() {
  const files = {
    "crates/nimbus-compute/src/workload_saga.rs": withoutCfgTestItems(``),
    "crates/nimbus-compute/src/workload_saga/restart_decision.rs":
      withoutCfgTestItems(`
fn decide_restart_admission(record: &WorkloadSagaRecord, request: &WorkloadRestartRequest) -> RestartAdmissionDecision {
    require_exact_revision(record.revision(), request.source_revision())?;
    require_exact_generation(record.generation(), request.generation())?;
    require_exact_desired_digest(record.desired_digest(), request.desired_digest())?;
    require_exact_inspection_version(request.inspection_version())?;
    require_exact_provider_selection(record, request.provider_selection())?;
    reject_withdrawal_or_successor(record)?;
    match request.trigger() {
        WorkloadRestartTrigger::Automatic => admit_automatic_restart(record, request, now_unix_millis),
        WorkloadRestartTrigger::Explicit => admit_explicit_restart(record, request),
    }
}
impl WorkloadSagaCoordinator {
    async fn compare_and_swap_restart_admission() {
        let candidate = decide_restart_admission(&current, request)?;
        self.commit_loaded(Some(&current), candidate.clone()).await?;
    }
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs":
      withoutCfgTestItems(`
pub(crate) struct ConfirmedWorkloadRestartCommand {
    command_id: WorkloadRestartCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    source_execution: WorkloadExecutionReference,
    execution: WorkloadExecutionReference,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    request_id: WorkloadRestartRequestId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    successor_veto_generation: Option<WorkloadGeneration>,
    step: WorkloadRestartStep,
    mode: WorkloadRestartCommandMode,
    claim: WorkloadRestartCommandClaim,
    executable: WorkloadExecutableIntent,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
}
impl ConfirmedWorkloadRestartCommand {
    fn from_confirmation(record: &WorkloadSagaRecord, confirmation: WorkloadSagaConfirmation) -> Result<Self, RestartDispatchError> {
        authenticate_exact_restart_confirmation(record, claim, mode)?;
        let mode = match confirmation { WorkloadSagaConfirmation::AppliedByThisCall => WorkloadRestartCommandMode::Execute, _ => WorkloadRestartCommandMode::Inspect };
        Self::try_from((record, confirmation))
    }
    fn source_attempt_id(&self) -> &WorkloadExecutionAttemptId { self.source_execution.attempt_id() }
    fn attempt_id(&self) -> &WorkloadExecutionAttemptId { self.execution.attempt_id() }
}
fn authenticate_exact_restart_confirmation(record: &WorkloadSagaRecord, claim: &WorkloadRestartCommandClaim, mode: WorkloadRestartCommandMode) {
    require_exact_admission(record, claim)?;
    match mode { WorkloadRestartCommandMode::Execute => require_dispatch_pending(record), WorkloadRestartCommandMode::Inspect => require_inspection_required(record) }
}
enum WorkloadRestartCommandOutcome { AuthenticatedAbsent, Ambiguous, InProgress, DefiniteFailure, Succeeded }
fn apply_restart_result(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, result: WorkloadRestartCommandResult) {
    authenticate_result_transition(record, command, &result)?;
    authenticate_result_attempt(record, command, &result)?;
    authenticate_result_dispatch_epoch(record, command, &result)?;
    match result.outcome {
        WorkloadRestartCommandOutcome::AuthenticatedAbsent { evidence } => {
            if command.mode == WorkloadRestartCommandMode::Execute {
                require_restart_inspection(record, command)
            } else if active.successor_veto_generation().is_some() {
                apply_authenticated_absence(candidate)
            } else if command.step() == WorkloadRestartStep::ObservePublication {
                republish_after_authenticated_observation_absence(record, command, evidence)
            } else {
                retry_after_authenticated_absence(record, command, evidence)
            }
        },
        WorkloadRestartCommandOutcome::Ambiguous | WorkloadRestartCommandOutcome::InProgress { .. } => {
            if command.mode == WorkloadRestartCommandMode::Execute {
                require_restart_inspection(record, command)
            } else {
                retain_restart_inspection(command)
            }
        },
        WorkloadRestartCommandOutcome::DefiniteFailure { evidence } => stop_restart_dispatch(candidate),
        WorkloadRestartCommandOutcome::Succeeded { evidence, observed_detail } => persist_restart_success(candidate),
    }
}
fn authenticate_result_transition(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, result: &WorkloadRestartCommandResult) {}
fn authenticate_result_attempt(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, result: &WorkloadRestartCommandResult) {}
fn authenticate_result_dispatch_epoch(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, result: &WorkloadRestartCommandResult) {}
fn retain_restart_inspection(command: &ConfirmedWorkloadRestartCommand) { WorkloadRestartDecision::InspectExact(command.claim()) }
fn require_restart_inspection(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand) {
    let candidate = record.restart_dispatch_to_inspection(command.claim())?;
    ProposedWorkloadRestartTransition::new(candidate, Some(WorkloadRestartSymbolicAction::InspectExactAttempt))
}
fn republish_after_authenticated_observation_absence(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, evidence: WorkloadRestartEvidenceDigest) {
    if command.mode != WorkloadRestartCommandMode::Inspect || command.step() != WorkloadRestartStep::ObservePublication { return Err(InvalidEvidence); }
    let absence = WorkloadRestartAbsenceEvidence::for_inspection(record, command.claim(), evidence)?;
    let candidate = record.restart_observation_absence_to_publication_retry(command.claim(), absence)?;
    ProposedWorkloadRestartTransition::new(candidate, Some(WorkloadRestartSymbolicAction::StartExactAttempt))
}
fn retry_after_authenticated_absence(record: &WorkloadSagaRecord, command: &ConfirmedWorkloadRestartCommand, evidence: WorkloadRestartEvidenceDigest) {
    if command.mode != WorkloadRestartCommandMode::Inspect { return Err(InvalidEvidence); }
    let absence = WorkloadRestartAbsenceEvidence::for_inspection(record, command.claim(), evidence)?;
    let candidate = record.restart_inspection_to_retry(command.claim(), absence)?;
    ProposedWorkloadRestartTransition::new(candidate, Some(WorkloadRestartSymbolicAction::StartExactAttempt))
}
fn stop_restart_dispatch(candidate: WorkloadSagaRecord) { WorkloadRestartDecision::DefiniteFailure(candidate) }
impl WorkloadSagaCoordinator {
    async fn claim_restart_command(loaded: &WorkloadSagaRecord, proposed: &ProposedWorkloadRestartTransition) {
        let confirmation = self.confirm_transition(Some(loaded), candidate.clone()).await?;
        if proposed.action_after_confirmation() == Some(WorkloadRestartSymbolicAction::StartExactAttempt)
            && matches!(confirmation, WorkloadSagaConfirmation::ConfirmedAfterAmbiguity | WorkloadSagaConfirmation::ConfirmedReplay) {
            return self.inspect_ambiguous_restart(&candidate).await;
        }
        ConfirmedWorkloadRestartCommand::from_confirmation(&candidate, action, confirmation)
    }
    async fn inspect_ambiguous_restart(pending: &WorkloadSagaRecord) {
        let inspection = pending.restart_dispatch_to_inspection(claim)?;
        let confirmation = self.confirm_transition(Some(pending), inspection.clone()).await?;
        ConfirmedWorkloadRestartCommand::from_confirmation(&inspection, WorkloadRestartSymbolicAction::InspectExactAttempt, confirmation)
    }
    async fn compare_and_swap_restart_result(loaded: &WorkloadSagaRecord, proposed: &ProposedWorkloadRestartTransition) {
        self.confirm_transition(Some(loaded), proposed.candidate().clone()).await
    }
}
`),
    "crates/nimbus-workloads/src/saga/state/restart.rs": withoutCfgTestItems(`
fn restart_step_for_phase(phase: WorkloadRestartPhase) -> Option<WorkloadRestartStep> {
    match phase {
        WorkloadRestartPhase::PublicationWithdrawalPending => Some(WorkloadRestartStep::WithdrawPublication),
        WorkloadRestartPhase::ExecutionQuiescencePending => Some(WorkloadRestartStep::QuiesceExecution),
        WorkloadRestartPhase::PreparationPending => Some(WorkloadRestartStep::PrepareExecution),
        WorkloadRestartPhase::AttachmentPending => Some(WorkloadRestartStep::AttachNetwork),
        WorkloadRestartPhase::ActivationPrerequisitePending => Some(WorkloadRestartStep::InspectActivationPrerequisites),
        WorkloadRestartPhase::ActivationPending => Some(WorkloadRestartStep::ActivateExecution),
        WorkloadRestartPhase::ReadinessPending => Some(WorkloadRestartStep::InspectReadiness),
        WorkloadRestartPhase::PublicationPending => Some(WorkloadRestartStep::Publish),
        WorkloadRestartPhase::ObservationPending => Some(WorkloadRestartStep::ObservePublication),
    }
}
fn restart_target_for_step(step: WorkloadRestartStep) -> Option<WorkloadRestartPhase> {
    match step {
        WorkloadRestartStep::WithdrawPublication => Some(WorkloadRestartPhase::ExecutionQuiescencePending),
        WorkloadRestartStep::QuiesceExecution => Some(WorkloadRestartPhase::Scheduled),
        WorkloadRestartStep::PrepareExecution => Some(WorkloadRestartPhase::AttachmentPending),
        WorkloadRestartStep::AttachNetwork => Some(WorkloadRestartPhase::ActivationPrerequisitePending),
        WorkloadRestartStep::InspectActivationPrerequisites => Some(WorkloadRestartPhase::ActivationPending),
        WorkloadRestartStep::ActivateExecution => Some(WorkloadRestartPhase::ReadinessPending),
        WorkloadRestartStep::InspectReadiness => Some(WorkloadRestartPhase::PublicationPending),
        WorkloadRestartStep::Publish => Some(WorkloadRestartPhase::ObservationPending),
        WorkloadRestartStep::ObservePublication => None,
    }
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_driver.rs":
      withoutCfgTestItems(`
async fn drive_confirmed_restart(record: WorkloadSagaRecord) {
    let decision = decide_restart_progress(&record, now_unix_millis)?;
    let confirmed = self.dispatcher.confirm_transition(&self.coordinator, &record, &proposed).await?;
    let durable = confirmed.confirmed_record().cloned()?;
    let command = transition.command().cloned()?;
    let result = self.dispatcher.dispatch_confirmed(&confirmed).await?;
    let result_decision = apply_restart_result(&durable, &command, result)?;
    self.coordinator.compare_and_swap_restart_result(&durable, &proposed).await?;
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_dispatcher.rs":
      withoutCfgTestItems(`
async fn dispatch_confirmed(command: &ConfirmedWorkloadRestartCommand) {
    let observation = capabilities.invoke(command).await;
    if !observation.matches_command(command) {
        return Err(WorkloadRestartDispatchError::CrossedProviderObservation);
    }
    WorkloadRestartCommandResult::for_command(command, observation.into_outcome())
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_runtime.rs":
      withoutCfgTestItems(`
async fn submit_explicit() { coordinator.compare_and_swap_restart_admission().await; }
`),
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs":
      withoutCfgTestItems(`
trait RestartPublicationWithdrawalCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait WorkloadExecutionQuiescenceCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait WorkloadRestartPreparationCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait NetworkRestartAttachmentCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait WorkloadRestartActivationPrerequisiteCapability: Send + Sync {
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait WorkloadRestartActivationCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait WorkloadRestartReadinessCapability: Send + Sync {
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait RestartPublicationCapability: Send + Sync {
    fn execute(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
trait RestartPublicationObservationCapability: Send + Sync {
    fn inspect(&self, command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartCapabilityFuture<'_>;
}
fn register_restart_capabilities(capabilities: WorkloadRestartCapabilities) {
    let realm = (capabilities.execution_provider_id.clone(), capabilities.network_selection.clone());
    if self.providers.insert(realm.clone(), capabilities).is_some() {
        return Err(WorkloadRestartCapabilityRegistryError::DuplicateProviderSelection);
    }
}
fn resolve_restart_capabilities(command: &ConfirmedWorkloadRestartCommand) {
    let realm = (command.provider_selection().clone(), network_selection);
    let capabilities = self.providers.get(&realm).ok_or_else(|| WorkloadRestartCapabilityRegistryError::MissingProviderSelection);
    if !capabilities.matches_command(command) { return Err(WorkloadRestartCapabilityRegistryError::CrossedProviderRealm); }
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs":
      withoutCfgTestItems(`
macro_rules! impl_sandbox_restart_capabilities {
    ($adapter:ty) => {
        impl WorkloadExecutionQuiescenceCapability for $adapter {}
        impl WorkloadRestartPreparationCapability for $adapter {}
        impl NetworkRestartAttachmentCapability for $adapter {}
        impl WorkloadRestartActivationPrerequisiteCapability for $adapter {}
        impl WorkloadRestartActivationCapability for $adapter {}
        impl WorkloadRestartReadinessCapability for $adapter {}
    };
}
impl_sandbox_restart_capabilities!(ContainerProvisionAdapter);
impl_sandbox_restart_capabilities!(KrunProvisionAdapter);
`),
    "crates/nimbus-server/src/workload_ingress.rs": withoutCfgTestItems(`
impl RestartPublicationWithdrawalCapability for ServerIngressPublicationAdapter {}
impl RestartPublicationCapability for ServerIngressPublicationAdapter {}
impl RestartPublicationObservationCapability for ServerIngressPublicationAdapter {}
`),
    "crates/nimbus-compute/src/workload_saga/restart_watch.rs":
      withoutCfgTestItems(`
const MAX_RESTART_PAGES_PER_SWEEP: usize = 64;
trait RestartClock: Send + Sync {
    fn now_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis;
    fn wait_until(&self, deadline: WorkloadRestartNotBeforeUnixMillis, cancellation: &WorkloadRestartCancellationToken) -> RestartWaitFuture<'_>;
}
struct DurableRestartWatch {
    page_size: NonZeroUsize,
    clock: Arc<dyn RestartClock>,
    cancellation: WorkloadRestartCancellationToken,
    sweep_cursor: Mutex<Option<WorkloadRestartCandidateCursor>>,
}
async fn load_durable_restart_page(&self) {
    let page_size = self.page_size.get();
    self.coordinator.list_restart_candidates(request).await?;
}
async fn dispatch_each_due_epoch_once(&self) {
    let mut retained_cursor = self.sweep_cursor.lock().await;
    while pages < MAX_RESTART_PAGES_PER_SWEEP {
        if self.cancellation.is_cancelled() { break; }
        let page = self.load_durable_restart_page(cursor).await?;
        self.supervisor.track(record.clone())?;
    }
    *retained_cursor = cursor;
}
async fn bounded_restart_watch(&self) {
    if self.cancellation.is_cancelled() { return RestartWait::Cancelled; }
    let now = self.clock.now_unix_millis();
    let deadline = self.dispatch_each_due_epoch_once().await?;
    self.clock.wait_until(deadline, &self.cancellation).await;
}
fn read_only_exit_hint() -> RestartHint { RestartHint::ReadOnly }
`),
    "crates/nimbus-compute/src/workload_saga/restart_submission.rs":
      withoutCfgTestItems(`
struct ExplicitWorkloadRestartSubmitter { explicit_submitter: () }
impl ExplicitWorkloadRestartSubmitter {
    async fn submit() {
        self.coordinator.compare_and_swap_restart_admission(&admission, cancellation).await?;
        self.supervisor.track(confirmed.record().clone())?;
    }
}
`),
    "crates/nimbus-compute/src/services.rs": withoutCfgTestItems(`
async fn submit_service_restart() {
    let source_generation = WorkloadProvisionSourceGeneration::new(source_generation);
    let source_identity = WorkloadProvisionSourceIdentity::sandbox_backed_service(service_name)?;
    let key = WorkloadSagaKey::new(tenant_id, workload_id);
    let request = ExplicitWorkloadRestartRequest::new(key, source_identity, source_generation, request_id);
    runtime.submit_explicit(&request, cancellation).await?;
}
`),
    "crates/nimbus-server/src/http/services.rs": withoutCfgTestItems(`
pub struct ServiceRestartRequest {
    source_generation: u64,
    request_id: String,
}
const ACCEPTED: StatusCode = StatusCode::ACCEPTED;
`),
    "crates/nimbus-services/src/manager/restart.rs": withoutCfgTestItems(`
fn resolve_service_name() { resolve_catalog_entry(); }
`),
  };
  const testEntries = fixtureTestEntries({
    "crates/nimbus-compute/src/workload_saga/restart_decision/tests.rs": `
fn automatic_and_explicit_restart_use_same_reducer() {}
fn concurrent_triggers_force_same_revision_before_competing_cas() {}
fn crossed_admission_fences_fail_before_cas() {}
fn withdrawal_winning_before_admission_vetoes_cas() {}
fn successor_winning_before_admission_vetoes_cas() {}
fn explicit_restart_does_not_increment_automatic_count() {}
fn deadline_not_due_returns_wait_without_effect() {}
fn cancellation_before_submission_makes_zero_store_and_provider_calls() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_dispatch/tests.rs": `
fn confirmed_restart_command_is_private_and_complete() {}
fn direct_claim_cas_winner_alone_executes() {}
fn confirmed_replay_does_not_execute() {}
fn ambiguous_claim_cas_fresh_reads_before_effect() {}
fn crash_after_restart_effect_inspects_before_retry() {}
fn authenticated_absence_retries_same_attempt_at_next_dispatch_epoch() {}
fn fresh_process_observation_absence_republishes_once_before_observing_again() {}
fn execute_absence_with_successor_requires_exact_inspection_before_terminal_veto() {}
fn in_progress_never_retries() {}
fn definite_failure_stops_later_commands() {}
fn crossed_restart_result_is_rejected() {}
fn reused_skipped_and_crossed_dispatch_epochs_fail_closed() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_dispatcher/tests.rs": `
fn old_attempt_provider_observation_is_rejected_before_result() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_driver/tests.rs": `
fn publication_withdrawal_precedes_execution_quiescence() {}
fn restart_retained_detach_precedes_attachment() {}
fn activation_waits_for_same_generation_attachment_and_pep() {}
fn readiness_binds_the_new_execution_attempt() {}
fn publication_waits_for_new_attempt_readiness() {}
fn withdrawal_after_admission_vetoes_unissued_command() {}
fn successor_after_effect_before_result_cas_allows_inspection_only() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_provider/tests.rs": `
fn restart_registry_rejects_duplicate_provider_selection() {}
fn restart_registry_has_no_first_available_fallback() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_sandbox/tests.rs": `
fn container_restart_quiescence_capability_authenticates_command() {}
fn container_restart_preparation_retains_authority_and_binds_attempt() {}
fn krun_restart_quiescence_capability_authenticates_command() {}
fn krun_restart_preparation_retains_authority_and_binds_attempt() {}
fn real_restart_adapters_reject_crossed_provider_attempt_and_inspection() {}
fn concurrent_restart_dispatch_produces_one_provider_effect() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_watch/tests.rs": `
fn automatic_watch_loads_one_bounded_durable_page() {}
fn automatic_watch_does_not_busy_spin_before_deadline() {}
fn automatic_watch_dispatches_each_due_epoch_once() {}
fn automatic_watch_caps_each_sweep_and_rotates_cursor() {}
fn read_only_exit_hint_cannot_submit_or_execute_restart() {}
fn watch_cancellation_cancels_waiter_not_durable_work() {}
`,
    "__fixture__/legacy_tests.rs": `
fn same_generation_restart_keeps_desired_generation() {}
fn restart_legal_transition_matrix_is_exhaustive() {}
fn restart_recovery_eligibility_is_exhaustive() {}
fn explicit_restart_does_not_consume_automatic_count() {}
fn deadline_survives_clock_rollback_without_early_admission() {}
fn deadline_survives_engine_reopen() {}
fn count_survives_engine_reopen() {}
fn withdrawal_vetoes_unissued_restart() {}
fn successor_vetoes_restart_before_admission() {}
fn duplicate_service_request_returns_same_restart_epoch() {}
fn completed_explicit_request_replay_returns_the_same_restart_epoch() {}
fn completed_explicit_request_rejects_crossed_admission_content() {}
fn reconciler_rejects_provider_restart_and_duplicates_before_backend_validation() {}
fn machine_restart_wire_rejects_crossed_fences() {}
fn restart_retained_attach_must_retain_network_allocation_retain_port_lease_retain_attachment_identity_retain_pep_authority() {}
fn fresh_process_restart_reopens_engine() {}
fn cancellation_after_submission_preserves_durable_work() {}
fn compose_local_and_forwarded_restart_use_compute() {}
`,
  });
  return {
    workloads: withoutCfgTestItems(`
pub enum WorkloadRestartPolicy {
    Never,
    OnFailure { max_restarts: u32 },
    Always { max_restarts: u32 },
}
pub enum WorkloadRestartTrigger { Automatic, Explicit }
pub struct WorkloadRestartEpoch(u64);
pub struct WorkloadRestartRequestId(String);
pub struct WorkloadExecutionAttemptId(String);
pub enum WorkloadRestartPhase {
    Idle,
    Requested,
    PublicationWithdrawalPending,
    ExecutionQuiescencePending,
    Scheduled,
    PreparationPending,
    AttachmentPending,
    ActivationPrerequisitePending,
    ActivationPending,
    ReadinessPending,
    PublicationPending,
    ObservationPending,
}
pub enum WorkloadRestartDisposition {
    Ready,
    DispatchPending,
    InspectionRequired,
    DefiniteFailure,
}
pub struct WorkloadRestartAdmission {
    saga_id: WorkloadSagaId,
    source: WorkloadProvisionSourceEvidence,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    revision: WorkloadSagaRevision,
    trigger: WorkloadRestartTrigger,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    restart_epoch: WorkloadRestartEpoch,
    policy_attempt_count: u32,
    request_id: WorkloadRestartRequestId,
    attempt_id: WorkloadExecutionAttemptId,
}
pub struct WorkloadRestartState {
    current_execution_attempt_id: WorkloadExecutionAttemptId,
    phase: WorkloadRestartPhase,
    not_before_unix_millis: u64,
    completed_automatic_restart_count: u32,
}
pub struct WorkloadSagaRecord {
    restart: WorkloadRestartState,
}
struct TransitionIdentityPayload { restart: &'a WorkloadRestartState }
`),
    compute: joinSources(
      Object.entries(files)
        .filter(([file]) => file.startsWith("crates/nimbus-compute/"))
        .map(([file, source]) => ({ file, source })),
    ),
    files,
    providers: withoutCfgTestItems(`
enum AttachmentDisposition { RestartRetained, Terminal }
`),
    services: files["crates/nimbus-services/src/manager/restart.rs"],
    server: files["crates/nimbus-server/src/http/services.rs"],
    sdk: `services.restart({ sourceGeneration, requestId });\n/api/tenants/:tenant/services/:service/restart`,
    codec: `
const REQUIRED_FIELDS: [&str; 2] = ["restartPolicy", "restartState"];
fn validate_physical_shape() {}
`,
    node: withoutCfgTestItems(`
fn into_host_lifecycle_request() { HostLifecycleProperty::Restart(HostRestartPolicy::No); }
fn ensure_external_restart_disabled() { policy != HostRestartPolicy::No; }
`),
    machine: withoutCfgTestItems(`
pub struct MachineApiWorkloadRestartCommandEnvelope {
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    source_execution: WorkloadExecutionReference,
    execution: WorkloadExecutionReference,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    request_id: WorkloadRestartRequestId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    step: WorkloadRestartStep,
    claim: WorkloadRestartCommandClaim,
    executable: WorkloadExecutableIntent,
    network_plan_digest: NetworkPlanDigest,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    machine_forwarder_authority: MachineForwarderAuthority,
    machine_provider_generation: NetworkResourceGeneration,
}
`),
    network: withoutCfgTestItems("pub struct NetworkAttachmentId(String);"),
    tests: joinSources(testEntries),
    testEntries,
    plan: "NNC6.4a A1-A20 NNCV034 candidate-frozen Sol/xhigh/fast",
    changedPaths: [
      "scripts/verify-nimbus-network-source-contract.mjs",
      "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
      "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
      "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
      "scripts/verify-nimbus-network-control-plane.sh",
      "docs/private/plans/nimbus-network-control-plane-plan.md",
      "docs/private/plans/README.md",
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
    ],
    r1ChangedPaths: [
      "crates/nimbus-workloads/src/saga/restart.rs",
      "crates/nimbus-workloads/src/saga/state/restart.rs",
      "crates/nimbus-server/src/workload_saga_store/codec.rs",
    ],
    r2ChangedPaths: [
      "crates/nimbus-compute/src/workload_saga/restart_decision.rs",
      "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs",
      "crates/nimbus-compute/src/workload_saga/restart_driver.rs",
      "crates/nimbus-compute/src/workload_saga/restart_provider.rs",
      "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs",
      "crates/nimbus-compute/src/workload_saga/restart_watch.rs",
      "crates/nimbus-sandbox/src/backends/container/runtime/restart_provider.rs",
      "crates/nimbus-sandbox/src/backends/krun/vm/restart.rs",
    ],
    r3ChangedPaths: [
      "scripts/nimbus-network-control-plane/workload-restart-contract-fixture.mjs",
      "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
    ],
  };
}
