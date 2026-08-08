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

use super::{WorkloadRestartCancellationToken, WorkloadSagaCoordinator};

/// Hard work bound before the watch yields to its injected clock.
const MAX_RESTART_PAGES_PER_SWEEP: usize = 64;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestartTrack {
    Started,
    Joined,
}

/// Keyed retained-work boundary. The watch grants no provider authority.
pub(super) trait RestartSupervisor: Send + Sync {
    fn track(&self, record: WorkloadSagaRecord) -> Result<RestartTrack, String>;
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
                        self.supervisor
                            .track(record.clone())
                            .map_err(|message| RestartWatchError::Supervisor { message })?;
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

        Ok(RestartSweep {
            earliest_deadline,
            pages,
            candidates,
        })
    }

    /// Run complete bounded sweeps until cooperative watch cancellation.
    pub(super) async fn bounded_restart_watch(&self) -> Result<RestartWait, RestartWatchError> {
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
            let deadline = match self.dispatch_each_due_epoch_once().await {
                Ok(sweep) => sweep
                    .earliest_deadline
                    .map_or(periodic, |candidate| candidate.min(periodic)),
                Err(RestartWatchError::Store(_)) => periodic,
                Err(error) => return Err(error),
            };

            let wait = self.clock.wait_until(deadline, &self.cancellation);
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

#[cfg(test)]
#[path = "restart_watch/tests.rs"]
mod tests;
