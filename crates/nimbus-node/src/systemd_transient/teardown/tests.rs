use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::{fs, path::PathBuf};

use nimbus_core::Error;
use nimbus_workloads::{
    WorkloadTeardownAttempt, WorkloadTeardownAttemptInput, WorkloadTeardownClaim,
    WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
};
use serde_json::json;
use tempfile::{TempDir, tempdir};
use tokio::sync::Notify;

use super::super::*;
use super::teardown::{SystemdStopStage, SystemdTeardownOperationKey, set_stage};
use super::teardown_store::SystemdTeardownStore;
use crate::host_lifecycle::teardown_fail_before_tests::{
    Fixture, fixture, fixture_with_source_tag, input, inspection_fixture,
    retry_fixture_after_not_completed,
};
use crate::host_lifecycle::{HostActivationFence, HostProviderPlan};
use crate::{
    HostExecutable, HostExecutionDrainProvider, HostExecutionStopProvider,
    HostLifecycleBackendKind, HostLifecycleRequest, HostTeardownExecuteClaim,
    HostTeardownExecuteObservation, HostTeardownInspectClaim, HostTeardownInspectObservation,
    RuntimePoolTrustClass,
};

#[path = "tests/activation_barrier.rs"]
mod activation_barrier;

#[derive(Clone)]
struct TeardownFakeSystemdClient {
    status: Arc<Mutex<Option<SystemdUnitStatus>>>,
    start_effects: Arc<AtomicUsize>,
    pause_next_start: Arc<AtomicBool>,
    start_entered: Arc<Notify>,
    release_start: Arc<Notify>,
    unknown_next_start_submission: Arc<AtomicBool>,
    stop_effects: Arc<AtomicUsize>,
    lose_next_stop_response: Arc<AtomicBool>,
    fail_before_next_stop: Arc<AtomicBool>,
    accept_next_stop_without_terminal_result: Arc<AtomicBool>,
    unknown_next_stop_submission: Arc<AtomicBool>,
    post_submission_job: Arc<Mutex<Option<String>>>,
    terminal_response_job: Arc<Mutex<Option<String>>>,
    fail_next_stop_job: Arc<Mutex<Option<String>>>,
    corrupt_state_after_stop: Arc<Mutex<Option<PathBuf>>>,
}

