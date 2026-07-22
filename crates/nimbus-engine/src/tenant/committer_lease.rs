use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, StorageErrorKind, Timestamp};
use nimbus_storage::{CommitterLease, CommitterLeaseError};

use crate::engine::ProjectionToken;

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
    next_renewal_at: Instant,
}

/// Monotonic time used only to schedule local lease-renewal attempts.
///
/// Durable lease validity remains provider-owned: storage adapters compare
/// expiry against their database server's clock inside the lease CAS. Keeping
/// this seam separate from [`nimbus_storage::Clock`] prevents local wall-clock
/// adjustments from delaying or accelerating the renewal cadence.
pub(crate) trait LeaseRenewalClock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Default)]
pub(crate) struct SystemLeaseRenewalClock;

impl LeaseRenewalClock for SystemLeaseRenewalClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
pub(crate) struct ManualLeaseRenewalClock {
    now: Mutex<Instant>,
}

#[cfg(test)]
impl ManualLeaseRenewalClock {
    pub(crate) fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    pub(crate) fn advance(&self, duration: Duration) {
        let mut now = self
            .now
            .lock()
            .expect("manual lease-renewal clock lock should not be poisoned");
        *now = now
            .checked_add(duration)
            .expect("manual lease-renewal clock must remain representable");
    }
}

#[cfg(test)]
impl LeaseRenewalClock for ManualLeaseRenewalClock {
    fn now(&self) -> Instant {
        *self
            .now
            .lock()
            .expect("manual lease-renewal clock lock should not be poisoned")
    }
}

struct RenewalWake {
    generation: Mutex<u64>,
    ready: Condvar,
    #[cfg(test)]
    observed_decision: Mutex<(u64, bool)>,
    #[cfg(test)]
    decision_observed: Condvar,
}

pub(crate) struct CommitterLeaseLifecycle {
    owner_id: String,
    renewal_clock: Arc<dyn LeaseRenewalClock>,
    state: Mutex<CommitterLeaseState>,
    wake: Arc<RenewalWake>,
    worker: BackgroundWorker,
    worker_active: Arc<AtomicBool>,
    closed: AtomicBool,
}

