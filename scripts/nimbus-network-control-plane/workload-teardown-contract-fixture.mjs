import {
  maskNonCode,
  withoutCfgTestItems,
} from "./source-contract-scanner.mjs";

export const BEHAVIOR_TESTS = [
  "service_stop_persists_then_observes_complete_teardown_order",
  "sandbox_stop_persists_then_observes_complete_teardown_order",
  "force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects",
  "definition_delete_keeps_source_and_sessions_until_recorded_teardown",
  "definition_delete_cancels_and_joins_inflight_provision_before_removing_source",
  "late_provision_result_after_force_delete_is_retired_before_definition_removal",
  "compose_down_local_uses_engine_saga_and_compute_teardown",
  "compose_down_forwarded_uses_engine_saga_and_exact_machine_phases",
  "compose_down_unresolved_submission_makes_zero_provider_calls",
  "compose_down_replay_is_idempotent_and_reports_durable_outcome",
  "parent_publication_withdraws_before_guest_stop_and_releases_after_exact_absence",
  "crossed_machine_teardown_fences_fail_before_effects",
  "machine_stop_rejects_active_workload_saga_authority",
  "standalone_machine_stop_fails_closed_without_engine_drain_authority",
  "final_ingress_withdrawal_cancels_joins_closes_and_settles_exact_leases",
  "ingress_settlement_failure_retains_cleanup_and_blocks_recorded",
  "tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete",
  "failed_service_start_enters_durable_compensation_without_caller_stop",
  "failed_sandbox_start_enters_durable_compensation_without_caller_stop",
  "restart_result_is_settled_before_withdrawal_committed",
];

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
`),
    compute: withoutCfgTestItems(`
fn compare_and_swap_teardown_claim() {
    require_exact_revision();
    require_exact_generation();
    require_exact_desired_digest();
    reject_crossed_teardown_subject();
    commit_loaded();
}
struct ConfirmedWorkloadTeardownCommand {
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    attempt_id: WorkloadTeardownAttemptId,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    subject: WorkloadTeardownSubjects,
    provider_target: WorkloadTeardownProviderTarget,
}
impl ConfirmedWorkloadTeardownCommand {
    fn from_confirmed_cas_winner() {
        WorkloadSagaConfirmation::AppliedByThisCall;
        WorkloadTeardownCommandMode::Execute;
    }
}
fn apply_teardown_result() {
    authenticate_result_transition();
    authenticate_result_attempt();
    authenticate_result_dispatch_epoch();
    WorkloadTeardownCommandMode::Inspect;
    inspect_before_retry();
    same_attempt_next_dispatch_epoch();
}
fn drive_confirmed_teardown() {
    settle_issued_restart_before_teardown();
    retain_late_restart_result();
    enter_withdrawal_committed_after_restart_settlement();
    persist_withdrawal_committed();
    withdraw_exact_publication();
    drain_exact_execution();
    stop_exact_execution();
    detach_exact_network();
    release_exact_network();
    record_terminal_evidence();
}
fn submit_service_teardown() {}
fn submit_sandbox_teardown() {}
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
fn cancel_and_join_inflight_provision() {}
fn retire_late_provision_result() {}
fn finalize_service_definition_after_recorded() {}
fn project_recorded_service_teardown() {}
fn project_recorded_sandbox_teardown() {}
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
    cli: withoutCfgTestItems(`
fn compose_down_engine_workload_saga_store() { EngineWorkloadSagaStore; }
struct MachineApiWorkloadTeardownCommandEnvelope;
fn dispatch_machine_teardown_phase() {}
fn authenticate_machine_teardown_attempt_and_epoch() {}
fn withdraw_parent_publication_before_guest_stop() {}
fn release_parent_publication_after_guest_absence() {}
fn ensure_no_active_workload_sagas_before_machine_stop() {}
`),
    network: withoutCfgTestItems(`pub struct NetworkAttachmentId(String);`),
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