impl TeardownFakeSystemdClient {
    fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(None)),
            start_effects: Arc::new(AtomicUsize::new(0)),
            pause_next_start: Arc::new(AtomicBool::new(false)),
            start_entered: Arc::new(Notify::new()),
            release_start: Arc::new(Notify::new()),
            unknown_next_start_submission: Arc::new(AtomicBool::new(false)),
            stop_effects: Arc::new(AtomicUsize::new(0)),
            lose_next_stop_response: Arc::new(AtomicBool::new(false)),
            fail_before_next_stop: Arc::new(AtomicBool::new(false)),
            accept_next_stop_without_terminal_result: Arc::new(AtomicBool::new(false)),
            unknown_next_stop_submission: Arc::new(AtomicBool::new(false)),
            post_submission_job: Arc::new(Mutex::new(None)),
            terminal_response_job: Arc::new(Mutex::new(None)),
            fail_next_stop_job: Arc::new(Mutex::new(None)),
            corrupt_state_after_stop: Arc::new(Mutex::new(None)),
        }
    }

    fn stop_effect_count(&self) -> usize {
        self.stop_effects.load(Ordering::SeqCst)
    }

    fn start_effect_count(&self) -> usize {
        self.start_effects.load(Ordering::SeqCst)
    }

    fn pause_next_start(&self) {
        self.pause_next_start.store(true, Ordering::SeqCst);
    }

    async fn wait_until_start_entered(&self) {
        self.start_entered.notified().await;
    }

    fn release_paused_start(&self) {
        self.release_start.notify_one();
    }

    fn unknown_next_start_submission(&self) {
        self.unknown_next_start_submission
            .store(true, Ordering::SeqCst);
    }

    fn clear_unit(&self) {
        *self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned") = None;
    }

    fn lose_next_stop_response(&self) {
        self.lose_next_stop_response.store(true, Ordering::SeqCst);
    }

    fn fail_before_next_stop(&self) {
        self.fail_before_next_stop.store(true, Ordering::SeqCst);
    }

    fn accept_next_stop_without_terminal_result(&self) {
        self.accept_next_stop_without_terminal_result
            .store(true, Ordering::SeqCst);
    }

    fn unknown_next_stop_submission(&self) {
        self.unknown_next_stop_submission
            .store(true, Ordering::SeqCst);
    }

    fn unknown_submission_with_terminal_job(&self, job_type: &str) {
        *self
            .post_submission_job
            .lock()
            .expect("fake post-submission job lock should not be poisoned") =
            Some(job_type.to_owned());
    }

    fn terminal_response_with_current_job(&self, job_type: &str) {
        *self
            .terminal_response_job
            .lock()
            .expect("fake terminal-response job lock should not be poisoned") =
            Some(job_type.to_owned());
    }

    fn corrupt_state_after_stop(&self, root: PathBuf) {
        *self
            .corrupt_state_after_stop
            .lock()
            .expect("fake corrupt-state path lock should not be poisoned") = Some(root);
    }

    fn fail_next_stop_job(&self, result: &str) {
        *self
            .fail_next_stop_job
            .lock()
            .expect("fake systemd terminal result lock should not be poisoned") =
            Some(result.to_owned());
    }

    fn replace_activation_fence(&self, fence: HostActivationFence) {
        let mut status = self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned");
        let retained = status.as_ref().expect("unit should be active");
        *status = Some(
            SystemdUnitStatus::new(
                retained.execution_id().clone(),
                retained.unit_name().clone(),
                retained.active_state(),
                retained.sub_state(),
            )
            .expect("crossed status should validate")
            .with_activation_fence(fence),
        );
    }

    fn set_current_job(&self, job: SystemdUnitJobStatus) {
        let mut status = self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned");
        let retained = status.as_ref().expect("unit should be active");
        let mut pending = SystemdUnitStatus::new(
            retained.execution_id().clone(),
            retained.unit_name().clone(),
            retained.active_state(),
            retained.sub_state(),
        )
        .expect("pending status should validate")
        .with_current_job(job);
        if let Some(fence) = retained.activation_fence().cloned() {
            pending = pending.with_activation_fence(fence);
        }
        *status = Some(pending);
    }

    fn set_terminal_with_current_job(&self, job: SystemdUnitJobStatus) {
        let mut status = self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned");
        let retained = status.as_ref().expect("unit should be active");
        let mut pending = SystemdUnitStatus::new(
            retained.execution_id().clone(),
            retained.unit_name().clone(),
            "inactive",
            "dead",
        )
        .expect("terminal status with a current job should validate")
        .with_current_job(job);
        if let Some(fence) = retained.activation_fence().cloned() {
            pending = pending.with_activation_fence(fence);
        }
        *status = Some(pending);
    }

    fn set_terminal_without_job(&self) {
        let mut status = self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned");
        let retained = status.as_ref().expect("unit should exist");
        let mut terminal = SystemdUnitStatus::new(
            retained.execution_id().clone(),
            retained.unit_name().clone(),
            "inactive",
            "dead",
        )
        .expect("terminal status should validate");
        if let Some(fence) = retained.activation_fence().cloned() {
            terminal = terminal.with_activation_fence(fence);
        }
        *status = Some(terminal);
    }

    fn set_provider_state_with_job(
        &self,
        active_state: &str,
        sub_state: &str,
        absent: bool,
        job: SystemdUnitJobStatus,
    ) {
        let mut status = self
            .status
            .lock()
            .expect("fake systemd status lock should not be poisoned");
        let retained = status.as_ref().expect("unit should exist");
        let mut changed = if absent {
            SystemdUnitStatus::explicitly_absent(
                retained.execution_id().clone(),
                retained.unit_name().clone(),
            )
            .expect("absent provider status should validate")
        } else {
            SystemdUnitStatus::new(
                retained.execution_id().clone(),
                retained.unit_name().clone(),
                active_state,
                sub_state,
            )
            .expect("provider status should validate")
        }
        .with_current_job(job);
        if !absent && let Some(fence) = retained.activation_fence().cloned() {
            changed = changed.with_activation_fence(fence);
        }
        *status = Some(changed);
    }
}

