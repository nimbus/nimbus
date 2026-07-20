use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

use nimbus_core::{Error, Result, StorageErrorKind, Timestamp};
use nimbus_storage::{Clock, CommitterLease, CommitterLeaseError};

use super::TenantRuntime;
use super::background::BackgroundWorker;

const ACQUIRE_LEASE_DURATION: Duration = Duration::from_secs(30);
const RENEW_LEASE_DURATION: Duration = Duration::from_secs(60);
const RENEW_INTERVAL: Duration = Duration::from_secs(10);
const MAX_RENEW_WAIT_SLICE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CommitterLeaseStats {
    pub(crate) acquired: bool,
    pub(crate) epoch: u64,
    pub(crate) expires_at: Timestamp,
    pub(crate) fenced: bool,
    pub(crate) acquire_count: u64,
    pub(crate) renewal_count: u64,
    pub(crate) renewal_failure_count: u64,
    pub(crate) renewal_worker_running: bool,
}

enum CommitterLeaseStatus {
    Unacquired,
    Held(CommitterLease),
    Fenced { owner_id: String, epoch: u64 },
}

struct CommitterLeaseState {
    status: CommitterLeaseStatus,
    acquire_count: u64,
    renewal_count: u64,
    renewal_failure_count: u64,
    next_renewal_at: Timestamp,
}

struct RenewalWake {
    generation: Mutex<u64>,
    ready: Condvar,
}

pub(crate) struct CommitterLeaseLifecycle {
    owner_id: String,
    clock: Arc<dyn Clock>,
    state: Mutex<CommitterLeaseState>,
    wake: Arc<RenewalWake>,
    worker: BackgroundWorker,
    worker_active: Arc<AtomicBool>,
    closed: AtomicBool,
}

impl TenantRuntime {
    /// Acquires provider sequence authority at the last responsible moment.
    /// Embedded runtimes have no lifecycle object, so this is a single option
    /// check and performs no store work for them.
    pub(crate) fn ensure_committer_lease_for_assignment(self: &Arc<Self>) -> Result<()> {
        let Some(lifecycle) = &self.committer_lease else {
            return Ok(());
        };
        lifecycle.ensure_acquired(self)
    }

    pub(crate) fn committer_lease_stats(&self) -> CommitterLeaseStats {
        self.committer_lease
            .as_ref()
            .map_or_else(CommitterLeaseStats::default, |lifecycle| lifecycle.stats())
    }

    /// Returns the provider lease token held by this runtime.
    ///
    /// Embedded runtimes deliberately return `None`: they retain process-local
    /// sequence authority and never pay for or interact with a durable lease.
    pub(crate) fn held_committer_lease(&self) -> Result<Option<(String, u64)>> {
        self.committer_lease
            .as_ref()
            .map(|lifecycle| lifecycle.held_identity().map(Some))
            .unwrap_or(Ok(None))
    }

    pub(crate) fn record_committer_fenced(&self, owner_id: String, epoch: u64) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.record_fenced(owner_id, epoch);
        }
    }

    /// Atomically persists a provider durable batch under the held lease.
    /// Returns `false` for embedded stores so their existing append/apply path
    /// remains byte-for-byte unchanged and never consults lease storage.
    pub(crate) fn persist_fenced_provider_batch(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        records: &[nimbus_core::TenantEventRecord],
    ) -> Result<bool> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return Ok(false);
        };
        self.map_fenced_write_result(self.store.fenced_append_and_apply_durable_records_batch(
            &owner_id,
            epoch,
            expected_previous,
            records,
        ))?;
        Ok(true)
    }

    pub(crate) fn persist_prepared_write_batch(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        record: &nimbus_core::TenantEventRecord,
        schedule_ops: &[nimbus_storage::ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> Result<Option<nimbus_core::CommitEntry>> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return self.store.apply_prepared_write_batch(
                record,
                schedule_ops,
                scheduled_execution_id,
            );
        };
        self.map_fenced_write_result(self.store.fenced_apply_prepared_write_batch(
            &owner_id,
            epoch,
            expected_previous,
            record,
            schedule_ops,
            scheduled_execution_id,
        ))
    }

    pub(crate) fn persist_table_schema(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        table_schema: &nimbus_core::TableSchema,
    ) -> Result<()> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return self.store.replace_table_schema(table_schema);
        };
        self.map_fenced_write_result(self.store.fenced_replace_table_schema(
            &owner_id,
            epoch,
            expected_previous,
            table_schema,
        ))
    }

    pub(crate) fn persist_table_schema_deletion(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        table: &nimbus_core::TableName,
    ) -> Result<()> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return self.store.delete_table_schema(table);
        };
        self.map_fenced_write_result(self.store.fenced_delete_table_schema(
            &owner_id,
            epoch,
            expected_previous,
            table,
        ))
    }

    fn map_fenced_write_result<T>(
        &self,
        result: nimbus_storage::CommitterLeaseResult<T>,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(CommitterLeaseError::Fenced { owner_id, epoch }) => {
                self.record_committer_fenced(owner_id.clone(), epoch);
                Err(Error::CommitterFenced { owner_id, epoch })
            }
            Err(CommitterLeaseError::Storage(error)) => Err(error),
            Err(CommitterLeaseError::Held | CommitterLeaseError::Unsupported) => Err(
                Error::Internal("provider durable write requires fenced-apply support".to_string()),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn wake_committer_lease_renewal_for_testing(&self) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.wake_for_testing();
        }
    }

    pub(crate) fn shutdown_committer_lease_renewal(&self) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.shutdown();
        }
    }
}

