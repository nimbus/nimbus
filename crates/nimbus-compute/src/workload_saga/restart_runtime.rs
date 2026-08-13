//! Production composition for durable automatic workload restart.
//!
//! One retained watch discovers durable candidates. A compute-owned
//! coordinator converts read-only exit evidence into the same admission and
//! command driver used by explicit restart requests.

use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nimbus_network::NetworkCapabilityRegistry;
use nimbus_sandbox::{
    SandboxExecutionAttemptId, SandboxExecutionAttemptObservation, SandboxExecutionObservation,
    SandboxInspection, SandboxRestartAssessment,
};
use nimbus_workloads::{
    WorkloadInspectionVersion, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy,
    WorkloadSagaRecord,
};

use super::restart_dispatcher::WorkloadRestartDispatcher;
use super::restart_driver::WorkloadRestartDriver;
use super::restart_provider::WorkloadRestartCapabilityRegistry;
#[cfg(any(test, feature = "test-hooks"))]
use super::restart_resolution::NoopWorkloadRestartResolutionFence;
use super::restart_resolution::WorkloadRestartResolutionFence;
use super::restart_submission::{
    ExplicitWorkloadRestartError, ExplicitWorkloadRestartRequest,
    ExplicitWorkloadRestartSubmission, ExplicitWorkloadRestartSubmitter,
};
use super::restart_supervisor::{
    RestartCandidateCoordinator, RestartCandidateFuture, RetainedRestartSupervisor,
};
use super::restart_watch::{
    DurableRestartWatch, RestartClock, RestartHintHandle, RestartSupervisor, RestartWait,
    RestartWaitFuture,
};
use super::{
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionSourceAuthority,
    WorkloadRestartAdmissionError, WorkloadRestartAdmissionRequest,
    WorkloadRestartCancellationToken, WorkloadSagaCoordinator,
};
use crate::workload_projection::{
    WorkloadExecutionObservationRequest, WorkloadProviderObservation,
};

const RESTART_WATCH_PAGE_SIZE: usize = 64;
const RESTART_WATCH_RESCAN_MILLIS: u64 = 1_000;
const RESTART_BACKOFF_INITIAL_MILLIS: u64 = 1_000;
const RESTART_BACKOFF_MAX_MILLIS: u64 = 60_000;

struct SystemRestartClock;

impl RestartClock for SystemRestartClock {
    fn now_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis();
        WorkloadRestartNotBeforeUnixMillis::new(u64::try_from(millis).unwrap_or(u64::MAX))
    }

    fn wait_until(
        &self,
        deadline: WorkloadRestartNotBeforeUnixMillis,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> RestartWaitFuture<'_> {
        let now = self.now_unix_millis().as_u64();
        let delay = Duration::from_millis(deadline.as_u64().saturating_sub(now));
        let mut cancelled = cancellation.subscribe();
        Box::pin(async move {
            tokio::select! {
                () = tokio::time::sleep(delay) => RestartWait::DeadlineReached,
                changed = cancelled.changed() => {
                    let _ = changed;
                    RestartWait::Cancelled
                }
            }
        })
    }
}

struct AutomaticRestartCoordinator {
    coordinator: Arc<WorkloadSagaCoordinator>,
    driver: Arc<WorkloadRestartDriver>,
    observations: Arc<WorkloadProvisionCapabilityRegistry>,
    clock: Arc<dyn RestartClock>,
}

impl AutomaticRestartCoordinator {
    async fn coordinate_record(&self, record: WorkloadSagaRecord) -> Result<(), String> {
        let now = self.clock.now_unix_millis();
        if record.restart_state().active().is_some() {
            let key = record.key().clone();
            self.driver
                .resume(&key, now)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())?;
            return Ok(());
        }

        let request = WorkloadExecutionObservationRequest::for_record(&record);
        let observation = self
            .observations
            .observe_execution(
                record.active_intent().source().execution_provider_id(),
                &request,
            )
            .await
            .map_err(|error| error.to_string())?;
        let WorkloadProviderObservation::Present(inspection) = observation else {
            // Absence, progress, and ambiguity are read-only hints. A later
            // bounded sweep reinspects durable truth; none can admit work.
            return Ok(());
        };
        let expected_attempt = SandboxExecutionAttemptId::new(
            record
                .current_execution_reference()
                .attempt_id()
                .to_string(),
        )
        .map_err(|error| error.to_string())?;
        if !matches!(
            &inspection.execution_attempt,
            SandboxExecutionAttemptObservation::Exact(observed) if observed == &expected_attempt
        ) {
            return Err(
                "automatic restart inspection crossed the current execution attempt".to_owned(),
            );
        }
        let Some(exit_code) = authenticated_restart_exit(&inspection)? else {
            return Ok(());
        };
        let policy = record.active_intent().restart_policy();
        let completed = record.restart_state().completed_automatic_restart_count();
        if !automatic_restart_eligible(policy, exit_code, completed) {
            return Ok(());
        }

