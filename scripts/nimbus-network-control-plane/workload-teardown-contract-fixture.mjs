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
  "parent_publication_must_be_withdrawn_before_every_guest_phase",
  "teardown_client_rejects_missing_or_crossed_authority_before_socket_io",
  ...PHYSICAL_MACHINE_STOP_TESTS,
  "final_withdrawal_closes_routes_joins_workers_and_releases_exact_leases",
  "final_withdrawal_settlement_failure_blocks_progress_and_preserves_fences",
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

export const FINAL_CONVERGENCE_TESTS = new Map([
  [
    "crates/nimbus-compute/src/resource_retirement/tests/tenant_retirement.rs",
    [
      "tenant_driver_paginates_and_drives_each_exact_key_once_per_pass",
      "tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete",
    ],
  ],
  [
    "crates/nimbus-compute/src/resource_provision/tests.rs",
    [
      "failed_service_start_enters_durable_compensation_without_caller_stop",
      "failed_sandbox_start_enters_durable_compensation_without_caller_stop",
      "failed_provision_compensation_survives_waiter_cancellation",
      "failed_provision_waiting_compensation_retains_owner_until_exact_inspection_completes",
      "failed_provision_cleanup_pending_retains_key_and_blocks_reuse",
      "failed_provision_compensation_error_retries_exact_run_without_provision_effects",
    ],
  ],
  [
    "crates/nimbus-sandbox/src/backends/krun/vm/teardown/tests/network_teardown/fresh_process.rs",
    ["fresh_process_interrupted_adoption_converges_and_replays_without_writes"],
  ],
  [
    "crates/nimbus-services/src/manager/tenant_retirement/tests.rs",
    [
      "sandbox_backed_service_and_standalone_sandbox_cannot_share_workload_id",
      "standalone_reservation_rechecks_sandbox_service_name_collision",
    ],
  ],
  [
    "crates/nimbus-compute/src/resource_retirement/tests/restart_settlement.rs",
    ["active_restart_settles_before_withdrawal_committed"],
  ],
  [
    "crates/nimbus-server/src/workload_saga_store/tests/provision_driver_process.rs",
    ["failed_provision_compensation_reopens_result_and_cause_cuts"],
  ],
  [
    "crates/nimbus-server/src/workload_saga_store/tests/restart_settlement_process.rs",
    ["restart_settlement_reopens_without_duplicate_execute"],
  ],
]);

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
  for (const [file, names] of FINAL_CONVERGENCE_TESTS) {
    testEntries.push({
      file,
      source: names.map(testSource).join("\n"),
    });
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
    tenantRetirement: withoutCfgTestItems(`
async fn retire(&self, tenant_id: TenantId) {
    let identity = self.engine.enter_tenant_runtime_async(tenant_id.clone()).await?;
    let snapshot = self.driver.services
        .claim_tenant_source_retirement(&tenant_id, identity.tenant_incarnation())?;
    let record = self.driver.persist_intent(&snapshot).await?;
    drop(identity);
    self.resume(record, snapshot).await
}
async fn recover_retained(&self) {
    let records = self.driver.list_retained_retirements().await?;
    let mut retirements = Vec::new();
    for record in records {
        let snapshot = self.driver.services.restore_tenant_source_retirement(&record)?;
        retirements.push((record, snapshot));
    }
    for (record, snapshot) in retirements {
        self.resume(record, snapshot).await?;
    }
    Ok(())
}
async fn resume(&self, retained: TenantRetirementRecord, snapshot: TenantSourceRetirementSnapshot) {
    let mut current = self.driver.adopt_exact_or_later(&retained).await?;
    let exact_deletion = self.engine
        .begin_tenant_incarnation_delete_async(current.tenant_id().clone(), current.tenant_incarnation())
        .await?;
    self.runtime_manager.retire_tenant_deletion(&exact_deletion).await?;
    let terminal = self.driver.drive_children_to_recorded(&snapshot).await?;
    current = self.driver
        .advance_progress(&current, TenantRetirementPhase::ChildrenRecorded)
        .await?;
    self.driver.finalize_recorded_sources(&snapshot, &terminal)?;
    current = self.driver
        .advance_progress(&current, TenantRetirementPhase::SourcesFinalized)
        .await?;
    self.engine.finish_tenant_delete_async(exact_deletion).await?;
    current = self.driver
        .advance_progress(&current, TenantRetirementPhase::EngineDeleted)
        .await?;
    current = self.driver
        .advance_progress(&current, TenantRetirementPhase::Recorded)
        .await?;
    self.driver.release_source_fences(&snapshot)?;
    self.driver.services.release_tenant_source_retirement(snapshot.claim())?;
    self.driver.delete_terminal(&current).await
}
async fn list_retained_retirements(&self) {
    let mut cursor = None;
    let mut pages = 0;
    let mut records = Vec::new();
    loop {
        if pages == MAX_TENANT_RETIREMENT_PAGES { return Err(PageLimit); }
        let request = TenantRetirementPageRequest::new(
            cursor,
            nimbus_workloads::MAX_TENANT_RETIREMENT_PAGE_SIZE,
        )?;
        let page = self.retirement_store.list_retirements(request).await?;
        pages += 1;
        records.extend_from_slice(page.records());
        let Some(next) = page.next_cursor().cloned() else { break; };
        cursor = Some(next);
    }
    Ok(records)
}
async fn persist_intent(&self, snapshot: &TenantSourceRetirementSnapshot) {
    let intended = TenantRetirementRecord::new(snapshot.claim().tenant_id().clone())?;
    match self.retirement_store
        .compare_and_swap_retirement(TenantRetirementExpected::Missing, intended.clone())
        .await
    {
        Ok(TenantRetirementCommit::Applied | TenantRetirementCommit::Unchanged) => Ok(intended),
        Err(TenantRetirementStoreError::Conflict { .. } | TenantRetirementStoreError::Ambiguous) => {
            self.adopt_exact_or_later(&intended).await
        }
    }
}
async fn advance_progress(&self, current: &TenantRetirementRecord, target: TenantRetirementPhase) {
    let next = current.advance(target)?;
    match self.retirement_store.compare_and_swap_retirement(
        TenantRetirementExpected::Revision(current.revision()),
        next.clone(),
    ).await {
        Ok(TenantRetirementCommit::Applied | TenantRetirementCommit::Unchanged) => Ok(next),
        Err(_) => self.adopt_exact_or_later(&next).await,
    }
}
async fn delete_terminal(&self, terminal: &TenantRetirementRecord) {
    match self.retirement_store.delete_retirement(terminal.clone()).await {
        Err(TenantRetirementStoreError::Ambiguous) => {
            match self.retirement_store.load_retirement(terminal.tenant_id()).await? {
                None => return Ok(()),
                Some(current) if current == *terminal => continue,
                Some(_) => return Err(Crossed),
            }
        }
        result => result,
    }
}
async fn drive_children_to_recorded(&self, snapshot: &TenantSourceRetirementSnapshot) {
    let source_keys = snapshot_source_keys(snapshot)?;
    self.resource_retirer.fence_tenant_sources_and_join(&source_keys).await?;
    let initial = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
    authenticate_snapshot_inventory(snapshot, &initial)?;
    for record in initial.values() {
        self.resource_retirer
            .submit_tenant_record_teardown(record.clone())
            .await?;
    }
    let final_records = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
    require_all_recorded_before_finish_tenant_delete(&initial, &final_records)?;
    authenticate_snapshot_inventory(snapshot, &final_records)?;
    Ok(final_records.into_values().collect())
}
async fn load_recorded_children(&self, snapshot: &TenantSourceRetirementSnapshot) {
    let source_keys = snapshot_source_keys(snapshot)?;
    self.resource_retirer.fence_tenant_sources_and_join(&source_keys).await?;
    let records = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
    authenticate_snapshot_inventory(snapshot, &records)?;
    if records.values().any(|record| {
        record.phase() != WorkloadSagaPhase::Recorded
            || record.active_intent().desired_state() != DesiredWorkloadState::Stopped
            || record.successor_intent().is_some()
    }) { return Err(InvalidInventory); }
    Ok(records.into_values().collect())
}
async fn list_tenant_sagas(&self, tenant_id: &TenantId) {
    let epoch_before = self.retirement_store.load_workload_mutation_epoch(tenant_id).await?;
    let mut records = BTreeMap::new();
    let mut cursor: Option<WorkloadSagaTenantCursor> = None;
    loop {
        let request = WorkloadSagaTenantPageRequest::new(cursor.clone(), self.page_size)?;
        let page = self.coordinator.list_for_tenant(tenant_id, request).await?;
        if page.tenant_id() != tenant_id { return Err(WorkloadSagaStoreError::Corrupt); }
        let next = page.next_cursor().cloned();
        let mut previous = cursor.as_ref().map(WorkloadSagaTenantCursor::key);
        for record in page.records() {
            if record.key().tenant_id() != tenant_id
                || previous.is_some_and(|previous| record.key() <= previous)
            { return Err(WorkloadSagaStoreError::Corrupt); }
            previous = Some(record.key());
        }
        if let Some(next) = next.as_ref()
            && (next.tenant_id() != tenant_id
                || page.records().last().map(WorkloadSagaRecord::key) != Some(next.key())
                || cursor.as_ref().is_some_and(|cursor| next.key() <= cursor.key()))
        { return Err(WorkloadSagaStoreError::Corrupt); }
        for record in page.into_records() {
            if records.insert(record.key().clone(), record).is_some() {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
        }
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    let epoch_after = self.retirement_store.load_workload_mutation_epoch(tenant_id).await?;
    if epoch_before != epoch_after { return Err(InvalidInventory); }
    Ok(records)
}
fn require_all_recorded_before_finish_tenant_delete(initial, final_records) {
    if initial.keys().ne(final_records.keys()) { return Err(InvalidInventory); }
    if final_records.values().any(|record| {
        record.phase() != WorkloadSagaPhase::Recorded
            || record.active_intent().desired_state() != DesiredWorkloadState::Stopped
            || record.successor_intent().is_some()
    }) { return Err(InvalidInventory); }
    Ok(())
}
`),
    provisionCompensation: withoutCfgTestItems(`
async fn compensate_definite_provision_failure(&self, failed: &WorkloadSagaRecord) {
    let withdrawal = self.coordinator
        .commit_failed_provision_compensation(failed)
        .await?;
    let cancellation = WorkloadTeardownCancellationToken::default();
    self.teardown_runtime
        .submit(withdrawal.key().clone(), &cancellation)
        .await
}
`),
    workloadSagaCoordinator: withoutCfgTestItems(`
async fn commit_failed_provision_compensation(&self, failed: &WorkloadSagaRecord) {
    let (claim, failure) = match failed.provision_disposition() {
        Some(WorkloadProvisionDisposition::DefiniteFailure { claim, failure }) => {
            (claim.clone(), failure.clone())
        }
        _ => return Err(InvalidTransition),
    };
    let cause = WorkloadTeardownCause::FailedProvision {
        claim: Box::new(claim),
        failure,
    };
    let withdrawal = failed.commit_teardown_cause(cause.clone())?;
    match self.confirm_transition(Some(failed), withdrawal.clone()).await? {
        WorkloadSagaConfirmation::AppliedByThisCall
        | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
        | WorkloadSagaConfirmation::ConfirmedReplay => Ok(withdrawal),
        WorkloadSagaConfirmation::Conflict { .. }
        | WorkloadSagaConfirmation::UnresolvedAmbiguity => {
            let observed = self.load(failed.key()).await?.ok_or(Corrupt)?;
            authenticate_failed_provision_compensation(failed, &withdrawal, &cause, &observed)?;
            Ok(observed)
        }
    }
}
`),
    workloadProvisioner: withoutCfgTestItems(`
pub fn new(
        local_node: NodeIdentity,
        coordinator: Arc<WorkloadSagaCoordinator>,
        teardown_runtime: Arc<WorkloadTeardownRuntime>,
) {
    let compensation = WorkloadProvisionCompensator::new(
        Arc::clone(&coordinator),
        teardown_runtime,
    );
}
pub enum WorkloadProvisionError {
    Compensation {
        source: Arc<WorkloadProvisionCompensationError>,
        failed_run: Box<WorkloadProvisionRun>,
    },
}
enum RetainedCompensationWork {
    ResumeTeardown(Box<WorkloadProvisionOutcome>),
    RetryFailedProvisionHandoff(Box<WorkloadProvisionRun>),
}
async fn finalize_run(&self, run: WorkloadProvisionRun) {
    let (durable_record, compensation) = match run.disposition() {
        WorkloadProvisionRunDisposition::DefiniteFailure => {
            let teardown = match self.compensation
                .compensate_definite_provision_failure(run.record())
                .await
            {
                Ok(teardown) => teardown,
                Err(source) => {
                    return Err(Arc::new(WorkloadProvisionError::Compensation {
                        source: Arc::new(source),
                        failed_run: Box::new(run),
                    }));
                }
            };
            let state = match teardown.disposition() {
                WorkloadTeardownRunDisposition::Completed => WorkloadProvisionCompensationState::Completed,
                WorkloadTeardownRunDisposition::Waiting => WorkloadProvisionCompensationState::Waiting,
                WorkloadTeardownRunDisposition::CleanupPending => WorkloadProvisionCompensationState::CleanupPending,
            };
            (teardown.record().clone(), state)
        }
        WorkloadProvisionRunDisposition::Observed
        | WorkloadProvisionRunDisposition::Waiting
        | WorkloadProvisionRunDisposition::SuccessorSettlementReady => {
            (run.record().clone(), WorkloadProvisionCompensationState::NotRequired)
        }
    };
    let projection = self.projection.project(&run).await;
    Ok(WorkloadProvisionOutcome { run, durable_record, compensation, projection, })
}
fn retain_after_result(result: &WorkloadProvisionResult) {
    match result {
        Ok(outcome) => matches!(
            outcome.compensation(),
            WorkloadProvisionCompensationState::Waiting
                | WorkloadProvisionCompensationState::CleanupPending
        ),
        Err(error) => matches!(error.as_ref(), WorkloadProvisionError::Compensation { .. }),
    }
}
fn retained_compensation_work(
    result: &WorkloadProvisionResult,
) -> Option<RetainedCompensationWork> {
    match result {
        Ok(outcome)
            if outcome.compensation() == WorkloadProvisionCompensationState::Waiting =>
        {
            Some(RetainedCompensationWork::ResumeTeardown(Box::new(
                outcome.clone(),
            )))
        }
        Err(error) => match error.as_ref() {
            WorkloadProvisionError::Compensation { failed_run, .. } => Some(
                RetainedCompensationWork::RetryFailedProvisionHandoff(failed_run.clone()),
            ),
            _ => None,
        },
        _ => None,
    }
}
fn parked_retained_result(result: &WorkloadProvisionResult) {
    Self::retain_after_result(result) && Self::retained_compensation_work(result).is_none()
}
fn publish_tracked_result(&self, key, sender, result) {
    let retain = Self::retain_after_result(&result);
    sender.send_replace(Some(result));
    if retain {
        entry._task = None;
    } else {
        supervisor.in_flight.remove(key);
    }
}
fn track_submission(&self) {
    if existing.completion.borrow().as_ref().is_some_and(Self::parked_retained_result) {
        return Ok(existing.completion.clone());
    }
    if let Some(retry) = existing.completion.borrow().as_ref().and_then(Self::retained_compensation_work) {
        self.spawn_retained_compensation_task(key, retry, sender);
    }
}
fn track_resume(&self) {
    if let Some(retry) = existing.completion.borrow().as_ref().and_then(Self::retained_compensation_work) {
        self.spawn_retained_compensation_task(key, retry, sender);
    }
}
fn spawn_submission_task(&self) {
    let task = tokio::spawn(async move {
        let result = match provisioner.driver.submit_and_drive(task_key.clone(), intent).await {
            Ok(run) => provisioner.finalize_run(run).await,
            Err(error) => Err(error),
        };
        provisioner.publish_tracked_result(&task_key, &sender, result);
    });
}
fn spawn_retained_compensation_task(&self) {
    let task = tokio::spawn(async move {
        let result = match work {
            RetainedCompensationWork::ResumeTeardown(prior) => {
                provisioner.resume_compensation(*prior).await
            }
            RetainedCompensationWork::RetryFailedProvisionHandoff(run) => {
                provisioner.finalize_run(*run).await
            }
        };
        provisioner.publish_tracked_result(&task_key, &sender, result);
    });
}
async fn wait_for_completion(mut receiver, cancellation) {
    loop {
        if let Some(result) = receiver.borrow().clone() { return result; }
        if *cancellation_signal.borrow() {
            return Err(WorkloadProvisionError::WaiterCancelled);
        }
        tokio::select! {
            changed = cancellation_signal.changed() => {
                if changed.is_err() || *cancellation_signal.borrow() {
                    return Err(WorkloadProvisionError::WaiterCancelled);
                }
            }
            changed = receiver.changed() => {}
        }
    }
}
`),
    restartRuntime: withoutCfgTestItems(`
async fn settle_restart_for_teardown_once(coordinator, driver, key, now_unix_millis) {
    let run = driver.resume(key, now_unix_millis).await?;
    if run.record().restart_state().active().is_none() {
        return Ok(WorkloadRestartSettlement::Settled);
    }
    if matches!(run.record().restart_state().active().map(|active| active.disposition()),
        Some(WorkloadRestartDisposition::SuccessorVetoed { .. }
            | WorkloadRestartDisposition::DefiniteFailure { .. }))
    {
        coordinator.commit_restart_settlement_teardown(run.record()).await?;
        return Ok(WorkloadRestartSettlement::Settled);
    }
    Ok(WorkloadRestartSettlement::Pending)
}
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
        let teardown_runtime = teardown_capabilities.map(|capabilities| {
            let capabilities = capabilities.into_registry_for(
                &capability_selection,
                &execution_provider_id,
            );
            WorkloadTeardownRuntime::new(capabilities)
        });
        let provisioner = teardown_runtime.as_ref().map(|runtime| {
            WorkloadProvisioner::new(Arc::clone(runtime));
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
    teardown_capabilities: ExactWorkloadTeardownCapabilityRealm,
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
            teardown_capabilities: Some(Box::new(self.teardown_capabilities)),
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
async fn drive_recorded_teardown(&self, key, loaded, claim, joined_provision) {
    let (record, successor_generation) = self.persist_stopped_successor(key, loaded).await?;
    self.retire_late_provision_result(key, record.phase(), joined_provision).await?;
    self.settle_issued_restart_before_native_teardown(key).await?;
    let run = self.teardown_runtime.submit(key.clone(), &cancellation).await?;
    Ok(run)
}
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
    let _permit = match &self.desire_admission_guard {
        Some(guard) => Some(guard.acquire(&admission).await),
        None => None,
    };
    let disposition = self.commit_loaded(loaded.as_ref(), next.clone()).await;
}
async fn compare_and_swap_restart_admission() {
    let _permit = match &self.desire_admission_guard {
        Some(guard) => Some(guard.acquire(&admission).await),
        None => None,
    };
    let result = self.commit_loaded(Some(&current), candidate.clone()).await;
    drop(_permit);
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
pub async fn authorize_physical_machine_stop() {
    let claim = barriers.claim_effect_free_barrier();
    let sagas = workloads.list_machine_workload_authority_from_engine().await;
    match classify_machine_stop_authority(claim, sagas) {
        MachineWorkloadStopDecision::ActiveWorkloadTeardownRequired => {
            barriers.clear_effect_free_barrier().await;
        }
        decision => decision,
    }
}
`),
    physicalStopDecisionStoreAdapter: withoutCfgTestItems(`
impl MachineWorkloadAuthorityStore for EngineWorkloadSagaStore {
    fn list_machine_workload_authority_from_engine() {}
}
`),
    physicalStopProvider: withoutCfgTestItems(`
struct DurableMachineStopBarrier {
    machine_name: String,
    forwarder_authority: MachineForwarderAuthority,
    epoch: MachineStopBarrierEpoch,
    state: DurableMachineStopBarrierState,
    digest: MachineStopBarrierDigest,
}
fn derive_digest() {
    let bytes = serde_json::to_vec(&DigestPayload {
        domain: STOP_BARRIER_DIGEST_DOMAIN,
        format_version: FORMAT_VERSION,
        machine_name: &self.machine_name,
        forwarder_authority: &self.forwarder_authority,
        epoch: self.epoch,
        state: self.state,
    })?;
    MachineStopBarrierDigest::new(format!("{:x}", Sha256::digest(bytes)))
        .map_err(evidence_error)
}
fn claim_machine_stop_barrier() {
    self.mutate_with_error(|body| {
        provider_instance();
        generation();
        body.stop_barriers.push(barrier);
        provider_witnesses(body, forwarder_authority);
    });
}
fn acquire_blocking() {
    let lock = self.journal.acquire_lock();
    let envelope = self.journal.load_envelope();
    envelope.body.stop_barriers;
    Ok(lock)
}
struct ConfirmedMachineDesireAdmissionPermit {
    _lock: ConfirmedMachinePublicationLock,
}
fn authenticate_workload_admission_absence(body, machine_name, forwarder_authority) {
    let barrier = body.stop_barriers.iter()
        .filter(|barrier| barrier.machine_name == machine_name)
        .max_by_key(|barrier| barrier.epoch)
        .filter(|barrier| !barrier.state.is_terminal());
    if barrier.forwarder_authority.provider_instance() != forwarder_authority.provider_instance() {
        return Err(Error::Crossed);
    }
    if barrier.forwarder_authority.generation() != forwarder_authority.generation() {
        return Err(Error::Stale);
    }
    Err(Error::Fenced)
}
fn authenticate_retirement_witness() {
    self.mutate(|body| {
        authenticate_workload_admission_absence(body, machine_name, authority);
        body.retirement_witnesses.push(candidate);
    });
}
fn authenticate_or_stage_restart_witness() {
    self.mutate(|body| {
        authenticate_workload_admission_absence(body, machine_name, authority);
        body.retirement_witnesses.push(candidate);
    });
}
fn authenticate_or_stage() {
    self.mutate(|body| {
        authenticate_workload_admission_absence(body, machine_name, authority);
        body.records.push(candidate);
    });
}
`),
    physicalProviderAdmissions: withoutCfgTestItems(`
fn validate_exact_phase() {
    self.publication_journal.authenticate_retirement_witness();
}
fn execute_exact_phase() {
    let validated = self.validate_exact_phase();
    self.phases.execute();
}
fn validate_publication() {
    self.validate_exact_phase();
}
fn authenticate_parent() {
    self.publication_journal.authenticate_or_stage();
}
fn execute_publication() {
    let validated = self.validate_publication();
    self.authenticate_parent(&validated);
    self.phases.execute(command, || self.publish(&validated));
}
fn publish() {
    reserve_parent_batch();
    commit_before_machine_api();
    provision_workload_phase();
}
fn validate_restart_phase() {
    self.publication_journal.authenticate_or_stage_restart_witness();
}
fn execute_restart_phase() {
    let validated = self.validate_restart_phase();
    self.restart_phases.execute();
}
`),
    physicalStopEffects: withoutCfgTestItems(`
pub(super) fn stop_machine(
    stop_authority: &HostMachineStopAuthority,
    authorization: ConfirmedMachineStopAuthorization,
) {
    authorization.barrier().machine_name();
    authorization.barrier().forwarder_authority();
    let stop_barrier = stop_authority.begin_physical_stop(&authorization);
    withdraw_machine_publications();
    withdraw_machine_ssh_port();
    stop_provider_machine();
    stop_pid();
    stop_exact_process();
    stop_authority.record_physical_stop_absent(&stop_barrier);
    super::write_json_file();
}
`),
    physicalStopStandalone: withoutCfgTestItems(`
async fn run_machine_stop() {
    let stop_authority = HostMachineStopAuthority::new();
    let authorization = stop_authority.authorize().await;
    stop_machine_with_layout_authorized(stop_authority, authorization);
}
async fn authorize_running_machine_os_restart() {
    let stop_authority = HostMachineStopAuthority::new();
    let authorization = stop_authority.authorize().await;
    AuthorizedMachineStop::new(stop_authority, authorization)
}
impl AuthorizedMachineStop {
    fn stop(self) {
        stop_machine(&self.stop_authority, self.authorization);
    }
}
`),
    physicalStopServer: withoutCfgTestItems(`
fn stop_machine<'a>() {
    let authorization = stop_authority.authorize().await;
    stop_machine_with_layout_authorized(stop_authority, authorization);
}
fn restart_machine<'a>() {
    let authorization = stop_authority.authorize().await;
    restart_machine_with_layout_authorized(stop_authority, authorization);
}
`),
    physicalStopOs: withoutCfgTestItems(`
fn restart_bootc_machine(authorized: &mut Option<AuthorizedMachineStop>) {
    let authorization = authorized.take().ok_or_else(missing_machine_stop_authority);
    authorization.stop();
    start_machine();
}
fn apply_machine_os_change(authorized: &mut Option<AuthorizedMachineStop>) {
    let authorization = authorized.take().ok_or_else(missing_machine_stop_authority);
    authorization.stop();
    config.guest.image_source = target_source;
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
      ["docs/private/plans", "nimbus-network-control-plane-plan.md"].join(
        "/",
      ),
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
      ["docs/private/plans", "nimbus-network-control-plane-plan.md"].join(
        "/",
      ),
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