impl CommitterLeaseLifecycle {
    pub(crate) fn new(owner_id: String, clock: Arc<dyn Clock>) -> Self {
        Self {
            owner_id,
            clock,
            state: Mutex::new(CommitterLeaseState {
                status: CommitterLeaseStatus::Unacquired,
                acquire_count: 0,
                renewal_count: 0,
                renewal_failure_count: 0,
                next_renewal_at: Timestamp(0),
            }),
            wake: Arc::new(RenewalWake {
                generation: Mutex::new(0),
                ready: Condvar::new(),
            }),
            worker: BackgroundWorker::new(),
            worker_active: Arc::new(AtomicBool::new(false)),
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn ensure_acquired(self: &Arc<Self>, runtime: &Arc<TenantRuntime>) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::storage(
                StorageErrorKind::Unavailable,
                "committer lease lifecycle is shutting down",
            ));
        }
        match &state.status {
            CommitterLeaseStatus::Held(_) => return Ok(()),
            CommitterLeaseStatus::Fenced { owner_id, epoch } => {
                return Err(fenced_error(owner_id, *epoch));
            }
            CommitterLeaseStatus::Unacquired => {}
        }

        let lease = runtime
            .store
            .acquire_committer_lease(&self.owner_id, ACQUIRE_LEASE_DURATION)
            .map_err(map_lease_error)?;
        let progress = runtime.store.recover_durable_journal()?;
        if progress.durable_head < lease.durable_sequence {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!(
                    "committer lease durable sequence {} exceeds recovered storage head {}",
                    lease.durable_sequence, progress.durable_head
                ),
            ));
        }

        // Acquisition runs inside the committer's serial assignment section.
        // No assignment can observe the pre-acquisition heads after this point.
        runtime.publish_mutation_journal_progress_in_actor(progress);
        state.acquire_count = state.acquire_count.saturating_add(1);
        state.next_renewal_at = add_duration(self.clock.now(), RENEW_INTERVAL);
        state.status = CommitterLeaseStatus::Held(lease);
        drop(state);

        self.start_worker(runtime);
        Ok(())
    }

    fn start_worker(self: &Arc<Self>, runtime: &Arc<TenantRuntime>) {
        let _state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let lifecycle = Arc::downgrade(self);
        let runtime = Arc::downgrade(runtime);
        let worker_active = self.worker_active.clone();
        worker_active.store(true, Ordering::Release);
        self.worker
            .start("nimbus-committer-lease-renewal", move |shutdown| {
                struct WorkerStopped(Arc<AtomicBool>);
                impl Drop for WorkerStopped {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _stopped = WorkerStopped(worker_active);
                run_renewal_worker(lifecycle, runtime, shutdown);
            });
    }

    fn held_identity(&self) -> Result<(String, u64)> {
        let state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        match &state.status {
            CommitterLeaseStatus::Held(lease) => Ok((lease.owner_id.clone(), lease.epoch)),
            CommitterLeaseStatus::Fenced { owner_id, epoch } => Err(fenced_error(owner_id, *epoch)),
            CommitterLeaseStatus::Unacquired => Err(Error::Internal(
                "provider durable write attempted without an acquired committer lease".to_string(),
            )),
        }
    }

    fn record_fenced(&self, owner_id: String, epoch: u64) {
        let mut state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        state.status = CommitterLeaseStatus::Fenced { owner_id, epoch };
    }

    fn shutdown(&self) {
        let state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        self.closed.store(true, Ordering::Release);
        drop(state);
        let wake = self.wake.clone();
        self.worker
            .shutdown(move |shutdown| wake.signal_shutdown(shutdown));
    }

    fn wait_until_renewal_due(&self, shutdown: &AtomicBool) -> bool {
        let mut generation = self
            .wake
            .generation
            .lock()
            .expect("committer lease renewal wake lock should not be poisoned");
        loop {
            if shutdown.load(Ordering::Acquire) {
                return false;
            }
            let next_renewal_at = self
                .state
                .lock()
                .expect("committer lease state lock should not be poisoned")
                .next_renewal_at;
            let now = self.clock.now();
            if now >= next_renewal_at {
                return true;
            }
            let wait = Duration::from_millis(next_renewal_at.0.saturating_sub(now.0))
                .min(MAX_RENEW_WAIT_SLICE);
            let observed_generation = *generation;
            let (next_generation, _) = self
                .wake
                .ready
                .wait_timeout_while(generation, wait, |current| {
                    *current == observed_generation && !shutdown.load(Ordering::Acquire)
                })
                .expect("committer lease renewal wait should not be poisoned");
            generation = next_generation;
        }
    }

    fn renew_once(&self, runtime: &TenantRuntime) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        let (owner_id, epoch) = match &state.status {
            CommitterLeaseStatus::Held(lease) => (lease.owner_id.clone(), lease.epoch),
            CommitterLeaseStatus::Unacquired | CommitterLeaseStatus::Fenced { .. } => return false,
        };

        match runtime
            .store
            .renew_committer_lease(owner_id.as_str(), epoch, RENEW_LEASE_DURATION)
        {
            Ok(lease) => {
                state.renewal_count = state.renewal_count.saturating_add(1);
                state.next_renewal_at = add_duration(self.clock.now(), RENEW_INTERVAL);
                state.status = CommitterLeaseStatus::Held(lease);
                true
            }
            Err(CommitterLeaseError::Fenced { owner_id, epoch }) => {
                state.renewal_failure_count = state.renewal_failure_count.saturating_add(1);
                state.status = CommitterLeaseStatus::Fenced { owner_id, epoch };
                false
            }
            Err(error) => {
                state.renewal_failure_count = state.renewal_failure_count.saturating_add(1);
                state.next_renewal_at = add_duration(self.clock.now(), RENEW_INTERVAL);
                tracing::warn!(
                    tenant = %runtime.tenant_id(),
                    error = %error,
                    "committer lease renewal failed; retaining bounded retry schedule"
                );
                true
            }
        }
    }

    pub(crate) fn stats(&self) -> CommitterLeaseStats {
        let state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        let mut stats = CommitterLeaseStats {
            acquire_count: state.acquire_count,
            renewal_count: state.renewal_count,
            renewal_failure_count: state.renewal_failure_count,
            renewal_worker_running: self.worker_active.load(Ordering::Acquire),
            ..CommitterLeaseStats::default()
        };
        match &state.status {
            CommitterLeaseStatus::Unacquired => {}
            CommitterLeaseStatus::Held(lease) => {
                stats.acquired = true;
                stats.epoch = lease.epoch;
                stats.expires_at = lease.expires_at;
            }
            CommitterLeaseStatus::Fenced { epoch, .. } => {
                stats.epoch = *epoch;
                stats.fenced = true;
            }
        }
        stats
    }

    #[cfg(test)]
    pub(crate) fn wake_for_testing(&self) {
        self.wake.notify();
    }
}