        let not_before = WorkloadRestartNotBeforeUnixMillis::new(
            now.as_u64()
                .checked_add(restart_backoff_millis(completed))
                .ok_or_else(|| "automatic restart deadline overflow".to_owned())?,
        );
        let request = WorkloadRestartAdmissionRequest::for_automatic(
            &record,
            exit_code,
            WorkloadInspectionVersion::sha256(inspection.version.as_bytes()),
            not_before,
        );
        let cancellation = WorkloadRestartCancellationToken::new();
        let admitted = match self
            .coordinator
            .compare_and_swap_restart_admission(&request, &cancellation)
            .await
        {
            Ok(admitted) => admitted,
            Err(WorkloadRestartAdmissionError::Saga(
                nimbus_workloads::WorkloadSagaStoreError::Conflict { .. },
            )) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        self.driver
            .drive_admitted(admitted.record().clone(), now)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

fn authenticated_restart_exit(inspection: &SandboxInspection) -> Result<Option<i32>, String> {
    let SandboxExecutionObservation::Exited { exit_code } = inspection.execution else {
        return Ok(None);
    };
    let SandboxRestartAssessment::Candidate {
        exit_code: assessed_exit_code,
        blocker: None,
    } = inspection.restart
    else {
        // Provider-authenticated shutdown, cleanup, and startup
        // reconciliation evidence veto admission. Compute owns policy and
        // scheduling, but it cannot ignore physical provider blockers.
        return Ok(None);
    };
    if assessed_exit_code != exit_code {
        return Err("automatic restart inspection crossed exit evidence".to_owned());
    }
    Ok(Some(exit_code))
}

impl RestartCandidateCoordinator for AutomaticRestartCoordinator {
    fn coordinate(&self, record: WorkloadSagaRecord) -> RestartCandidateFuture<'_> {
        Box::pin(async move { self.coordinate_record(record).await })
    }
}

/// Retained automatic-restart watch and its shared exact command driver.
pub(crate) struct WorkloadRestartRuntime {
    cancellation: WorkloadRestartCancellationToken,
    watch_thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
    _hint_handle: RestartHintHandle,
    explicit_submitter: ExplicitWorkloadRestartSubmitter,
    coordinator: Arc<WorkloadSagaCoordinator>,
    driver: Arc<WorkloadRestartDriver>,
    clock: Arc<dyn RestartClock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkloadRestartSettlement {
    Settled,
    Pending,
}

impl WorkloadRestartRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        coordinator: Arc<WorkloadSagaCoordinator>,
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provider_reports: NetworkCapabilityRegistry,
        provision_capabilities: Arc<WorkloadProvisionCapabilityRegistry>,
        restart_capabilities: Arc<WorkloadRestartCapabilityRegistry>,
        resolution_fence: Arc<dyn WorkloadRestartResolutionFence>,
    ) -> Result<Self, String> {
        let dispatcher = Arc::new(WorkloadRestartDispatcher::new(
            source_authority,
            provider_reports,
            restart_capabilities,
        ));
        let driver = Arc::new(WorkloadRestartDriver::new(
            Arc::clone(&coordinator),
            dispatcher,
            resolution_fence,
        ));
        let clock: Arc<dyn RestartClock> = Arc::new(SystemRestartClock);
        let candidate_coordinator = Arc::new(AutomaticRestartCoordinator {
            coordinator: Arc::clone(&coordinator),
            driver: Arc::clone(&driver),
            observations: provision_capabilities,
            clock: Arc::clone(&clock),
        });
        let supervisor = Arc::new(RetainedRestartSupervisor::new(candidate_coordinator));
        let explicit_submitter = ExplicitWorkloadRestartSubmitter::new(
            Arc::clone(&coordinator),
            Arc::clone(&supervisor),
        );
        let watch_supervisor: Arc<dyn RestartSupervisor> = supervisor.clone();
        let cancellation = WorkloadRestartCancellationToken::new();
        let watch = Arc::new(
            DurableRestartWatch::new(
                NonZeroUsize::new(RESTART_WATCH_PAGE_SIZE)
                    .expect("restart watch page size is nonzero"),
                NonZeroU64::new(RESTART_WATCH_RESCAN_MILLIS)
                    .expect("restart watch interval is nonzero"),
                Arc::clone(&clock),
                cancellation.clone(),
                Arc::clone(&coordinator),
                watch_supervisor,
            )
            .map_err(|error| error.to_string())?,
        );
        let hint_handle = watch.hint_handle();
        let (started, ready) = std::sync::mpsc::sync_channel(1);
        let watch_thread = std::thread::Builder::new()
            .name("nimbus-workload-restart-watch".to_owned())
            .spawn(move || {
                let runtime = build_restart_runtime();
                let _ = started.send(runtime.as_ref().map(|_| ()).map_err(Clone::clone));
                let runtime = runtime?;
                runtime
                    .block_on(watch.bounded_restart_watch())
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())?;
        ready.recv().map_err(|error| error.to_string())??;
        Ok(Self {
            cancellation,
            watch_thread: Mutex::new(Some(watch_thread)),
            _hint_handle: hint_handle,
            explicit_submitter,
            coordinator,
            driver,
            clock,
        })
    }

    pub(crate) async fn submit_explicit(
        &self,
        request: &ExplicitWorkloadRestartRequest,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> Result<ExplicitWorkloadRestartSubmission, ExplicitWorkloadRestartError> {
        self.explicit_submitter.submit(request, cancellation).await
    }

    /// Resolve exact issued restart work after a stopped successor has fenced
    /// new execution. Absence of an active restart is terminal evidence; an
    /// active result that remains unresolved is a typed wait, never inferred
    /// from polling.
    pub(crate) async fn settle_for_teardown(
        &self,
        key: &nimbus_workloads::WorkloadSagaKey,
    ) -> Result<WorkloadRestartSettlement, String> {
        settle_restart_for_teardown_once(
            &self.coordinator,
            &self.driver,
            key,
            self.clock.now_unix_millis(),
        )
        .await
    }
}

async fn settle_restart_for_teardown_once(
    coordinator: &WorkloadSagaCoordinator,
    driver: &WorkloadRestartDriver,
    key: &nimbus_workloads::WorkloadSagaKey,
    now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
) -> Result<WorkloadRestartSettlement, String> {
    let current = coordinator
        .load(key)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "workload restart recovery record does not exist".to_owned())?;
    if current.restart_state().active().is_none() {
        return Ok(WorkloadRestartSettlement::Settled);
    }
    let run = driver
        .resume(key, now_unix_millis)
        .await
        .map_err(|error| error.to_string())?;
    if run.record().restart_state().active().is_none() {
        return Ok(WorkloadRestartSettlement::Settled);
    }
    if matches!(
        run.record()
            .restart_state()
            .active()
            .map(|active| active.disposition()),
        Some(
            nimbus_workloads::WorkloadRestartDisposition::SuccessorVetoed { .. }
                | nimbus_workloads::WorkloadRestartDisposition::DefiniteFailure { .. }
        )
    ) {
        coordinator
            .commit_restart_settlement_teardown(run.record())
            .await
            .map_err(|error| error.to_string())?;
        return Ok(WorkloadRestartSettlement::Settled);
    }
    Ok(WorkloadRestartSettlement::Pending)
}

/// Drive one exact restart settlement without starting the retained watch.
///
/// This test-only seam lets a different test process reopen the real saga
/// store and provider journal at a precise crash boundary. Production uses
/// the private production settlement loop with the same reducer.
#[cfg(any(test, feature = "test-hooks"))]
pub async fn settle_restart_for_teardown_once_for_test(
    coordinator: Arc<WorkloadSagaCoordinator>,
    source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
    provider_reports: NetworkCapabilityRegistry,
    restart_capabilities: Arc<WorkloadRestartCapabilityRegistry>,
    key: &nimbus_workloads::WorkloadSagaKey,
    now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
) -> Result<bool, String> {
    let dispatcher = Arc::new(WorkloadRestartDispatcher::new(
        source_authority,
        provider_reports,
        restart_capabilities,
    ));
    let driver = WorkloadRestartDriver::new(
        Arc::clone(&coordinator),
        dispatcher,
        Arc::new(NoopWorkloadRestartResolutionFence),
    );
    Ok(matches!(
        settle_restart_for_teardown_once(&coordinator, &driver, key, now_unix_millis).await?,
        WorkloadRestartSettlement::Settled
    ))
}

fn build_restart_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

impl Drop for WorkloadRestartRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let thread = self.watch_thread.get_mut().ok().and_then(Option::take);
        if let Some(thread) = thread {
            match thread.join() {
                Err(_) => tracing::error!("workload restart watch thread panicked during shutdown"),
                Ok(Err(message)) => {
                    tracing::error!(%message, "workload restart watch stopped with an error")
                }
                Ok(Ok(())) => {}
            }
        }
    }
}

fn automatic_restart_eligible(
    policy: WorkloadRestartPolicy,
    exit_code: i32,
    completed: u32,
) -> bool {
    match policy {
        WorkloadRestartPolicy::Never => false,
        WorkloadRestartPolicy::OnFailure { max_restarts } => {
            exit_code != 0 && completed < max_restarts
        }
        WorkloadRestartPolicy::Always { max_restarts } => completed < max_restarts,
    }
}

fn restart_backoff_millis(completed: u32) -> u64 {
    let multiplier = 1_u128 << completed.min(31);
    u128::from(RESTART_BACKOFF_INITIAL_MILLIS)
        .saturating_mul(multiplier)
        .min(u128::from(RESTART_BACKOFF_MAX_MILLIS)) as u64
}

#[cfg(test)]
#[path = "restart_runtime/tests.rs"]
mod tests;