impl SystemdDbusClient for TeardownFakeSystemdClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        SystemdTransientCapabilities::available()
    }

    fn start_transient_unit<'a>(
        &'a self,
        request: SystemdStartTransientUnitRequest,
    ) -> crate::HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            self.start_effects.fetch_add(1, Ordering::SeqCst);
            if self.pause_next_start.swap(false, Ordering::SeqCst) {
                self.start_entered.notify_one();
                self.release_start.notified().await;
            }
            if self
                .unknown_next_start_submission
                .swap(false, Ordering::SeqCst)
            {
                return Err(Error::Internal(
                    "fake systemd start submission outcome is unknown".to_owned(),
                ));
            }
            let status = SystemdUnitStatus::for_start_request(&request, "active", "running")?;
            *self
                .status
                .lock()
                .expect("fake systemd status lock should not be poisoned") = Some(status);
            SystemdStartTransientUnitResponse::new(
                request.unit_name().clone(),
                "/org/freedesktop/systemd1/job/501",
            )
        })
    }

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> crate::HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            self.stop_effects.fetch_add(1, Ordering::SeqCst);
            let mut status = self
                .status
                .lock()
                .expect("fake systemd status lock should not be poisoned");
            let retained = status
                .as_ref()
                .ok_or_else(|| Error::NotFound("fake systemd unit is absent".to_owned()))?;
            let mut stopped = SystemdUnitStatus::new(
                request.execution_id().clone(),
                request.unit_name().clone(),
                "inactive",
                "dead",
            )?;
            if let Some(fence) = retained.activation_fence().cloned() {
                stopped = stopped.with_activation_fence(fence);
            }
            *status = Some(stopped.clone());
            if self.lose_next_stop_response.swap(false, Ordering::SeqCst) {
                return Err(Error::Internal(
                    "fake lost StopUnit response after submission".to_owned(),
                ));
            }
            SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/502", stopped)
        })
    }

    fn stop_unit_exact<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> crate::HostLifecycleFuture<'a, SystemdStopUnitSubmission> {
        Box::pin(async move {
            if self.fail_before_next_stop.swap(false, Ordering::SeqCst) {
                return Ok(SystemdStopUnitSubmission::pre_call_failure(
                    "fake failure before StopUnit",
                ));
            }
            if self
                .accept_next_stop_without_terminal_result
                .swap(false, Ordering::SeqCst)
            {
                self.stop_effects.fetch_add(1, Ordering::SeqCst);
                let mut status = self
                    .status
                    .lock()
                    .expect("fake systemd status lock should not be poisoned");
                let retained = status
                    .as_ref()
                    .ok_or_else(|| Error::NotFound("fake systemd unit is absent".to_owned()))?;
                let mut pending = SystemdUnitStatus::new(
                    request.execution_id().clone(),
                    request.unit_name().clone(),
                    "active",
                    "running",
                )?
                .with_current_job(SystemdUnitJobStatus::new(
                    503,
                    "/org/freedesktop/systemd1/job/503",
                    "stop",
                    "running",
                )?);
                if let Some(fence) = retained.activation_fence().cloned() {
                    pending = pending.with_activation_fence(fence);
                }
                *status = Some(pending);
                return SystemdStopUnitSubmission::accepted_job_incomplete(
                    "/org/freedesktop/systemd1/job/503",
                    "fake lost JobRemoved result",
                );
            }
            if self
                .unknown_next_stop_submission
                .swap(false, Ordering::SeqCst)
            {
                self.stop_effects.fetch_add(1, Ordering::SeqCst);
                return Ok(SystemdStopUnitSubmission::unknown_submission(
                    "fake unknown StopUnit submission",
                ));
            }
            if let Some(job_type) = self
                .post_submission_job
                .lock()
                .expect("fake post-submission job lock should not be poisoned")
                .take()
            {
                self.stop_effects.fetch_add(1, Ordering::SeqCst);
                self.set_provider_state_with_job(
                    "inactive",
                    "dead",
                    false,
                    SystemdUnitJobStatus::new(
                        509,
                        "/org/freedesktop/systemd1/job/509",
                        job_type,
                        "running",
                    )?,
                );
                return Ok(SystemdStopUnitSubmission::unknown_submission(
                    "fake unknown submission with a current job",
                ));
            }
            if let Some(job_type) = self
                .terminal_response_job
                .lock()
                .expect("fake terminal-response job lock should not be poisoned")
                .take()
            {
                self.stop_effects.fetch_add(1, Ordering::SeqCst);
                self.set_provider_state_with_job(
                    "inactive",
                    "dead",
                    false,
                    SystemdUnitJobStatus::new(
                        510,
                        "/org/freedesktop/systemd1/job/510",
                        job_type,
                        "running",
                    )?,
                );
                let status = self
                    .status
                    .lock()
                    .expect("fake systemd status lock should not be poisoned")
                    .clone()
                    .expect("terminal response status should exist");
                return Ok(SystemdStopUnitSubmission::Terminal(Box::new(
                    SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/510", status)?,
                )));
            }
            let terminal_failure = self
                .fail_next_stop_job
                .lock()
                .expect("fake systemd terminal result lock should not be poisoned")
                .take();
            if let Some(result) = terminal_failure {
                self.stop_effects.fetch_add(1, Ordering::SeqCst);
                return SystemdStopUnitSubmission::terminal_failure(
                    "/org/freedesktop/systemd1/job/504",
                    result,
                );
            }
            let submission = match self.stop_unit(request).await {
                Ok(response) => SystemdStopUnitSubmission::Terminal(Box::new(response)),
                Err(error) => SystemdStopUnitSubmission::unknown_submission(error.to_string()),
            };
            if let Some(root) = self
                .corrupt_state_after_stop
                .lock()
                .expect("fake corrupt-state path lock should not be poisoned")
                .take()
            {
                fs::write(
                    root.join("systemd-teardown-state.json"),
                    b"{corrupt-after-effect",
                )
                .expect("fake post-effect state corruption should write");
            }
            Ok(submission)
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> crate::HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            self.status
                .lock()
                .expect("fake systemd status lock should not be poisoned")
                .clone()
                .map(Ok)
                .unwrap_or_else(|| SystemdUnitStatus::absent_for_inspect_request(&request))
        })
    }
}

