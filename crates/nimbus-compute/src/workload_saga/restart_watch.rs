//! Bounded durable discovery of workload restart work.
//!
//! The watch owns paging, due-time decisions, wake hints, and load control. It
//! does not inspect providers, admit restarts, or execute commands. A retained
//! supervisor composes those compute-owned seams for each exact durable saga.

use std::future::Future;
use std::num::{NonZeroU64, NonZeroUsize};
use std::pin::Pin;
use std::sync::Arc;

use nimbus_workloads::{
    MAX_WORKLOAD_SAGA_PAGE_SIZE, WorkloadRestartCandidateCursor, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadRestartNotBeforeUnixMillis,
    WorkloadRestartRecoveryDecision, WorkloadSagaRecord, WorkloadSagaStoreError,
};
use thiserror::Error;
use tokio::sync::{Mutex, Notify};

use super::restart_supervisor::RestartCandidateFailure;
use super::{WorkloadRestartCancellationToken, WorkloadSagaCoordinator};

/// Hard work bound before the watch yields to its injected clock.
const MAX_RESTART_PAGES_PER_SWEEP: usize = 64;
/// Cap durable-store outage backoff at 64 times the configured rescan period.
const MAX_RESTART_STORE_BACKOFF_SHIFT: u32 = 6;

/// Result of one injected-clock wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestartWait {
    DeadlineReached,
    Cancelled,
}

/// One asynchronous clock wait.
pub(super) type RestartWaitFuture<'a> = Pin<Box<dyn Future<Output = RestartWait> + Send + 'a>>;

/// Wall-clock seam for deterministic restart scheduling and rollback tests.
pub(super) trait RestartClock: Send + Sync {
    fn now_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis;

    fn wait_until(
        &self,
        deadline: WorkloadRestartNotBeforeUnixMillis,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> RestartWaitFuture<'_>;
}

/// A wake signal without restart authority or durable evidence.
#[allow(
    dead_code,
    reason = "provider exit hints are an optional wake optimization and own no restart authority"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestartHint {
    ReadOnly,
}

#[allow(
    dead_code,
    reason = "provider exit hints are an optional wake optimization and own no restart authority"
)]
pub(super) const fn read_only_exit_hint() -> RestartHint {
    RestartHint::ReadOnly
}

/// Cloneable producer for advisory read-only wake hints.
#[derive(Clone)]
pub(super) struct RestartHintHandle {
    wake: Arc<Notify>,
}

impl RestartHintHandle {
    #[allow(
        dead_code,
        reason = "provider exit hints are an optional wake optimization and own no restart authority"
    )]
    pub(super) fn notify(&self, _hint: RestartHint) {
        self.wake.notify_one();
    }
}

/// Whether one exact durable candidate started or joined retained work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RestartTrack {
    Started,
    Joined,
    Failed(RestartCandidateFailure),
}

/// Keyed retained-work boundary. The watch grants no provider authority.
pub(super) trait RestartSupervisor: Send + Sync {
    fn track(&self, record: WorkloadSagaRecord) -> Result<RestartTrack, String>;

    /// Retire one exact failure only after the current sweep observed it.
    fn acknowledge_failure(&self, failure: &RestartCandidateFailure) -> Result<bool, String>;
}

/// Durable page, supervision, or configuration failure.
#[derive(Debug, Error)]
pub(super) enum RestartWatchError {
    #[error("restart watch page size must be between 1 and {MAX_WORKLOAD_SAGA_PAGE_SIZE}")]
    InvalidPageSize,
    #[error("restart watch deadline overflow")]
    DeadlineOverflow,
    #[error("restart watch durable store failed: {0}")]
    Store(#[from] WorkloadSagaStoreError),
    #[error("restart supervisor rejected durable candidate: {message}")]
    Supervisor { message: String },
}

/// One complete bounded sweep summary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartSweep {
    earliest_deadline: Option<WorkloadRestartNotBeforeUnixMillis>,
    pages: usize,
    candidates: usize,
}

/// Compute-owned durable restart discovery loop.
pub(super) struct DurableRestartWatch {
    page_size: NonZeroUsize,
    rescan_interval_millis: NonZeroU64,
    clock: Arc<dyn RestartClock>,
    cancellation: WorkloadRestartCancellationToken,
    wake: Arc<Notify>,
    coordinator: Arc<WorkloadSagaCoordinator>,
    supervisor: Arc<dyn RestartSupervisor>,
    sweep_cursor: Mutex<Option<WorkloadRestartCandidateCursor>>,
}

impl DurableRestartWatch {
    pub(super) fn new(
        page_size: NonZeroUsize,
        rescan_interval_millis: NonZeroU64,
        clock: Arc<dyn RestartClock>,
        cancellation: WorkloadRestartCancellationToken,
        coordinator: Arc<WorkloadSagaCoordinator>,
        supervisor: Arc<dyn RestartSupervisor>,
    ) -> Result<Self, RestartWatchError> {
        if page_size.get() > usize::from(MAX_WORKLOAD_SAGA_PAGE_SIZE) {
            return Err(RestartWatchError::InvalidPageSize);
        }
        Ok(Self {
            page_size,
            rescan_interval_millis,
            clock,
            cancellation,
            wake: Arc::new(Notify::new()),
            coordinator,
            supervisor,
            sweep_cursor: Mutex::new(None),
        })
    }

    pub(super) fn hint_handle(&self) -> RestartHintHandle {
        RestartHintHandle {
            wake: Arc::clone(&self.wake),
        }
    }