impl RenewalWake {
    #[cfg(test)]
    fn notify(&self) {
        let mut generation = self
            .generation
            .lock()
            .expect("committer lease renewal wake lock should not be poisoned");
        *generation = generation.wrapping_add(1);
        self.ready.notify_all();
    }

    fn signal_shutdown(&self, shutdown: &AtomicBool) {
        let mut generation = self
            .generation
            .lock()
            .expect("committer lease renewal wake lock should not be poisoned");
        shutdown.store(true, Ordering::Release);
        *generation = generation.wrapping_add(1);
        self.ready.notify_all();
    }
}

impl Drop for CommitterLeaseLifecycle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_renewal_worker(
    lifecycle: Weak<CommitterLeaseLifecycle>,
    runtime: Weak<TenantRuntime>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        let Some(lifecycle) = lifecycle.upgrade() else {
            return;
        };
        if !lifecycle.wait_until_renewal_due(&shutdown) {
            return;
        }
        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        if !lifecycle.renew_once(runtime.as_ref()) {
            return;
        }
    }
}

fn add_duration(timestamp: Timestamp, duration: Duration) -> Timestamp {
    Timestamp(
        timestamp
            .0
            .saturating_add(duration.as_millis().try_into().unwrap_or(u64::MAX)),
    )
}

fn fenced_error(owner_id: &str, epoch: u64) -> Error {
    Error::CommitterFenced {
        owner_id: owner_id.to_string(),
        epoch,
    }
}

fn map_lease_error(error: CommitterLeaseError) -> Error {
    match error {
        CommitterLeaseError::Held => Error::storage(
            StorageErrorKind::Busy,
            "committer lease is held by another owner",
        ),
        CommitterLeaseError::Fenced { owner_id, epoch } => fenced_error(&owner_id, epoch),
        CommitterLeaseError::Unsupported => {
            Error::Internal("provider assignment requires committer-lease support".to_string())
        }
        CommitterLeaseError::Storage(error) => error,
    }
}