fn request() -> HostLifecycleRequest {
    HostLifecycleRequest::new(
        HostLifecycleBackendKind::SystemdTransientUnit,
        HostExecutable::trusted("/bin/nimbus-systemd-teardown-test")
            .expect("test executable should validate"),
    )
    .with_args(["--teardown-test"])
    .expect("test args should validate")
    .with_trust_class(RuntimePoolTrustClass::SingleTenant)
}

async fn activate(
    backend: &SystemdTransientUnitBackend<TeardownFakeSystemdClient>,
    fixture: &Fixture,
) {
    activate_without_drain(backend, fixture).await;
    if fixture.claim.attempt().step() == WorkloadTeardownStep::StopExecution
        && backend.teardown_store().is_ok()
    {
        let drain = prior_drain_fixture(fixture);
        let claim =
            HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
                .expect("prior drain claim should validate");
        assert!(matches!(
            backend.execute_drain(claim).await,
            HostTeardownExecuteObservation::Succeeded(_)
        ));
    }
}

async fn activate_without_drain(
    backend: &SystemdTransientUnitBackend<TeardownFakeSystemdClient>,
    fixture: &Fixture,
) {
    backend
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect("fixture systemd unit should activate");
}

fn prior_drain_fixture(fixture: &Fixture) -> Fixture {
    let receipt = fixture
        .prior_receipt_prefix
        .receipt_for(WorkloadTeardownStep::DrainExecution)
        .expect("stop fixture should retain its exact prior drain receipt");
    let mut drain = fixture.clone();
    drain.claim = receipt.claim().clone();
    drain.confirmed_revision = drain.claim.claimed_revision();
    drain.confirmed_transition_id = format!("wst_{}", "0d".repeat(32))
        .parse()
        .expect("prior drain confirmation should validate");
    drain.prior_receipt_prefix = serde_json::from_value(json!({ "receipts": [] }))
        .expect("prior drain prefix should validate");
    drain
}

fn activation_fence(fixture: &Fixture) -> HostActivationFence {
    HostProviderPlan::from_execution(&fixture.execution, &fixture.activation_claim, request())
        .expect("fixture provider plan should validate")
        .activation_fence()
        .expect("fixture should retain an activation fence")
        .clone()
}

fn durable_backend(
    client: TeardownFakeSystemdClient,
) -> (
    TempDir,
    SystemdTransientUnitBackend<TeardownFakeSystemdClient>,
) {
    let root = tempdir().expect("temporary systemd teardown state root should open");
    let backend = SystemdTransientUnitBackend::new_with_teardown_state_root(client, root.path())
        .expect("durable systemd teardown backend should open");
    (root, backend)
}

