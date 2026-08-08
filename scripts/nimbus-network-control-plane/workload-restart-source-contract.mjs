import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  maskNonCode,
  walkRust,
  withoutCfgTestItems,
} from "./source-contract-scanner.mjs";

// Ownership reason: this deep NNCV034 verifier keeps its production scan,
// green fixture, and sole-diagnostic mutations on one lexical contract. A
// split before 2,000 lines must move fixture data without adding a parser.

const AUDIT_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_AUDIT_CHECKPOINT ??
  "8723bc9a8ac27abc8ecbbd59d5f8d8d159e98cc1";
const R1_START_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R1_START_CHECKPOINT ??
  "6d8961bd6d4da819b2524128cb398e22e0a9382f";
const R1_COMPLETE_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R1_COMPLETE_CHECKPOINT ??
  "d117ba369eaf5acc5ede9ec3edad32a11ddfbeb2";
const R2_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R2_CHECKPOINT ?? R1_COMPLETE_CHECKPOINT;

const ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/store.rs",
  "crates/nimbus-workloads/src/store/tests.rs",
  "crates/nimbus-workloads/src/store/tests/restart_candidates.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/resource_provision/tests.rs",
  "crates/nimbus-compute/src/state.rs",
  "crates/nimbus-compute/src/workload_projection.rs",
  "crates/nimbus-compute/src/workload_provisioner.rs",
  "crates/nimbus-compute/src/services.rs",
  "crates/nimbus-server/src/workload_saga_store.rs",
  "crates/nimbus-server/src/workload_composition/tests.rs",
  "crates/nimbus-server/src/http/services.rs",
  "crates/nimbus-server/src/router.rs",
  "crates/nimbus-server/src/workload_composition.rs",
  "crates/nimbus-server/src/state.rs",
  "crates/nimbus-sandbox/src/inspection.rs",
  "crates/nimbus-sandbox/src/lib.rs",
  "crates/nimbus-sandbox/src/provider_command.rs",
  "crates/nimbus-machine/src/api.rs",
  "crates/nimbus-node/src/host_lifecycle.rs",
  "crates/nimbus-node/src/reconciler.rs",
  "crates/nimbus-node/src/direct_process.rs",
  "crates/nimbus-node/src/systemd_transient.rs",
  "crates/nimbus-system/src/inventory.rs",
  "packages/nimbus/src/selftest.mjs",
  "packages/nimbus/README.md",
  "scripts/verify-nimbus-network-control-plane.sh",
  "scripts/verify-nimbus-network-source-contract.mjs",
  "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
  "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const ALLOWED_PREFIXES = [
  "crates/nimbus-workloads/src/saga/",
  "crates/nimbus-compute/src/workload_saga/",
  "crates/nimbus-compute/src/workload_provisioner/",
  "crates/nimbus-server/src/workload_saga_store/",
  "crates/nimbus-server/src/http/services/",
  "crates/nimbus-sandbox/src/backends/container/runtime/",
  "crates/nimbus-sandbox/src/backends/krun/vm/",
  "crates/nimbus-sandbox/src/backends/oci/network/",
  "crates/nimbus-cli/src/compose/",
  "crates/nimbus-cli/src/machine/api/",
  "crates/nimbus-cli/src/machine/backend/",
  "crates/nimbus-cli/src/machine/stub/",
  "packages/nimbus/src/control-plane/",
  "packages/nimbus/tests/",
];

const R1_ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/saga/network/tests.rs",
  "crates/nimbus-workloads/src/saga/provision/tests.rs",
  "crates/nimbus-workloads/src/saga/provision.rs",
  "crates/nimbus-workloads/src/saga/restart.rs",
  "crates/nimbus-workloads/src/saga/restart/tests.rs",
  "crates/nimbus-workloads/src/saga/state.rs",
  "crates/nimbus-workloads/src/saga/state/provision.rs",
  "crates/nimbus-workloads/src/saga/state/restart.rs",
  "crates/nimbus-workloads/src/saga/tests.rs",
  "crates/nimbus-workloads/src/saga/tests/restart_state.rs",
  "crates/nimbus-server/src/workload_saga_store/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/schema.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/composition.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/durability.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/mod.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart.rs",
  "crates/nimbus-compute/src/workload_saga/recovery.rs",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh",
  "scripts/verify-nimbus-network-control-plane.sh",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const R2_ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/saga/restart.rs",
  "crates/nimbus-workloads/src/saga/restart/tests.rs",
  "crates/nimbus-workloads/src/saga/state.rs",
  "crates/nimbus-workloads/src/saga/state/restart.rs",
  "crates/nimbus-workloads/src/saga/tests.rs",
  "crates/nimbus-workloads/src/saga/tests/restart_state.rs",
  "crates/nimbus-workloads/src/store.rs",
  "crates/nimbus-workloads/src/store/tests.rs",
  "crates/nimbus-workloads/src/store/tests/restart_candidates.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/resource_provision/tests.rs",
  "crates/nimbus-compute/src/state.rs",
  "crates/nimbus-compute/src/workload_provisioner/tests.rs",
  "crates/nimbus-sandbox/src/lib.rs",
  "crates/nimbus-sandbox/src/provider_command.rs",
  "crates/nimbus-cli/src/machine/backend/provision/tests.rs",
  "crates/nimbus-server/src/workload_composition/tests.rs",
  "crates/nimbus-server/src/workload_saga_store.rs",
  "crates/nimbus-server/src/workload_saga_store/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/restart_candidates.rs",
  "crates/nimbus-server/src/workload_saga_store/schema.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/durability.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/ingress.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/mod.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/provision_driver_process.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart_candidates.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/store.rs",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "scripts/verify-nimbus-network-control-plane.sh",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const R2_ALLOWED_PREFIXES = [
  "crates/nimbus-compute/src/workload_saga/",
  "crates/nimbus-sandbox/src/backends/container/runtime/",
  "crates/nimbus-sandbox/src/backends/krun/vm/",
];

const DIAGNOSTICS = {
  vocabulary:
    "restart-contract/vocabulary: portable restart vocabulary is missing or open",
  nestedState:
    "restart-contract/nested-state: same-generation restart state or attempt identity is incomplete",
  admissionIdentity:
    "restart-contract/admission-identity: restart admission does not bind every identity and fence",
  reducer:
    "restart-contract/reducer: compute is not the sole CAS restart admission authority",
  command:
    "restart-contract/command: confirmed restart commands are forgeable or incompletely fenced",
  ambiguity:
    "restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry",
  schedule:
    "restart-contract/schedule: durable count, deadline, or deterministic-clock behavior is incomplete",
  withdrawal:
    "restart-contract/withdrawal: withdrawal or successor does not veto restart effects",
  readiness:
    "restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness",
  capabilities:
    "restart-contract/capabilities: small Container and Krun restart substitutions are incomplete",
  service:
    "restart-contract/service: service or SDK restart lacks fenced idempotent submission",
  watch:
    "restart-contract/watch: automatic restart is not a bounded compute-owned durable watch",
  node: "restart-contract/node: tenant workload node providers do not enforce Restart=No",
  machine:
    "restart-contract/machine: forwarded restart command drops a saga or inspection fence",
  scheduler:
    "restart-contract/scheduler: provider-local restart scheduling or obsolete deadline state remains",
  behavior:
    "restart-contract/behavior: required restart behavior and recovery proofs are incomplete",
  network:
    "restart-contract/network: nimbus-network gained restart effects or a god provider",
  paths:
    "restart-contract/paths: NNC6.4a changed a path outside the frozen allowlist",
  ledger:
    "restart-contract/ledger: plan and proof do not retain the NNC6.4a acceptance and review tokens",
};