impl TenantRuntime {
    /// Ensures provider sequence authority before the ordered publisher can
    /// assign ahead of durability.
    ///
    /// Embedded runtimes return without a task hop. Provider acquisition is a
    /// blocking storage interface (PostgreSQL bridges through `block_on`), so
    /// the async committer must never execute it on a Tokio worker. Successful
    /// acquisition also republishes recovered progress; callers must await
    /// this method while holding the assignment/recovery gate and before
    /// capturing an assignment baseline.
    pub(crate) async fn ensure_committer_lease_for_ordered_assignment(
        self: &Arc<Self>,
    ) -> Result<()> {
        if self.committer_lease.is_none() {
            return Ok(());
        }
        let runtime = Arc::clone(self);
        tokio::task::spawn_blocking(move || runtime.ensure_committer_lease_for_assignment())
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "committer lease acquisition task panicked before ordered assignment: {error}"
                ))
            })?
    }

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

    /// Returns the durable source order represented by this runtime's visible
    /// state after a locally authorized commit.
    ///
    /// Provider runtimes must already hold their lease; an unacquired or
    /// fenced runtime cannot manufacture publication provenance. Embedded
    /// runtimes use epoch zero and retain their process-local ordering.
    pub(crate) fn projection_token(&self) -> Result<ProjectionToken> {
        let lease_epoch = self
            .committer_lease
            .as_ref()
            .map(|lifecycle| lifecycle.source_epoch())
            .transpose()?
            .unwrap_or(0);
        Ok(ProjectionToken {
            tenant_incarnation: self.tenant_incarnation(),
            lease_epoch,
            durable_sequence: self.applied_head(),
        })
    }

    pub(crate) fn record_committer_fenced(&self, owner_id: String, epoch: u64) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.record_fenced(owner_id, epoch);
            // Wake the committer actor, which owns the existing tenant-runtime
            // eviction sequence. Until it drains, held_identity remains fenced,
            // so no later assignment can degrade to an unfenced store call.
            self.shutdown_committer();
        }
    }

    pub(crate) fn committer_fenced_error(&self) -> Option<Error> {
        self.committer_lease
            .as_ref()
            .and_then(|lifecycle| lifecycle.fenced_error())
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

    pub(crate) fn persist_trigger_invocations(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        records: &[nimbus_core::TriggerInvocationRecord],
        cursor: nimbus_core::TriggerDeliveryCursor,
    ) -> Result<()> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return self.store.materialize_trigger_invocations(records, cursor);
        };
        self.map_fenced_write_result(self.store.fenced_materialize_trigger_invocations(
            &owner_id,
            epoch,
            expected_previous,
            records,
            cursor,
        ))
    }

    pub(crate) fn persist_point_in_time_restore_archive(
        &self,
        expected_previous: nimbus_core::SequenceNumber,
        archive: &nimbus_storage::PointInTimeRestoreArchive,
    ) -> Result<nimbus_storage::JournalProgress> {
        let Some((owner_id, epoch)) = self.held_committer_lease()? else {
            return self.store.import_point_in_time_restore_archive(archive);
        };
        self.map_fenced_write_result(self.store.fenced_import_point_in_time_restore_archive(
            &owner_id,
            epoch,
            expected_previous,
            archive,
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

    #[cfg(test)]
    pub(crate) fn confirm_committer_lease_renewal_not_due_for_testing(
        &self,
        timeout: Duration,
    ) -> bool {
        self.committer_lease
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.confirm_not_due_for_testing(timeout))
    }

    pub(crate) fn shutdown_committer_lease_renewal(&self) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.shutdown();
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn pause_committer_lease_renewal_for_testing(&self) {
        if let Some(lifecycle) = &self.committer_lease {
            lifecycle.pause_renewal_for_testing();
        }
    }
}

impl CommitterLeaseLifecycle {
    pub(crate) fn new(owner_id: String, renewal_clock: Arc<dyn LeaseRenewalClock>) -> Self {
        let now = renewal_clock.now();
        Self {
            owner_id,
            renewal_clock,
            state: Mutex::new(CommitterLeaseState {
                status: CommitterLeaseStatus::Unacquired,
                acquire_count: 0,
                renewal_count: 0,
                renewal_failure_count: 0,
                next_renewal_at: now,
            }),
            wake: Arc::new(RenewalWake {
                generation: Mutex::new(0),
                ready: Condvar::new(),
                #[cfg(test)]
                observed_decision: Mutex::new((0, false)),
                #[cfg(test)]
                decision_observed: Condvar::new(),
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
        state.next_renewal_at = renewal_deadline(self.renewal_clock.as_ref());
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

    fn source_epoch(&self) -> Result<u64> {
        let state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        match &state.status {
            CommitterLeaseStatus::Held(lease) => Ok(lease.epoch),
            // The rejected token still identifies work durably committed by
            // this runtime before takeover. Keeping it available prevents a
            // concurrent fence notification from erasing that provenance.
            CommitterLeaseStatus::Fenced { epoch, .. } => Ok(*epoch),
            CommitterLeaseStatus::Unacquired => Err(Error::Internal(
                "provider projection attempted without an acquired committer lease".to_string(),
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

    fn fenced_error(&self) -> Option<Error> {
        let state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        match &state.status {
            CommitterLeaseStatus::Fenced { owner_id, epoch } => {
                Some(fenced_error(owner_id, *epoch))
            }
            CommitterLeaseStatus::Unacquired | CommitterLeaseStatus::Held(_) => None,
        }
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

    #[cfg(any(test, feature = "test-hooks"))]
    fn pause_renewal_for_testing(&self) {
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
            let now = self.renewal_clock.now();
            if now >= next_renewal_at {
                #[cfg(test)]
                self.wake.record_decision(*generation, false);
                return true;
            }
            #[cfg(test)]
            self.wake.record_decision(*generation, true);
            let wait = next_renewal_at
                .saturating_duration_since(now)
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
                state.next_renewal_at = renewal_deadline(self.renewal_clock.as_ref());
                state.status = CommitterLeaseStatus::Held(lease);
                true
            }
            Err(CommitterLeaseError::Fenced { owner_id, epoch }) => {
                state.renewal_failure_count = state.renewal_failure_count.saturating_add(1);
                state.status = CommitterLeaseStatus::Fenced { owner_id, epoch };
                drop(state);
                // Do not run eviction from this renewal thread: eviction joins
                // the renewal worker. Waking the committer hands ownership to
                // the established close/drain/deregister machinery instead.
                runtime.shutdown_committer();
                false
            }
            Err(error) => {
                state.renewal_failure_count = state.renewal_failure_count.saturating_add(1);
                state.next_renewal_at = renewal_deadline(self.renewal_clock.as_ref());
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

    #[cfg(test)]
    pub(crate) fn confirm_not_due_for_testing(&self, timeout: Duration) -> bool {
        self.wake.notify_and_wait_until_not_due(timeout)
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

    #[cfg(test)]
    fn notify_and_wait_until_not_due(&self, timeout: Duration) -> bool {
        let expected_generation = {
            let mut generation = self
                .generation
                .lock()
                .expect("committer lease renewal wake lock should not be poisoned");
            *generation = generation.saturating_add(1);
            let expected = *generation;
            self.ready.notify_all();
            expected
        };
        let observed = self
            .observed_decision
            .lock()
            .expect("committer lease decision observation lock should not be poisoned");
        let (observed, _) = self
            .decision_observed
            .wait_timeout_while(observed, timeout, |observed| {
                observed.0 < expected_generation
            })
            .expect("committer lease decision observation wait should not be poisoned");
        observed.0 >= expected_generation && observed.1
    }

    #[cfg(test)]
    fn record_decision(&self, generation: u64, not_due: bool) {
        let mut observed = self
            .observed_decision
            .lock()
            .expect("committer lease decision observation lock should not be poisoned");
        if generation > observed.0 {
            *observed = (generation, not_due);
            self.decision_observed.notify_all();
        }
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

fn renewal_deadline(clock: &dyn LeaseRenewalClock) -> Instant {
    clock
        .now()
        .checked_add(RENEW_INTERVAL)
        .expect("fixed lease-renewal interval must fit in the monotonic clock")
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