fn distinct_teardown_attempt(fixture: &Fixture, transition_tag: &str) -> Fixture {
    let current = fixture.claim.attempt();
    let attempt = WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
        key: current.key().clone(),
        saga_id: current.saga_id().clone(),
        issuing_revision: current.issuing_revision(),
        issuing_transition_id: format!("wst_{}", transition_tag.repeat(32))
            .parse()
            .expect("distinct issuing transition should validate"),
        generation: current.generation(),
        desired_digest: current.desired_digest(),
        required_node: current.required_node().clone(),
        source_digest: current.source_digest(),
        execution_provider_id: current.execution_provider_id().clone(),
        network_plan_digest: current.network_plan_digest(),
        selection_evidence: current.selection_evidence().cloned(),
        cause: current.cause().clone(),
        successor_fence: current.successor_fence(),
        source_phase: current.source_phase(),
        target_phase: current.target_phase(),
        step: current.step(),
        subjects: current.subjects().clone(),
    })
    .expect("distinct teardown attempt should validate");
    let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
        .expect("provider target should validate")
        .expect("execution teardown should select a provider");
    let claim: WorkloadTeardownClaim = serde_json::from_value(json!({
        "attempt": attempt,
        "claimedRevision": fixture.claim.claimed_revision(),
        "dispatchEpoch": fixture.claim.dispatch_epoch(),
        "providerTarget": provider_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("distinct teardown claim should validate");
    let mut distinct = fixture.clone();
    distinct.claim = claim;
    distinct.confirmed_transition_id = format!("wst_{}", "7d".repeat(32))
        .parse()
        .expect("distinct confirmation should validate");
    distinct
}

#[test]
fn systemd_absent_status_is_bound_to_the_inspect_request() {
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    let request =
        SystemdInspectUnitRequest::for_execution(fixture.execution.execution_id().clone())
            .expect("inspection request should validate");

    let status = SystemdUnitStatus::absent_for_inspect_request(&request)
        .expect("absent inspection status should validate");

    assert_eq!(status.execution_id(), request.execution_id());
    assert_eq!(status.unit_name(), request.unit_name());
    assert_eq!(status.active_state(), "inactive");
    assert_eq!(status.sub_state(), "dead");
    assert!(status.is_absent());
}

#[test]
fn systemd_operation_key_separates_restart_attempts_and_reopens_exactly() {
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    let execution_id = fixture.execution.execution_id().clone();
    let teardown_attempt_id = fixture.claim.attempt().attempt_id().clone();
    let first = SystemdTeardownOperationKey::from_parts(
        execution_id.clone(),
        fixture.execution.attempt_id().clone(),
        teardown_attempt_id.clone(),
    );
    let restarted = SystemdTeardownOperationKey::from_parts(
        execution_id.clone(),
        nimbus_workloads::WorkloadExecutionAttemptId::for_execution(
            &execution_id,
            nimbus_workloads::WorkloadRestartEpoch::new(1),
        ),
        teardown_attempt_id,
    );

    assert_ne!(first, restarted);
    let encoded = serde_json::to_string(&first).expect("operation key should serialize");
    let reopened: SystemdTeardownOperationKey =
        serde_json::from_str(&encoded).expect("operation key should reopen");
    assert_eq!(reopened, first);
}

#[tokio::test]
async fn systemd_crossed_teardown_fails_before_stop_unit() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &fixture).await;
    let crossed = fixture_with_source_tag(WorkloadTeardownStep::StopExecution, "crossed");
    client.replace_activation_fence(activation_fence(&crossed));
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert!(matches!(
        backend.execute_stop(claim).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 0);
}

#[tokio::test]
async fn systemd_crossed_confirmation_cannot_adopt_prior_success() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    let original =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("original stop claim should validate");
    assert!(matches!(
        backend.execute_stop(original.clone()).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));

    let mut crossed = fixture(WorkloadTeardownStep::StopExecution);
    crossed.confirmed_transition_id = format!("wst_{}", "7c".repeat(32))
        .parse()
        .expect("crossed confirmation should validate");
    let crossed =
        HostTeardownExecuteClaim::new(input(&crossed, WorkloadTeardownCommandMode::Execute))
            .expect("internally consistent crossed claim should validate");

    assert!(matches!(
        backend.execute_stop(crossed).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_distinct_teardown_attempts_do_not_share_receipts() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    let original =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("original stop claim should validate");
    assert!(matches!(
        backend.execute_stop(original.clone()).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));

    let distinct = distinct_teardown_attempt(&primary, "7c");
    let distinct =
        HostTeardownExecuteClaim::new(input(&distinct, WorkloadTeardownCommandMode::Execute))
            .expect("distinct stop claim should validate");
    assert!(matches!(
        backend.execute_stop(distinct).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert!(matches!(
        backend.execute_stop(original).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_independent_backends_submit_one_exact_stop() {
    let client = TeardownFakeSystemdClient::new();
    let state = tempdir().expect("temporary systemd teardown state root should open");
    let first =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("first durable systemd backend should open");
    let second =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("second durable systemd backend should open");
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&first, &primary).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    let (first_observation, second_observation) = tokio::join!(
        first.execute_stop(claim.clone()),
        second.execute_stop(claim)
    );
    assert!(matches!(
        first_observation,
        HostTeardownExecuteObservation::Succeeded(_) | HostTeardownExecuteObservation::Ambiguous
    ));
    assert!(matches!(
        second_observation,
        HostTeardownExecuteObservation::Succeeded(_) | HostTeardownExecuteObservation::Ambiguous
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_pending_stop_job_is_never_not_completed() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    let inspection = inspection_fixture(&primary, "6a");
    let claim =
        HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
            .expect("inspection claim should validate");
    client.set_current_job(
        SystemdUnitJobStatus::new(502, "/org/freedesktop/systemd1/job/502", "stop", "running")
            .expect("current stop job should validate"),
    );

    assert!(matches!(
        backend.inspect_stop(claim).await,
        HostTeardownInspectObservation::InProgress(_)
    ));
    assert_eq!(
        client.stop_effect_count(),
        0,
        "inspection must not call StopUnit"
    );
}

#[tokio::test]
async fn systemd_exact_drain_never_calls_stop_unit() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    activate(&backend, &fixture).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");

    assert!(matches!(
        backend.execute_drain(claim).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 0, "drain cannot call StopUnit");
}

#[tokio::test]
async fn systemd_lost_stop_response_converges_without_duplicate_effect() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &fixture).await;
    client.lose_next_stop_response();
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    let first = backend.execute_stop(claim.clone()).await;
    let replay = backend.execute_stop(claim).await;
    assert_eq!(first, replay);
    assert!(matches!(
        first,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_pre_call_failure_is_safe_to_retry_once() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.fail_before_next_stop();
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert_eq!(
        backend.execute_stop(claim.clone()).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 0);

    let inspection_fixture = inspection_fixture(&primary, "6b");
    let inspection = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("inspection claim should validate");
    let retry_evidence = inspection.canonical_evidence("nimbus.node.systemd.stop.pre-call.v1");
    assert!(matches!(
        backend.inspect_stop(inspection).await,
        HostTeardownInspectObservation::NotCompleted(_)
    ));

    assert_eq!(
        backend.execute_stop(claim.clone()).await,
        HostTeardownExecuteObservation::Ambiguous,
        "the original epoch cannot resubmit after pre-call ambiguity"
    );
    assert_eq!(client.stop_effect_count(), 0);
    let retry_fixture =
        retry_fixture_after_not_completed(&primary, &inspection_fixture, retry_evidence, "6c");
    let retry =
        HostTeardownExecuteClaim::new(input(&retry_fixture, WorkloadTeardownCommandMode::Execute))
            .expect("next-epoch retry claim should validate");
    assert!(matches!(
        backend.execute_stop(retry).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_pre_call_receipt_survives_backend_reopen() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.fail_before_next_stop();
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");
    assert_eq!(
        backend.execute_stop(claim).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 0);

    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd teardown backend should reopen");
    let inspection_fixture = inspection_fixture(&primary, "7a");
    let inspection = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("inspection claim should validate");
    let retry_evidence = inspection.canonical_evidence("nimbus.node.systemd.stop.pre-call.v1");
    assert!(matches!(
        reopened.inspect_stop(inspection).await,
        HostTeardownInspectObservation::NotCompleted(_)
    ));
    assert_eq!(client.stop_effect_count(), 0);

    let retry_fixture =
        retry_fixture_after_not_completed(&primary, &inspection_fixture, retry_evidence, "7c");
    let retry =
        HostTeardownExecuteClaim::new(input(&retry_fixture, WorkloadTeardownCommandMode::Execute))
            .expect("next-epoch retry claim should validate");
    assert!(matches!(
        reopened.execute_stop(retry).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_unknown_submission_remains_ambiguous_after_reopen() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.unknown_next_stop_submission();
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");
    assert_eq!(
        backend.execute_stop(claim.clone()).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 1);

    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd teardown backend should reopen");
    assert_eq!(
        reopened.execute_stop(claim).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    let inspection_fixture = inspection_fixture(&primary, "7e");
    let inspection = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("inspection claim should validate");
    assert_eq!(
        reopened.inspect_stop(inspection).await,
        HostTeardownInspectObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_post_submission_reconciliation_checks_current_job_first() {
    for job_type in ["stop", "start", "restart"] {
        let client = TeardownFakeSystemdClient::new();
        let (_state, backend) = durable_backend(client.clone());
        let primary = fixture(WorkloadTeardownStep::StopExecution);
        activate(&backend, &primary).await;
        client.unknown_submission_with_terminal_job(job_type);
        let claim =
            HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
                .expect("teardown claim should validate");

        assert_eq!(
            backend.execute_stop(claim).await,
            HostTeardownExecuteObservation::Ambiguous
        );
        assert_eq!(client.stop_effect_count(), 1);
        let inspection_fixture = inspection_fixture(&primary, "8c");
        let inspection = HostTeardownInspectClaim::new(input(
            &inspection_fixture,
            WorkloadTeardownCommandMode::Inspect,
        ))
        .expect("inspection claim should validate");
        let observation = backend.inspect_stop(inspection).await;
        if job_type == "stop" {
            assert!(matches!(
                observation,
                HostTeardownInspectObservation::InProgress(_)
            ));
        } else {
            assert_eq!(observation, HostTeardownInspectObservation::Ambiguous);
        }
    }
}

#[tokio::test]
async fn systemd_terminal_response_with_current_job_is_not_adopted() {
    for job_type in ["stop", "start", "restart"] {
        let client = TeardownFakeSystemdClient::new();
        let (_state, backend) = durable_backend(client.clone());
        let primary = fixture(WorkloadTeardownStep::StopExecution);
        activate(&backend, &primary).await;
        client.terminal_response_with_current_job(job_type);
        let claim =
            HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
                .expect("teardown claim should validate");

        assert_eq!(
            backend.execute_stop(claim).await,
            HostTeardownExecuteObservation::Ambiguous
        );
        assert_eq!(client.stop_effect_count(), 1);
    }
}

#[tokio::test]
async fn systemd_terminal_status_with_start_job_is_not_terminal_success() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.set_terminal_with_current_job(
        SystemdUnitJobStatus::new(507, "/org/freedesktop/systemd1/job/507", "start", "running")
            .expect("current start job should validate"),
    );
    let inspection_fixture = inspection_fixture(&primary, "7b");
    let inspection = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("inspection claim should validate");

    assert_eq!(
        backend.inspect_stop(inspection).await,
        HostTeardownInspectObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 0);
}

#[tokio::test]
async fn systemd_current_job_precedes_terminal_state_across_exact_paths() {
    let provider_states = [
        ("inactive", "dead", false),
        ("failed", "failed", false),
        ("inactive", "dead", true),
    ];
    let job_types = ["stop", "start", "restart"];

    for (active_state, sub_state, absent) in provider_states {
        for job_type in job_types {
            let job = || {
                SystemdUnitJobStatus::new(
                    508,
                    "/org/freedesktop/systemd1/job/508",
                    job_type,
                    "running",
                )
                .expect("current job should validate")
            };

            let drain_client = TeardownFakeSystemdClient::new();
            let (_drain_state, drain_backend) = durable_backend(drain_client.clone());
            let drain = fixture(WorkloadTeardownStep::DrainExecution);
            activate(&drain_backend, &drain).await;
            drain_client.set_provider_state_with_job(active_state, sub_state, absent, job());
            let drain_execute =
                HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
                    .expect("drain execute claim should validate");
            assert_eq!(
                drain_backend.execute_drain(drain_execute).await,
                HostTeardownExecuteObservation::Ambiguous
            );
            let drain_inspection = inspection_fixture(&drain, "8a");
            let drain_inspection = HostTeardownInspectClaim::new(input(
                &drain_inspection,
                WorkloadTeardownCommandMode::Inspect,
            ))
            .expect("drain inspect claim should validate");
            let drain_observation = drain_backend.inspect_drain(drain_inspection).await;
            assert_eq!(
                matches!(
                    &drain_observation,
                    HostTeardownInspectObservation::InProgress(_)
                ),
                job_type == "stop"
            );
            if job_type != "stop" {
                assert_eq!(drain_observation, HostTeardownInspectObservation::Ambiguous);
            }

            let stop_client = TeardownFakeSystemdClient::new();
            let (_stop_state, stop_backend) = durable_backend(stop_client.clone());
            let stop = fixture(WorkloadTeardownStep::StopExecution);
            activate(&stop_backend, &stop).await;
            stop_client.set_provider_state_with_job(active_state, sub_state, absent, job());
            let stop_execute =
                HostTeardownExecuteClaim::new(input(&stop, WorkloadTeardownCommandMode::Execute))
                    .expect("stop execute claim should validate");
            assert_eq!(
                stop_backend.execute_stop(stop_execute).await,
                HostTeardownExecuteObservation::Ambiguous
            );
            assert_eq!(stop_client.stop_effect_count(), 0);
            let stop_inspection = inspection_fixture(&stop, "8b");
            let stop_inspection = HostTeardownInspectClaim::new(input(
                &stop_inspection,
                WorkloadTeardownCommandMode::Inspect,
            ))
            .expect("stop inspect claim should validate");
            let stop_observation = stop_backend.inspect_stop(stop_inspection).await;
            assert_eq!(
                matches!(
                    &stop_observation,
                    HostTeardownInspectObservation::InProgress(_)
                ),
                job_type == "stop"
            );
            if job_type != "stop" {
                assert_eq!(stop_observation, HostTeardownInspectObservation::Ambiguous);
            }
        }
    }
}

#[tokio::test]
async fn systemd_accepted_job_retains_progress_without_duplicate_stop() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.accept_next_stop_without_terminal_result();
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert_eq!(
        backend.execute_stop(claim.clone()).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 1);

    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd teardown backend should reopen");
    let inspection = inspection_fixture(&primary, "6d");
    let inspection =
        HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
            .expect("inspection claim should validate");
    assert!(matches!(
        reopened.inspect_stop(inspection).await,
        HostTeardownInspectObservation::InProgress(_)
    ));
    assert_eq!(
        reopened.execute_stop(claim.clone()).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 1);

    client.set_terminal_without_job();
    assert!(matches!(
        reopened.execute_stop(claim).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_terminal_failure_is_retained_without_duplicate_stop() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.fail_next_stop_job("failed");
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    let first_failure = backend.execute_stop(claim.clone()).await;
    assert!(matches!(
        &first_failure,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);

    let inspection = inspection_fixture(&primary, "6e");
    let inspection =
        HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
            .expect("inspection claim should validate");
    assert!(matches!(
        backend.inspect_stop(inspection).await,
        HostTeardownInspectObservation::DefiniteFailure(_)
    ));
    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd teardown backend should reopen");
    assert_eq!(reopened.execute_stop(claim).await, first_failure);
    assert_eq!(client.stop_effect_count(), 1);

    let other_client = TeardownFakeSystemdClient::new();
    let (_other_state, other_backend) = durable_backend(other_client.clone());
    activate(&other_backend, &primary).await;
    other_client.fail_next_stop_job("timeout");
    let other_claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("other terminal-failure claim should validate");
    let other_failure = other_backend.execute_stop(other_claim).await;
    assert!(matches!(
        &other_failure,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_ne!(
        first_failure, other_failure,
        "different terminal job results must retain distinct evidence"
    );
    assert_eq!(other_client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_terminal_receipts_are_absorbing_against_late_submission_updates() {
    let success_client = TeardownFakeSystemdClient::new();
    let (_success_state, success_backend) = durable_backend(success_client.clone());
    let success_fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate(&success_backend, &success_fixture).await;
    let success_claim = HostTeardownExecuteClaim::new(input(
        &success_fixture,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("success claim should validate");
    let success = success_backend.execute_stop(success_claim.clone()).await;
    assert!(matches!(
        success,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    let success_key = SystemdTeardownOperationKey::from_parts(
        success_fixture.execution.execution_id().clone(),
        success_fixture.execution.attempt_id().clone(),
        success_fixture.claim.attempt().attempt_id().clone(),
    );
    assert_eq!(
        set_stage(
            success_backend
                .teardown_store()
                .expect("success backend should retain a durable store"),
            &success_key,
            SystemdStopStage::UnknownSubmission {
                _error: "late submitter".to_owned(),
            },
        )
        .expect("late success update should inspect the receipt"),
        Some(success.clone())
    );
    assert_eq!(success_backend.execute_stop(success_claim).await, success);

    let failure_client = TeardownFakeSystemdClient::new();
    let (_failure_state, failure_backend) = durable_backend(failure_client.clone());
    let failure_fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate(&failure_backend, &failure_fixture).await;
    failure_client.fail_next_stop_job("failed");
    let failure_claim = HostTeardownExecuteClaim::new(input(
        &failure_fixture,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("failure claim should validate");
    let failure = failure_backend.execute_stop(failure_claim.clone()).await;
    assert!(matches!(
        failure,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    let failure_key = SystemdTeardownOperationKey::from_parts(
        failure_fixture.execution.execution_id().clone(),
        failure_fixture.execution.attempt_id().clone(),
        failure_fixture.claim.attempt().attempt_id().clone(),
    );
    assert_eq!(
        set_stage(
            failure_backend
                .teardown_store()
                .expect("failure backend should retain a durable store"),
            &failure_key,
            SystemdStopStage::AcceptedJob {
                job_path: "/org/freedesktop/systemd1/job/late".to_owned(),
                _wait_error: "late submitter".to_owned(),
            },
        )
        .expect("late failure update should inspect the receipt"),
        Some(failure.clone())
    );
    assert_eq!(failure_backend.execute_stop(failure_claim).await, failure);
}

#[tokio::test]
async fn systemd_store_failure_before_submission_has_zero_effect() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    fs::write(
        state.path().join("systemd-teardown-state.json"),
        b"{corrupt-before-effect",
    )
    .expect("corrupt state should write");
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert!(matches!(
        backend.execute_stop(claim).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 0);
}

#[tokio::test]
async fn systemd_corrupt_barrier_store_fails_before_activation_effect() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    fs::write(
        state.path().join("systemd-teardown-state.json"),
        b"{corrupt-before-activation",
    )
    .expect("corrupt state should write");
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);

    backend
        .activate_exact(fixture.execution, fixture.activation_claim, request())
        .await
        .expect_err("corrupt barrier store must reject activation");
    assert_eq!(client.start_effect_count(), 0);
}

#[tokio::test]
async fn systemd_store_failure_after_submission_is_ambiguous() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.corrupt_state_after_stop(state.path().to_path_buf());
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert_eq!(
        backend.execute_stop(claim).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    assert_eq!(client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_exact_teardown_without_durable_state_fails_closed() {
    let client = TeardownFakeSystemdClient::new();
    let activation_backend = SystemdTransientUnitBackend::new(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&activation_backend, &primary).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("teardown claim should validate");

    assert!(matches!(
        activation_backend.execute_stop(claim).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 0);
}