function joinSources(sources) {
  return sources.map((entry) => entry.source).join("\n");
}

function normalizeRustEntries(root, directory) {
  return walkRust(path.join(root, directory)).map((entry) => ({
    file: path.relative(root, entry.file).split(path.sep).join("/"),
    source: entry.source,
  }));
}

function readText(root, relativePath, { lexical = false } = {}) {
  const absolute = path.join(root, relativePath);
  if (!fs.existsSync(absolute) || !fs.statSync(absolute).isFile()) return "";
  const source = fs.readFileSync(absolute, "utf8");
  return lexical ? maskNonCode(source) : source;
}

function collectTestSources(root, directories) {
  const sources = [];
  const visit = (directory) => {
    if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
      return;
    }
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (entry.isFile() && entry.name.endsWith(".rs")) {
        const relative = path
          .relative(root, absolute)
          .split(path.sep)
          .join("/");
        const source = fs.readFileSync(absolute, "utf8");
        if (
          relative.includes("/tests/") ||
          entry.name === "tests.rs" ||
          /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/u.test(source)
        ) {
          sources.push({ file: relative, source: maskNonCode(source) });
        }
      }
    }
  };
  for (const directory of directories) visit(path.join(root, directory));
  return sources;
}

function fixtureTestEntries(entries) {
  return Object.entries(entries).map(([file, source]) => ({
    file,
    source: maskNonCode(source),
  }));
}