    async fn load_durable_restart_page(
        &self,
        after: Option<WorkloadRestartCandidateCursor>,
    ) -> Result<WorkloadRestartCandidatePage, RestartWatchError> {
        let page_size =
            u16::try_from(self.page_size.get()).map_err(|_| RestartWatchError::InvalidPageSize)?;
        let request = WorkloadRestartCandidatePageRequest::new(after, page_size)?;
        self.coordinator
            .list_restart_candidates(request)
            .await
            .map_err(Into::into)
    }

    async fn dispatch_each_due_epoch_once(&self) -> Result<RestartSweep, RestartWatchError> {
        // One watch owns this cursor. Retaining it across bounded sweeps avoids
        // starving records beyond the first page budget while also preventing
        // concurrent callers from dispatching the same sweep.
        let mut retained_cursor = self.sweep_cursor.lock().await;
        let mut cursor = retained_cursor.clone();
        let mut pages = 0;
        let mut candidates = 0;
        let mut earliest_deadline: Option<WorkloadRestartNotBeforeUnixMillis> = None;
        let mut observed_failures = Vec::new();

        while pages < MAX_RESTART_PAGES_PER_SWEEP {
            if self.cancellation.is_cancelled() {
                break;
            }
            let page = self.load_durable_restart_page(cursor).await?;
            pages += 1;
            candidates += page.records().len();
            for record in page.records() {
                let now = self.clock.now_unix_millis();
                match record.restart_recovery_decision(now) {
                    WorkloadRestartRecoveryDecision::WaitingUntil(deadline) => {
                        earliest_deadline = Some(match earliest_deadline {
                            Some(current) => current.min(deadline),
                            None => deadline,
                        });
                    }
                    WorkloadRestartRecoveryDecision::Ready
                    | WorkloadRestartRecoveryDecision::Quiescent => {
                        let track = self
                            .supervisor
                            .track(record.clone())
                            .map_err(|message| RestartWatchError::Supervisor { message })?;
                        if let RestartTrack::Failed(failure) = track {
                            observed_failures.push(failure);
                        }
                    }
                }
            }
            let Some(next) = page.next_cursor().cloned() else {
                cursor = None;
                break;
            };
            cursor = Some(next);
        }
        *retained_cursor = cursor;
        for failure in observed_failures {
            if self
                .supervisor
                .acknowledge_failure(&failure)
                .map_err(|message| RestartWatchError::Supervisor { message })?
            {
                tracing::warn!(
                    saga_id = %failure.key().saga_id(),
                    message = failure.message(),
                    "retained restart failure will be retried by a later durable sweep"
                );
            }
        }

        Ok(RestartSweep {
            earliest_deadline,
            pages,
            candidates,
        })
    }

    /// Run complete bounded sweeps until cooperative watch cancellation.
    pub(super) async fn bounded_restart_watch(&self) -> Result<RestartWait, RestartWatchError> {
        let mut consecutive_store_failures = 0_u32;
        loop {
            if self.cancellation.is_cancelled() {
                return Ok(RestartWait::Cancelled);
            }
            let now = self.clock.now_unix_millis();
            let periodic = WorkloadRestartNotBeforeUnixMillis::new(
                now.as_u64()
                    .checked_add(self.rescan_interval_millis.get())
                    .ok_or(RestartWatchError::DeadlineOverflow)?,
            );
            let (deadline, hints_may_wake) = match self.dispatch_each_due_epoch_once().await {
                Ok(sweep) => {
                    consecutive_store_failures = 0;
                    (
                        sweep
                            .earliest_deadline
                            .map_or(periodic, |candidate| candidate.min(periodic)),
                        true,
                    )
                }
                Err(RestartWatchError::Store(error)) => {
                    consecutive_store_failures = consecutive_store_failures.saturating_add(1);
                    let retry_backoff_millis = restart_store_backoff_millis(
                        self.rescan_interval_millis,
                        consecutive_store_failures,
                    );
                    let deadline = WorkloadRestartNotBeforeUnixMillis::new(
                        now.as_u64()
                            .checked_add(retry_backoff_millis)
                            .ok_or(RestartWatchError::DeadlineOverflow)?,
                    );
                    tracing::warn!(
                        error = %error,
                        consecutive_failures = consecutive_store_failures,
                        retry_backoff_millis,
                        "durable restart discovery failed; retained work will retry after bounded backoff"
                    );
                    // Exit hints carry no durable evidence. During a store
                    // outage they must not bypass the failure backoff and turn
                    // an advisory signal stream into a hot durable-store loop.
                    (deadline, false)
                }
                Err(error) => return Err(error),
            };

            let wait = self.clock.wait_until(deadline, &self.cancellation);
            if !hints_may_wake {
                let result = wait.await;
                if result == RestartWait::Cancelled {
                    return Ok(result);
                }
                continue;
            }
            tokio::select! {
                result = wait => {
                    if result == RestartWait::Cancelled {
                        return Ok(result);
                    }
                }
                () = self.wake.notified() => {}
            }
        }
    }
}

fn restart_store_backoff_millis(base: NonZeroU64, consecutive_failures: u32) -> u64 {
    let shift = consecutive_failures
        .saturating_sub(1)
        .min(MAX_RESTART_STORE_BACKOFF_SHIFT);
    base.get().saturating_mul(1_u64 << shift)
}

#[cfg(test)]
#[path = "restart_watch/tests.rs"]
mod tests;