function greenFixture() {
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
    saga_id: WorkloadSagaId,
    transition_id: WorkloadTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    request_id: WorkloadRestartRequestId,
    issuing_revision: WorkloadSagaRevision,
    inspection_version: SandboxInspectionVersion,
    provider_selection: WorkloadExecutionProviderId,
    result_transition_id: WorkloadTransitionId,
}
impl ConfirmedWorkloadRestartCommand {
    pub(crate) fn from_confirmation(confirmation: WorkloadSagaConfirmation) -> Result<Self, RestartDispatchError> {
        authenticate_exact_restart_confirmation(&confirmation)?;
        Self::try_from(confirmation)
    }
}
fn claim_restart_command() {
    match coordinator.compare_and_swap_restart_claim(command.issuing_revision(), command.dispatch_epoch())? {
        RestartClaimConfirmation::DirectWinner(confirmation) => execute_confirmed_restart(ConfirmedWorkloadRestartCommand::from_confirmation(confirmation)?),
        RestartClaimConfirmation::Replay(confirmation) => inspect_ambiguous_restart(ConfirmedWorkloadRestartCommand::from_confirmation(confirmation)?),
        RestartClaimConfirmation::InspectionRequired(confirmation) => inspect_ambiguous_restart(ConfirmedWorkloadRestartCommand::from_confirmation(confirmation)?),
    }
}
fn inspect_ambiguous_restart(command: &ConfirmedWorkloadRestartCommand) {
    match inspect_exact_restart_attempt(command)? {
        RestartEffectInspection::AuthenticatedAbsent => retry_after_authenticated_absence(command),
        RestartEffectInspection::InProgress => RestartDispatchOutcome::Wait,
        RestartEffectInspection::Succeeded(result) => apply_restart_result(command, result),
        RestartEffectInspection::DefiniteFailure(error) => stop_restart_dispatch(command, error),
    }
}
fn retry_after_authenticated_absence(command: &ConfirmedWorkloadRestartCommand) {
    let next_dispatch_epoch = command.dispatch_epoch().checked_next()?;
    coordinator.compare_and_swap_restart_claim(command.with_dispatch_epoch(next_dispatch_epoch))?;
}
fn apply_restart_result(command: &ConfirmedWorkloadRestartCommand, result: WorkloadRestartCommandResult) {
    authenticate_result_transition(command.result_transition_id(), result.transition_id())?;
    authenticate_result_attempt(command.attempt_id(), result.attempt_id())?;
    authenticate_result_dispatch_epoch(command.dispatch_epoch(), result.dispatch_epoch())?;
    coordinator.compare_and_swap_restart_result(command, result)?;
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_driver.rs":
      withoutCfgTestItems(`
fn drive_confirmed_restart(command: &ConfirmedWorkloadRestartCommand) {
    withdraw_publication(command)?;
    quiesce_execution(command)?;
    prepare_restart_retained_authority(command)?;
    attach_same_generation_network(command)?;
    require_same_attempt_attachment(command)?;
    require_same_attempt_pep(command)?;
    activate_execution_attempt(command)?;
    inspect_new_attempt_readiness(command)?;
    publish_new_attempt(command)?;
    observe_new_attempt_publication(command)?;
}
fn accept_restart_callback(command: &ConfirmedWorkloadRestartCommand, callback: RestartCallback) {
    require_exact_generation(command.generation(), callback.generation())?;
    require_exact_attempt(command.attempt_id(), callback.attempt_id())?;
    require_exact_transition(command.result_transition_id(), callback.transition_id())?;
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs":
      withoutCfgTestItems(`
trait RestartPublicationWithdrawalCapability: Send + Sync {
    fn withdraw(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectResult;
    fn inspect_withdrawal(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
trait WorkloadExecutionQuiescenceCapability: Send + Sync {
    fn quiesce(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectResult;
    fn inspect_quiescence(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
trait WorkloadRestartPreparationCapability: Send + Sync {
    fn prepare(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectResult;
    fn inspect_preparation(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
trait NetworkRestartAttachmentCapability: Send + Sync {
    fn attach(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectResult;
    fn inspect_attachment(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
trait WorkloadRestartActivationCapability: Send + Sync {
    fn activate(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectResult;
    fn inspect_activation(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
trait WorkloadRestartReadinessCapability: Send + Sync {
    fn inspect_readiness(&self, command: &ConfirmedWorkloadRestartCommand) -> RestartEffectInspection;
}
fn register_restart_capabilities(selection: WorkloadExecutionProviderId, capabilities: RestartCapabilities) {
    if self.providers.insert(selection, capabilities).is_some() {
        return Err(RestartRegistryError::DuplicateProviderSelection);
    }
}
fn resolve_restart_capabilities(selection: &WorkloadExecutionProviderId) {
    self.providers.get(selection).ok_or(RestartRegistryError::MissingProviderSelection)
}
`),
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs":
      withoutCfgTestItems(`
impl RestartPublicationWithdrawalCapability for ContainerRestartAdapter {}
impl WorkloadExecutionQuiescenceCapability for ContainerRestartAdapter {}
impl WorkloadRestartPreparationCapability for ContainerRestartAdapter {}
impl NetworkRestartAttachmentCapability for ContainerRestartAdapter {}
impl WorkloadRestartActivationCapability for ContainerRestartAdapter {}
impl WorkloadRestartReadinessCapability for ContainerRestartAdapter {}
impl RestartPublicationWithdrawalCapability for KrunRestartAdapter {}
impl WorkloadExecutionQuiescenceCapability for KrunRestartAdapter {}
impl WorkloadRestartPreparationCapability for KrunRestartAdapter {}
impl NetworkRestartAttachmentCapability for KrunRestartAdapter {}
impl WorkloadRestartActivationCapability for KrunRestartAdapter {}
impl WorkloadRestartReadinessCapability for KrunRestartAdapter {}
`),
    "crates/nimbus-compute/src/workload_saga/restart_watch.rs":
      withoutCfgTestItems(`
trait RestartClock: Send + Sync {
    fn now_unix_millis(&self) -> u64;
    fn wait_until(&self, deadline_unix_millis: u64, cancellation: &CancellationToken) -> RestartWait;
}
struct DurableRestartWatch {
    page_size: NonZeroUsize,
    clock: Arc<dyn RestartClock>,
    cancellation: CancellationToken,
}
fn load_durable_restart_page(watch: &DurableRestartWatch) {
    store.load_restart_candidates(watch.page_size)?;
}
fn bounded_restart_watch(watch: &DurableRestartWatch) {
    while !watch.cancellation.is_cancelled() {
        let page = load_durable_restart_page(watch)?;
        dispatch_each_due_epoch_once(page, watch.clock.now_unix_millis())?;
        watch.clock.wait_until(next_deadline(page), &watch.cancellation)?;
    }
}
fn read_only_exit_hint() -> RestartHint { RestartHint::ReadOnly }
`),
  };
  const testEntries = fixtureTestEntries({
    "crates/nimbus-compute/src/workload_saga/restart_decision/tests.rs": `
fn automatic_and_explicit_restart_use_same_reducer() {}
fn concurrent_triggers_admit_one_restart_epoch() {}
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
fn in_progress_never_retries() {}
fn definite_failure_stops_later_commands() {}
fn crossed_restart_result_is_rejected() {}
fn reused_skipped_and_crossed_dispatch_epochs_fail_closed() {}
`,
    "crates/nimbus-compute/src/workload_saga/restart_driver/tests.rs": `
fn publication_withdrawal_precedes_execution_quiescence() {}
fn restart_retained_detach_precedes_attachment() {}
fn activation_waits_for_same_generation_attachment_and_pep() {}
fn readiness_binds_the_new_execution_attempt() {}
fn publication_waits_for_new_attempt_readiness() {}
fn old_attempt_callback_is_rejected() {}
fn withdrawal_after_admission_vetoes_unissued_command() {}
fn withdrawal_after_ambiguous_effect_requires_inspection() {}
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
fn read_only_exit_hint_cannot_submit_or_execute_restart() {}
fn watch_cancellation_cancels_waiter_not_durable_work() {}
fn get_and_name_resolution_make_zero_restart_effects() {}
`,
    "__fixture__/legacy_tests.rs": `
fn same_generation_restart_keeps_desired_generation() {}
fn restart_recovery_eligibility_is_exhaustive() {}
fn explicit_restart_does_not_consume_automatic_count() {}
fn deadline_survives_clock_rollback_without_early_admission() {}
fn deadline_survives_engine_reopen() {}
fn count_survives_engine_reopen() {}
fn withdrawal_vetoes_unissued_restart() {}
fn successor_vetoes_restart_before_admission() {}
fn duplicate_service_request_returns_same_restart_epoch() {}
fn reconciler_rejects_provider_restart_and_duplicates_before_backend_validation() {}
fn machine_restart_wire_rejects_crossed_fences() {}
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
      Object.entries(files).map(([file, source]) => ({ file, source })),
    ),
    files,
    providers: withoutCfgTestItems(`
enum AttachmentDisposition { RestartRetained, Terminal }
fn retain_network_allocation() {}
fn retain_port_lease() {}
fn retain_attachment_identity() {}
fn retain_pep_authority() {}
`),
    server: withoutCfgTestItems(`
pub struct ServiceRestartRequest {
    source_generation: WorkloadGeneration,
    request_id: WorkloadRestartRequestId,
}
fn submit_service_restart() {}
`),
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
pub struct MachineRestartCommand {
    saga_id: WorkloadSagaId,
    generation: WorkloadGeneration,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    inspection_version: SandboxInspectionVersion,
    provider_selection: WorkloadExecutionProviderId,
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
  };
}

function productionSources(root) {
  const entries = new Map();
  const crateEntries = (name) => {
    const found = normalizeRustEntries(root, `crates/${name}/src`);
    for (const entry of found) entries.set(entry.file, entry.source);
    return found;
  };
  const workloads = crateEntries("nimbus-workloads");
  const compute = crateEntries("nimbus-compute");
  const providers = crateEntries("nimbus-sandbox");
  const server = crateEntries("nimbus-server");
  const node = crateEntries("nimbus-node");
  const machine = [
    ...crateEntries("nimbus-machine"),
    ...crateEntries("nimbus-cli"),
  ];
  const network = crateEntries("nimbus-network");
  const testEntries = collectTestSources(root, [
    "crates/nimbus-workloads/src",
    "crates/nimbus-workloads/tests",
    "crates/nimbus-compute/src",
    "crates/nimbus-compute/tests",
    "crates/nimbus-server/src",
    "crates/nimbus-server/tests",
    "crates/nimbus-sandbox/src",
    "crates/nimbus-sandbox/tests",
    "crates/nimbus-machine/src",
    "crates/nimbus-cli/src",
    "crates/nimbus-node/src",
  ]);
  return {
    workloads: joinSources(workloads),
    compute: joinSources(compute),
    providers: joinSources(providers),
    server: joinSources(server),
    codec: readText(
      root,
      "crates/nimbus-server/src/workload_saga_store/codec.rs",
    ),
    sdk: [
      readText(root, "packages/nimbus/src/control-plane/client.ts"),
      readText(root, "packages/nimbus/src/control-plane/routes.ts"),
      readText(root, "packages/nimbus/src/selftest.mjs"),
      readText(root, "packages/nimbus/README.md"),
    ].join("\n"),
    node: joinSources(node),
    machine: joinSources(machine),
    network: joinSources(network),
    tests: joinSources(testEntries),
    testEntries,
    files: Object.fromEntries(entries),
    plan: [
      readText(root, "docs/private/plans/nimbus-network-control-plane-plan.md"),
      readText(
        root,
        "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
      ),
    ].join("\n"),
    changedPaths: changedPathsSince(root, AUDIT_CHECKPOINT),
    r1ChangedPaths: changedPathsBetween(
      root,
      R1_START_CHECKPOINT,
      R1_COMPLETE_CHECKPOINT,
    ),
    r2ChangedPaths: changedPathsSince(root, R2_CHECKPOINT),
  };
}

function changedPathsSince(root, checkpoint) {
  const tracked = execFileSync(
    "git",
    ["diff", "--name-only", checkpoint, "--"],
    { cwd: root, encoding: "utf8" },
  );
  const untracked = execFileSync(
    "git",
    ["ls-files", "--others", "--exclude-standard"],
    { cwd: root, encoding: "utf8" },
  );
  return [...new Set(`${tracked}\n${untracked}`.split("\n").filter(Boolean))];
}

function changedPathsBetween(root, startCheckpoint, endCheckpoint) {
  const tracked = execFileSync(
    "git",
    ["diff", "--name-only", `${startCheckpoint}..${endCheckpoint}`, "--"],
    { cwd: root, encoding: "utf8" },
  );
  return [...new Set(tracked.split("\n").filter(Boolean))];
}

function replaceOnce(sources, area, before, after) {
  if (!sources[area].includes(before)) {
    throw new Error(`restart mutation target missing: ${area}:${before}`);
  }
  sources[area] = sources[area].replace(before, after);
  if (area === "tests") {
    const owner = sources.testEntries.find((entry) =>
      entry.source.includes(before),
    );
    if (!owner) {
      throw new Error(`restart test mutation owner missing: ${before}`);
    }
    owner.source = owner.source.replace(before, after);
  }
}

function replaceOnceInFile(sources, file, before, after) {
  const source = sources.files[file] ?? "";
  if (!source.includes(before)) {
    throw new Error(`restart file mutation target missing: ${file}:${before}`);
  }
  sources.files[file] = source.replace(before, after);
  sources.compute = Object.entries(sources.files)
    .filter(([candidate]) => candidate.startsWith("crates/nimbus-compute/"))
    .map(([, candidate]) => candidate)
    .join("\n");
}

function sourceAt(sources, file) {
  return sources.files[file] ?? "";
}

function testSourceAt(sources, file) {
  return sources.testEntries.find((entry) => entry.file === file)?.source ?? "";
}

function hasTestsAt(sources, file, testNames) {
  return hasAll(testSourceAt(sources, file), testNames);
}

function applyFixtureMutation(sources, mutation) {
  const admissionFields = {
    "missing-saga-id": "saga_id: WorkloadSagaId,",
    "missing-source": "source: WorkloadProvisionSourceEvidence,",
    "missing-generation": "generation: WorkloadGeneration,",
    "missing-desired-digest": "desired_digest: WorkloadDesiredDigest,",
    "missing-revision": "revision: WorkloadSagaRevision,",
    "missing-trigger": "trigger: WorkloadRestartTrigger,",
    "missing-inspection-version":
      "inspection_version: Option<WorkloadInspectionVersion>,",
    "missing-provider-selection":
      "provider_selection: WorkloadExecutionProviderId,",
    "missing-restart-epoch": "restart_epoch: WorkloadRestartEpoch,",
    "missing-policy-count": "policy_attempt_count: u32,",
    "missing-request-id": "request_id: WorkloadRestartRequestId,",
    "missing-attempt-id": "attempt_id: WorkloadExecutionAttemptId,",
  };
  if (mutation in admissionFields) {
    replaceOnce(sources, "workloads", admissionFields[mutation], "");
    return;
  }
  const decisionFile =
    "crates/nimbus-compute/src/workload_saga/restart_decision.rs";
  const dispatchFile =
    "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs";
  const driverFile =
    "crates/nimbus-compute/src/workload_saga/restart_driver.rs";
  const providerFile =
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs";
  const sandboxFile =
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs";
  const watchFile = "crates/nimbus-compute/src/workload_saga/restart_watch.rs";

  const fileMutations = {
    "separate-explicit-reducer": [
      decisionFile,
      "admit_explicit_restart(record, request)",
      "decide_explicit_restart_separately(record, request)",
    ],
    "stale-admission-revision": [
      decisionFile,
      "require_exact_revision(record.revision(), request.source_revision())?;",
      "let _stale_revision = request.source_revision();",
    ],
    "double-admission-winner": [
      decisionFile,
      "self.commit_loaded(Some(&current), candidate.clone()).await?;",
      "self.write_without_compare(Some(&current), candidate.clone()).await?;",
    ],
    "withdrawal-race-after-read": [
      decisionFile,
      "reject_withdrawal_or_successor(record)?;",
      "allow_withdrawal_race_after_read(record)?;",
    ],
    "missing-command-transition-id": [
      dispatchFile,
      "transition_id: WorkloadTransitionId,",
      "",
    ],
    "missing-command-desired-digest": [
      dispatchFile,
      "desired_digest: WorkloadDesiredDigest,",
      "",
    ],
    "missing-command-request-id": [
      dispatchFile,
      "request_id: WorkloadRestartRequestId,",
      "",
    ],
    "crossed-command-result": [
      dispatchFile,
      "authenticate_result_transition(command.result_transition_id(), result.transition_id())?;",
      "accept_crossed_result_transition(result.transition_id())?;",
    ],
    "execute-on-confirmed-replay": [
      dispatchFile,
      "RestartClaimConfirmation::Replay(confirmation) => inspect_ambiguous_restart",
      "RestartClaimConfirmation::Replay(confirmation) => execute_confirmed_restart",
    ],
    "ambiguity-infers-absence": [
      dispatchFile,
      "match inspect_exact_restart_attempt(command)?",
      "match RestartEffectInspection::AuthenticatedAbsent",
    ],
    "absence-retry-changes-attempt": [
      dispatchFile,
      "command.with_dispatch_epoch(next_dispatch_epoch)",
      "command.with_new_attempt(next_dispatch_epoch)",
    ],
    "absence-retry-reuses-dispatch-epoch": [
      dispatchFile,
      "command.dispatch_epoch().checked_next()?",
      "command.dispatch_epoch()",
    ],
    "absence-retry-skips-dispatch-epoch": [
      dispatchFile,
      "command.dispatch_epoch().checked_next()?",
      "command.dispatch_epoch().checked_add(2)?",
    ],
    "definite-failure-continues": [
      dispatchFile,
      "stop_restart_dispatch(command, error)",
      "retry_after_authenticated_absence(command)",
    ],
    "quiesce-before-publication-withdrawal": [
      driverFile,
      "withdraw_publication(command)?;\n    quiesce_execution(command)?;",
      "quiesce_execution(command)?;\n    withdraw_publication(command)?;",
    ],
    "restart-detach-releases-authority": [
      driverFile,
      "prepare_restart_retained_authority(command)?;",
      "release_terminal_authority(command)?;",
    ],
    "attachment-drops-attempt-fence": [
      driverFile,
      "require_same_attempt_attachment(command)?;",
      "accept_any_attachment_attempt(command)?;",
    ],
    "pep-drops-attempt-fence": [
      driverFile,
      "require_same_attempt_pep(command)?;",
      "accept_any_pep_attempt(command)?;",
    ],
    "publish-before-new-attempt-ready": [
      driverFile,
      "inspect_new_attempt_readiness(command)?;\n    publish_new_attempt(command)?;",
      "publish_new_attempt(command)?;\n    inspect_new_attempt_readiness(command)?;",
    ],
    "missing-container-restart-adapter": [
      sandboxFile,
      "impl WorkloadExecutionQuiescenceCapability for ContainerRestartAdapter {}",
      "",
    ],
    "missing-krun-restart-adapter": [
      sandboxFile,
      "impl WorkloadExecutionQuiescenceCapability for KrunRestartAdapter {}",
      "",
    ],
    "restart-registry-first-available-fallback": [
      providerFile,
      "self.providers.get(selection).ok_or",
      "self.providers.values().next().ok_or",
    ],
    "duplicate-restart-capability-registration": [
      providerFile,
      "if self.providers.insert(selection, capabilities).is_some()",
      "if self.providers.contains_key(&selection)",
    ],
    "unbounded-watch-page": [
      watchFile,
      "page_size: NonZeroUsize,",
      "page_size: usize,",
    ],
    "watch-busy-spin": [
      watchFile,
      "watch.clock.wait_until(next_deadline(page), &watch.cancellation)?;",
      "continue;",
    ],
    "watch-uses-system-clock": [
      watchFile,
      "watch.clock.now_unix_millis()",
      "SystemTime::now()",
    ],
    "watch-effects-from-read-only-hint": [
      watchFile,
      "RestartHint::ReadOnly",
      "execute_provider(); RestartHint::ReadOnly",
    ],
    "get-starts-restart-watch": [
      watchFile,
      "fn read_only_exit_hint()",
      "fn get_service() { bounded_restart_watch(); }\nfn read_only_exit_hint()",
    ],
    "watch-cancellation-drops-durable-work": [
      watchFile,
      "while !watch.cancellation.is_cancelled() {",
      "while !watch.cancellation.is_cancelled() { store.delete_restart()?;",
    ],
  };
  if (mutation in fileMutations) {
    replaceOnceInFile(sources, ...fileMutations[mutation]);
    return;
  }
  const replacements = {
    "crossed-attempt-id": [
      "workloads",
      "attempt_id: WorkloadExecutionAttemptId,",
      "attempt_id: WorkloadExecutionId,",
    ],
    "synthetic-generation": [
      "tests",
      "same_generation_restart_keeps_desired_generation",
      "restart_increments_desired_generation",
    ],
    "unknown-variant": [
      "workloads",
      "    ObservationPending,",
      "    ObservationPending,\n    ProviderManaged,",
    ],
    "reset-count": [
      "tests",
      "count_survives_engine_reopen",
      "count_resets_after_process_restart",
    ],
    "reset-deadline": [
      "tests",
      "deadline_survives_engine_reopen",
      "deadline_recomputed_from_process_start",
    ],
    "withdrawal-loses": [
      "tests",
      "withdrawal_vetoes_unissued_restart",
      "restart_ignores_withdrawal",
    ],
    "activate-before-readiness": [
      "tests",
      "activation_waits_for_same_generation_attachment_and_pep",
      "activation_precedes_attachment",
    ],
    "old-attempt-callback": [
      "tests",
      "old_attempt_callback_is_rejected",
      "old_attempt_callback_updates_projection",
    ],
    "god-provider": [
      "compute",
      "trait RestartPublicationWithdrawalCapability {}",
      "trait RestartProvider {}",
    ],
    "network-effect": [
      "network",
      "pub struct NetworkAttachmentId(String);",
      "pub struct NetworkAttachmentId(String); fn restart() { TcpListener::bind(); }",
    ],
    "local-stop-start": [
      "server",
      "fn submit_service_restart() {}",
      "fn submit_service_restart() { stop_service(); start_service(); }",
    ],
    "missing-api-idempotency": [
      "server",
      "request_id: WorkloadRestartRequestId,",
      "",
    ],
    "node-restart": [
      "node",
      "HostRestartPolicy::No",
      "HostRestartPolicy::OnFailure",
    ],
    "machine-fence-discard": [
      "machine",
      "inspection_version: SandboxInspectionVersion,",
      "",
    ],
    "backend-local-scheduler": [
      "providers",
      "fn retain_pep_authority() {}",
      "fn retain_pep_authority() {} struct Manifest { next_restart_at_millis: u64 }",
    ],
    "missing-behavior-proof": [
      "tests",
      "fresh_process_restart_reopens_engine",
      "fresh_process_restart_uses_handoff",
    ],
    "missing-ledger-token": ["plan", "A1-A20", "acceptance-pending"],
  };
  if (mutation === "unexpected-path") {
    sources.changedPaths.push("crates/nimbus-tenant/src/restart.rs");
  } else if (mutation === "forgeable-constructor") {
    replaceOnceInFile(
      sources,
      dispatchFile,
      "pub(crate) fn from_confirmation",
      "pub fn new",
    );
  } else if (mutation === "bypass-admission-cas") {
    replaceOnceInFile(
      sources,
      decisionFile,
      "compare_and_swap_restart_admission",
      "restart_without_admission",
    );
  } else if (mutation === "direct-ambiguity-retry") {
    replaceOnceInFile(
      sources,
      dispatchFile,
      "fn inspect_ambiguous_restart",
      "retry_ambiguous_restart",
    );
  } else if (mutation === "god-provider") {
    replaceOnceInFile(
      sources,
      providerFile,
      "trait RestartPublicationWithdrawalCapability",
      "trait RestartProvider",
    );
  } else if (mutation === "missing-restart-codec-field") {
    replaceOnce(sources, "codec", '"restartState"', '"removedRestartState"');
  } else if (mutation === "accept-unknown-restart-codec-field") {
    replaceOnce(
      sources,
      "codec",
      "validate_physical_shape",
      "accept_unknown_physical_shape",
    );
  } else if (mutation === "restart-transition-id-omits-state") {
    replaceOnce(
      sources,
      "workloads",
      "struct TransitionIdentityPayload { restart: &'a WorkloadRestartState }",
      "struct TransitionIdentityPayload { omitted_restart: () }",
    );
  } else if (mutation === "restart-phase-not-recoverable") {
    replaceOnce(
      sources,
      "tests",
      "restart_recovery_eligibility_is_exhaustive",
      "restart_phase_is_not_recoverable",
    );
  } else if (mutation === "explicit-consumes-automatic-count") {
    replaceOnce(
      sources,
      "tests",
      "explicit_restart_does_not_consume_automatic_count",
      "explicit_restart_consumes_automatic_count",
    );
  } else if (mutation === "r1-scope-broadening") {
    sources.r1ChangedPaths.push(
      "crates/nimbus-compute/src/workload_saga/restart.rs",
    );
  } else if (mutation in replacements) {
    replaceOnce(sources, ...replacements[mutation]);
  } else if (mutation) {
    throw new Error(`unknown restart contract mutation: ${mutation}`);
  }
}

function extractItem(source, marker) {
  const start = source.indexOf(marker);
  const open = source.indexOf("{", start);
  if (start < 0 || open < 0) return "";
  let depth = 0;
  for (let cursor = open; cursor < source.length; cursor += 1) {
    if (source[cursor] === "{") depth += 1;
    else if (source[cursor] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, cursor + 1);
  }
  return "";
}

function enumVariants(source, name) {
  return extractItem(source, `enum ${name}`)
    .split("\n")
    .map((line) => line.match(/^\s*([A-Z][A-Za-z0-9_]*)\s*(?:[,({])/u)?.[1])
    .filter(Boolean);
}

function hasAll(source, tokens) {
  return tokens.every((token) => source.includes(token));
}

function hasField(source, name, type) {
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedType = type.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(
    `^\\s*${escapedName}\\s*:\\s*${escapedType}\\s*,\\s*$`,
    "mu",
  ).test(source);
}

function appearsInOrder(source, tokens) {
  let cursor = 0;
  for (const token of tokens) {
    const found = source.indexOf(token, cursor);
    if (found < 0) return false;
    cursor = found + token.length;
  }
  return true;
}

function isAllowedPath(candidate) {
  return (
    ALLOWED_EXACT_PATHS.has(candidate) ||
    ALLOWED_PREFIXES.some((prefix) => candidate.startsWith(prefix))
  );
}

function isR1AllowedPath(candidate) {
  return R1_ALLOWED_EXACT_PATHS.has(candidate);
}

function isR2AllowedPath(candidate) {
  return (
    R2_ALLOWED_EXACT_PATHS.has(candidate) ||
    R2_ALLOWED_PREFIXES.some((prefix) => candidate.startsWith(prefix))
  );
}

export function verifyWorkloadRestartContract() {
  const fixture = process.env.NIMBUS_NETWORK_VERIFY_RESTART_FIXTURE === "1";
  const root = path.resolve(
    process.env.NIMBUS_NETWORK_VERIFY_RESTART_SCAN_ROOT ?? ".",
  );
  const sources = fixture ? greenFixture() : productionSources(root);
  if (fixture) {
    applyFixtureMutation(
      sources,
      process.env.NIMBUS_NETWORK_VERIFY_RESTART_MUTATION ?? "",
    );
  }

  const errors = [];
  const requireContract = (condition, diagnostic) => {
    if (!condition) errors.push(diagnostic);
  };

  const policyVariants = enumVariants(
    sources.workloads,
    "WorkloadRestartPolicy",
  );
  const phaseVariants = enumVariants(sources.workloads, "WorkloadRestartPhase");
  requireContract(
    hasAll(sources.workloads, [
      "WorkloadRestartTrigger",
      "WorkloadRestartEpoch",
      "WorkloadRestartRequestId",
      "WorkloadExecutionAttemptId",
      "WorkloadRestartDisposition",
    ]) &&
      policyVariants.join(" ") === "Never OnFailure Always" &&
      phaseVariants.join(" ") ===
        "Idle Requested PublicationWithdrawalPending ExecutionQuiescencePending Scheduled PreparationPending AttachmentPending ActivationPrerequisitePending ActivationPending ReadinessPending PublicationPending ObservationPending" &&
      hasAll(sources.codec, [
        '"restartPolicy"',
        '"restartState"',
        "validate_physical_shape",
      ]),
    DIAGNOSTICS.vocabulary,
  );

  requireContract(
    hasAll(sources.workloads, [
      "restart: WorkloadRestartState",
      "current_execution_attempt_id: WorkloadExecutionAttemptId",
    ]) &&
      hasAll(
        extractItem(sources.workloads, "struct TransitionIdentityPayload"),
        ["restart:", "WorkloadRestartState"],
      ) &&
      sources.tests.includes(
        "same_generation_restart_keeps_desired_generation",
      ) &&
      sources.tests.includes("restart_recovery_eligibility_is_exhaustive"),
    DIAGNOSTICS.nestedState,
  );

  const admission = extractItem(
    sources.workloads,
    "struct WorkloadRestartAdmission",
  );
  requireContract(
    hasAll(admission, [
      "saga_id: WorkloadSagaId",
      "source: WorkloadProvisionSourceEvidence",
      "generation: WorkloadGeneration",
      "desired_digest: WorkloadDesiredDigest",
      "revision: WorkloadSagaRevision",
      "trigger: WorkloadRestartTrigger",
      "inspection_version: Option<WorkloadInspectionVersion>",
      "provider_selection: WorkloadExecutionProviderId",
      "restart_epoch: WorkloadRestartEpoch",
      "policy_attempt_count: u32",
      "request_id: WorkloadRestartRequestId",
      "attempt_id: WorkloadExecutionAttemptId",
    ]),
    DIAGNOSTICS.admissionIdentity,
  );

  const restartDecisionSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_decision.rs",
  );
  const restartDecision = extractItem(
    restartDecisionSource,
    "fn decide_restart_admission",
  );
  const restartAdmissionCas = extractItem(
    restartDecisionSource,
    "fn compare_and_swap_restart_admission",
  );
  requireContract(
    hasAll(restartDecision, [
      "require_exact_revision",
      "require_exact_generation",
      "require_exact_desired_digest",
      "require_exact_inspection_version",
      "require_exact_provider_selection",
      "reject_withdrawal_or_successor",
      "WorkloadRestartTrigger::Automatic",
      "WorkloadRestartTrigger::Explicit",
      "admit_automatic_restart",
      "admit_explicit_restart",
    ]) &&
      hasAll(restartAdmissionCas, [
        "decide_restart_admission",
        "commit_loaded",
      ]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_decision/tests.rs",
        [
          "automatic_and_explicit_restart_use_same_reducer",
          "concurrent_triggers_admit_one_restart_epoch",
          "crossed_admission_fences_fail_before_cas",
          "withdrawal_winning_before_admission_vetoes_cas",
          "successor_winning_before_admission_vetoes_cas",
          "explicit_restart_does_not_increment_automatic_count",
          "deadline_not_due_returns_wait_without_effect",
          "cancellation_before_submission_makes_zero_store_and_provider_calls",
        ],
      ),
    DIAGNOSTICS.reducer,
  );

  const restartDispatchSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs",
  );
  const command = extractItem(
    restartDispatchSource,
    "struct ConfirmedWorkloadRestartCommand",
  );
  const commandImpl = extractItem(
    restartDispatchSource,
    "impl ConfirmedWorkloadRestartCommand",
  );
  const claimRestartCommand = extractItem(
    restartDispatchSource,
    "fn claim_restart_command",
  );
  const applyRestartResult = extractItem(
    restartDispatchSource,
    "fn apply_restart_result",
  );
  const commandFields = [
    ["saga_id", "WorkloadSagaId"],
    ["transition_id", "WorkloadTransitionId"],
    ["generation", "WorkloadGeneration"],
    ["desired_digest", "WorkloadDesiredDigest"],
    ["source_attempt_id", "WorkloadExecutionAttemptId"],
    ["attempt_id", "WorkloadExecutionAttemptId"],
    ["restart_epoch", "WorkloadRestartEpoch"],
    ["dispatch_epoch", "WorkloadRestartDispatchEpoch"],
    ["request_id", "WorkloadRestartRequestId"],
    ["issuing_revision", "WorkloadSagaRevision"],
    ["inspection_version", "SandboxInspectionVersion"],
    ["provider_selection", "WorkloadExecutionProviderId"],
    ["result_transition_id", "WorkloadTransitionId"],
  ];
  requireContract(
    commandFields.every(([name, type]) => hasField(command, name, type)) &&
      hasAll(commandImpl, [
        "pub(crate) fn from_confirmation",
        "authenticate_exact_restart_confirmation",
      ]) &&
      !/\bpub\s+fn\s+(?:new|from_confirmation)\b/u.test(commandImpl) &&
      claimRestartCommand.includes(
        "RestartClaimConfirmation::Replay(confirmation) => inspect_ambiguous_restart",
      ) &&
      hasAll(applyRestartResult, [
        "authenticate_result_transition",
        "authenticate_result_attempt",
        "authenticate_result_dispatch_epoch",
        "compare_and_swap_restart_result",
      ]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_dispatch/tests.rs",
        ["confirmed_restart_command_is_private_and_complete"],
      ),
    DIAGNOSTICS.command,
  );

  const inspectAmbiguousRestart = extractItem(
    restartDispatchSource,
    "fn inspect_ambiguous_restart",
  );
  const retryAfterAbsence = extractItem(
    restartDispatchSource,
    "fn retry_after_authenticated_absence",
  );
  requireContract(
    hasAll(claimRestartCommand, [
      "compare_and_swap_restart_claim",
      "DirectWinner",
      "Replay",
      "InspectionRequired",
    ]) &&
      hasAll(inspectAmbiguousRestart, [
        "inspect_exact_restart_attempt",
        "AuthenticatedAbsent",
        "InProgress",
        "Succeeded",
        "DefiniteFailure",
        "retry_after_authenticated_absence",
        "stop_restart_dispatch",
      ]) &&
      hasAll(retryAfterAbsence, [
        "checked_next",
        "with_dispatch_epoch",
        "compare_and_swap_restart_claim",
      ]) &&
      !hasAll(retryAfterAbsence, ["with_new_attempt", "checked_add(2)"]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_dispatch/tests.rs",
        [
          "direct_claim_cas_winner_alone_executes",
          "confirmed_replay_does_not_execute",
          "ambiguous_claim_cas_fresh_reads_before_effect",
          "crash_after_restart_effect_inspects_before_retry",
          "authenticated_absence_retries_same_attempt_at_next_dispatch_epoch",
          "in_progress_never_retries",
          "definite_failure_stops_later_commands",
          "crossed_restart_result_is_rejected",
          "reused_skipped_and_crossed_dispatch_epochs_fail_closed",
        ],
      ),
    DIAGNOSTICS.ambiguity,
  );

  requireContract(
    hasAll(sources.workloads, [
      "not_before_unix_millis",
      "completed_automatic_restart_count",
    ]) &&
      hasAll(sources.tests, [
        "explicit_restart_does_not_consume_automatic_count",
        "deadline_survives_clock_rollback_without_early_admission",
        "deadline_survives_engine_reopen",
        "count_survives_engine_reopen",
      ]),
    DIAGNOSTICS.schedule,
  );

  requireContract(
    hasAll(sources.tests, [
      "withdrawal_vetoes_unissued_restart",
      "successor_vetoes_restart_before_admission",
    ]),
    DIAGNOSTICS.withdrawal,
  );

  const restartDriverSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_driver.rs",
  );
  const driveConfirmedRestart = extractItem(
    restartDriverSource,
    "fn drive_confirmed_restart",
  );
  const acceptRestartCallback = extractItem(
    restartDriverSource,
    "fn accept_restart_callback",
  );
  requireContract(
    appearsInOrder(driveConfirmedRestart, [
      "withdraw_publication",
      "quiesce_execution",
      "prepare_restart_retained_authority",
      "attach_same_generation_network",
      "require_same_attempt_attachment",
      "require_same_attempt_pep",
      "activate_execution_attempt",
      "inspect_new_attempt_readiness",
      "publish_new_attempt",
      "observe_new_attempt_publication",
    ]) &&
      hasAll(acceptRestartCallback, [
        "require_exact_generation",
        "require_exact_attempt",
        "require_exact_transition",
      ]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_driver/tests.rs",
        [
          "publication_withdrawal_precedes_execution_quiescence",
          "restart_retained_detach_precedes_attachment",
          "activation_waits_for_same_generation_attachment_and_pep",
          "readiness_binds_the_new_execution_attempt",
          "publication_waits_for_new_attempt_readiness",
          "old_attempt_callback_is_rejected",
          "withdrawal_after_admission_vetoes_unissued_command",
          "withdrawal_after_ambiguous_effect_requires_inspection",
        ],
      ),
    DIAGNOSTICS.readiness,
  );

  const restartProviderSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs",
  );
  const restartSandboxSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs",
  );
  const restartCapabilityNames = [
    "RestartPublicationWithdrawalCapability",
    "WorkloadExecutionQuiescenceCapability",
    "WorkloadRestartPreparationCapability",
    "NetworkRestartAttachmentCapability",
    "WorkloadRestartActivationCapability",
    "WorkloadRestartReadinessCapability",
  ];
  const restartCapabilitiesAreObjectSafe = restartCapabilityNames.every(
    (name) => {
      const trait = extractItem(restartProviderSource, `trait ${name}`);
      return (
        hasAll(trait, [
          "Send + Sync",
          "&self",
          "&ConfirmedWorkloadRestartCommand",
        ]) && !/\bfn\s+\w+\s*</u.test(trait)
      );
    },
  );
  const registerRestartCapabilities = extractItem(
    restartProviderSource,
    "fn register_restart_capabilities",
  );
  const resolveRestartCapabilities = extractItem(
    restartProviderSource,
    "fn resolve_restart_capabilities",
  );
  requireContract(
    restartCapabilitiesAreObjectSafe &&
      restartCapabilityNames.every(
        (name) =>
          restartSandboxSource.includes(
            `impl ${name} for ContainerRestartAdapter`,
          ) &&
          restartSandboxSource.includes(`impl ${name} for KrunRestartAdapter`),
      ) &&
      hasAll(registerRestartCapabilities, [
        "insert(selection, capabilities).is_some()",
        "DuplicateProviderSelection",
      ]) &&
      hasAll(resolveRestartCapabilities, [
        "providers.get(selection)",
        "MissingProviderSelection",
      ]) &&
      !/\b(?:values|iter)\s*\(\s*\)\s*\.\s*(?:next|find)\b/u.test(
        resolveRestartCapabilities,
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_provider/tests.rs",
        [
          "restart_registry_rejects_duplicate_provider_selection",
          "restart_registry_has_no_first_available_fallback",
        ],
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_sandbox/tests.rs",
        [
          "container_restart_quiescence_capability_authenticates_command",
          "container_restart_preparation_retains_authority_and_binds_attempt",
          "krun_restart_quiescence_capability_authenticates_command",
          "krun_restart_preparation_retains_authority_and_binds_attempt",
          "real_restart_adapters_reject_crossed_provider_attempt_and_inspection",
          "concurrent_restart_dispatch_produces_one_provider_effect",
        ],
      ) &&
      !/\b(?:trait|struct|enum)\s+(?:God)?RestartProvider\b/u.test(
        sources.compute,
      ),
    DIAGNOSTICS.capabilities,
  );

  requireContract(
    hasAll(sources.server, [
      "source_generation: WorkloadGeneration",
      "request_id: WorkloadRestartRequestId",
      "submit_service_restart",
    ]) &&
      hasAll(sources.sdk, [
        "services.restart",
        "/restart",
        "sourceGeneration",
        "requestId",
      ]) &&
      sources.tests.includes(
        "duplicate_service_request_returns_same_restart_epoch",
      ) &&
      !/submit_service_restart[\s\S]{0,300}\bstop_service\b[\s\S]{0,200}\bstart_service\b/u.test(
        sources.server,
      ),
    DIAGNOSTICS.service,
  );

  const restartWatchSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_watch.rs",
  );
  const restartClock = extractItem(restartWatchSource, "trait RestartClock");
  const restartWatch = extractItem(
    restartWatchSource,
    "struct DurableRestartWatch",
  );
  const loadRestartPage = extractItem(
    restartWatchSource,
    "fn load_durable_restart_page",
  );
  const boundedRestartWatch = extractItem(
    restartWatchSource,
    "fn bounded_restart_watch",
  );
  const readOnlyExitHint = extractItem(
    restartWatchSource,
    "fn read_only_exit_hint",
  );
  requireContract(
    hasAll(restartClock, [
      "now_unix_millis",
      "wait_until",
      "CancellationToken",
    ]) &&
      hasAll(restartWatch, [
        "page_size: NonZeroUsize",
        "clock: Arc<dyn RestartClock>",
        "cancellation: CancellationToken",
      ]) &&
      hasAll(loadRestartPage, ["load_restart_candidates", "page_size"]) &&
      hasAll(boundedRestartWatch, [
        "load_durable_restart_page",
        "dispatch_each_due_epoch_once",
        "clock.now_unix_millis",
        "clock.wait_until",
        "cancellation.is_cancelled",
      ]) &&
      hasAll(readOnlyExitHint, ["RestartHint::ReadOnly"]) &&
      !/\b(?:SystemTime|Utc)::now\b|\b(?:execute|publish|attach|quiesce)_provider\b|\bdelete_restart\b|\bfn\s+(?:get|resolve_name)\w*\b[\s\S]{0,160}\bbounded_restart_watch\b/u.test(
        restartWatchSource,
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_watch/tests.rs",
        [
          "automatic_watch_loads_one_bounded_durable_page",
          "automatic_watch_does_not_busy_spin_before_deadline",
          "automatic_watch_dispatches_each_due_epoch_once",
          "read_only_exit_hint_cannot_submit_or_execute_restart",
          "watch_cancellation_cancels_waiter_not_durable_work",
          "get_and_name_resolution_make_zero_restart_effects",
        ],
      ),
    DIAGNOSTICS.watch,
  );

  const nodeLowering = extractItem(
    sources.node,
    "fn into_host_lifecycle_request",
  );
  const nodeRestartGuard = extractItem(
    sources.node,
    "fn ensure_external_restart_disabled",
  );
  requireContract(
    nodeLowering.includes("HostRestartPolicy::No") &&
      nodeRestartGuard.includes("!= HostRestartPolicy::No") &&
      `${sources.tests}\n${sources.node}`.includes(
        "reconciler_rejects_provider_restart_and_duplicates_before_backend_validation",
      ),
    DIAGNOSTICS.node,
  );

  const machineCommand = extractItem(
    sources.machine,
    "struct MachineRestartCommand",
  );
  requireContract(
    hasAll(machineCommand, [
      "saga_id: WorkloadSagaId",
      "generation: WorkloadGeneration",
      "attempt_id: WorkloadExecutionAttemptId",
      "restart_epoch: WorkloadRestartEpoch",
      "dispatch_epoch: WorkloadRestartDispatchEpoch",
      "inspection_version: SandboxInspectionVersion",
      "provider_selection: WorkloadExecutionProviderId",
    ]) && sources.tests.includes("machine_restart_wire_rejects_crossed_fences"),
    DIAGNOSTICS.machine,
  );

  requireContract(
    hasAll(sources.providers, [
      "RestartRetained",
      "retain_network_allocation",
      "retain_port_lease",
      "retain_attachment_identity",
      "retain_pep_authority",
    ]) &&
      !/\bnext_restart_at_millis\b|\bprovider_local_restart_scheduler\b/u.test(
        sources.providers,
      ),
    DIAGNOSTICS.scheduler,
  );

  requireContract(
    hasAll(sources.tests, [
      "fresh_process_restart_reopens_engine",
      "crash_after_restart_effect_inspects_before_retry",
      "cancellation_after_submission_preserves_durable_work",
      "compose_local_and_forwarded_restart_use_compute",
    ]),
    DIAGNOSTICS.behavior,
  );

  requireContract(
    !/\b(?:TcpListener|TcpStream|UdpSocket|SandboxBackend|RestartProvider)\b/u.test(
      sources.network,
    ),
    DIAGNOSTICS.network,
  );

  requireContract(
    sources.changedPaths.every(isAllowedPath) &&
      sources.r1ChangedPaths.every(isR1AllowedPath) &&
      sources.r2ChangedPaths.every(isR2AllowedPath),
    DIAGNOSTICS.paths,
  );

  requireContract(
    hasAll(sources.plan, [
      "NNC6.4a",
      "A1-A20",
      "NNCV034",
      "candidate-frozen",
      "Sol/xhigh/fast",
    ]),
    DIAGNOSTICS.ledger,
  );

  return errors;
}

export const workloadRestartDiagnostics = DIAGNOSTICS;
