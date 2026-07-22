use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result, StorageErrorKind, Timestamp};
use nimbus_storage::{CommitterLease, CommitterLeaseError, SchedulerWriteReconciliation};

use crate::engine::{ProjectionToken, begin_durable_recovery_eviction};

use super::TenantRuntime;
use super::background::BackgroundWorker;

const ACQUIRE_LEASE_DURATION: Duration = Duration::from_secs(30);
const RENEW_LEASE_DURATION: Duration = Duration::from_secs(60);
const RENEW_INTERVAL: Duration = Duration::from_secs(10);
const TRANSIENT_RETRY_BASE: Duration = Duration::from_secs(1);
const TRANSIENT_RETRY_CAP: Duration = Duration::from_secs(4);
const PROVIDER_EXPIRY_SAFETY_MARGIN: Duration = Duration::from_secs(15);
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
    pub(crate) renewal_failure_streak: u64,
    pub(crate) last_success_age_millis: Option<u64>,
    pub(crate) renewal_worker_running: bool,
}

enum CommitterLeaseStatus {
    Unacquired,
    Held(CommitterLease),
    Fenced { owner_id: String, epoch: u64 },
    ValidityUnknown { owner_id: String, epoch: u64 },
}

struct CommitterLeaseState {
    status: CommitterLeaseStatus,
    acquire_count: u64,
    renewal_count: u64,
    renewal_failure_count: u64,
    renewal_failure_streak: u64,
    last_success_at: Option<Instant>,
    local_safety_deadline: Option<Instant>,
    next_renewal_at: Instant,
}

/// Monotonic time used only to schedule local lease-renewal attempts.
///
/// Durable lease validity remains provider-owned: storage adapters compare
/// expiry against their database server's clock inside the lease CAS. Keeping
/// this seam separate from [`nimbus_core::WallClock`] prevents local wall-clock
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
        let shutdown = self.committer_shutdown_token();
        self.map_fenced_write_result(
            self.store
                .fenced_append_and_apply_durable_records_batch_cancellable(
                    &owner_id,
                    epoch,
                    expected_previous,
                    records,
                    move || {
                        if shutdown.is_cancelled() {
                            Err(Error::Cancelled)
                        } else {
                            Ok(())
                        }
                    },
                ),
        )?;
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

    /// Persists scheduler state behind the same per-tenant sequence owner as
    /// journal, schema, restore, and trigger writes. Scheduler state does not
    /// advance the durable journal, so provider transactions validate the
    /// held lease at the current durable sequence and update scheduler rows in
    /// that same transaction.
    pub(crate) fn persist_scheduler_write<Check, AfterPersist>(
        self: &Arc<Self>,
        operation: nimbus_storage::SchedulerWrite,
        recovery_now: Timestamp,
        check_cancel: Check,
        after_persist: AfterPersist,
        initiated_eviction: Arc<AtomicBool>,
    ) -> Result<nimbus_storage::SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static,
        AfterPersist: Fn() -> Result<()> + Send + 'static,
    {
        let check_cancel = Arc::new(Mutex::new(check_cancel));
        check_cancel
            .lock()
            .expect("scheduler cancellation check lock should not be poisoned")()?;
        if self.begin_scheduler_recovery()
            && let Err(error) = self.persist_scheduler_write_once(
                nimbus_storage::SchedulerWrite::RecoverRunning { now: recovery_now },
                {
                    let check_cancel = check_cancel.clone();
                    move || {
                        check_cancel
                            .lock()
                            .expect("scheduler cancellation check lock should not be poisoned")(
                        )
                    }
                },
                || Ok(()),
                initiated_eviction.clone(),
            )
        {
            self.restore_scheduler_recovery();
            return Err(error);
        }
        self.persist_scheduler_write_once(
            operation,
            move || {
                check_cancel
                    .lock()
                    .expect("scheduler cancellation check lock should not be poisoned")(
                )
            },
            after_persist,
            initiated_eviction,
        )
    }

    fn persist_scheduler_write_once<Check, AfterPersist>(
        self: &Arc<Self>,
        operation: nimbus_storage::SchedulerWrite,
        check_cancel: Check,
        after_persist: AfterPersist,
        initiated_eviction: Arc<AtomicBool>,
    ) -> Result<nimbus_storage::SchedulerWriteResult>
    where
        Check: Fn() -> Result<()> + Send + 'static,
        AfterPersist: Fn() -> Result<()> + Send + 'static,
    {
        self.ensure_committer_lease_for_assignment()?;
        let prepared = self.store.prepare_scheduler_write(operation)?;
        let expected_durable_sequence = self.durable_head();
        let result = match self.held_committer_lease()? {
            Some((owner_id, epoch)) => self.store.fenced_scheduler_write_cancellable(
                &owner_id,
                epoch,
                expected_durable_sequence,
                prepared.operation(),
                check_cancel,
            ),
            None => self
                .store
                .scheduler_write_cancellable(prepared.operation(), check_cancel)
                .map_err(CommitterLeaseError::Storage),
        };
        let write_error = match result {
            Ok(result) => match after_persist() {
                Ok(()) => return Ok(result),
                Err(error) => error,
            },
            Err(CommitterLeaseError::Fenced { owner_id, epoch }) => {
                self.record_committer_fenced(owner_id.clone(), epoch);
                return Err(Error::CommitterFenced { owner_id, epoch });
            }
            Err(CommitterLeaseError::Storage(error)) => error,
            Err(CommitterLeaseError::Held | CommitterLeaseError::Unsupported) => {
                return Err(Error::Internal(
                    "provider scheduler write requires fenced-apply support".to_string(),
                ));
            }
        };

        match self.store.reconcile_scheduler_write(&prepared) {
            Ok(SchedulerWriteReconciliation::Committed(result)) => Ok(result),
            Ok(SchedulerWriteReconciliation::RolledBack) => Err(write_error),
            Ok(SchedulerWriteReconciliation::Ambiguous) => {
                let error = Error::Internal(format!(
                    "scheduler write outcome is ambiguous; crash-and-replay required after persistence failed ({write_error})"
                ));
                self.begin_scheduler_durable_recovery(&error, initiated_eviction);
                Err(error)
            }
            Err(progress_error) => {
                let error = Error::Internal(format!(
                    "scheduler write outcome is ambiguous; crash-and-replay required after persistence failed ({write_error}) and scheduler state could not be read ({progress_error})"
                ));
                self.begin_scheduler_durable_recovery(&error, initiated_eviction);
                Err(error)
            }
        }
    }

    fn begin_scheduler_durable_recovery(&self, error: &Error, initiated_eviction: Arc<AtomicBool>) {
        self.publisher_record_ambiguous_error();
        begin_durable_recovery_eviction(self, error);
        self.fail_and_drain_mutation_queues(error);
        self.close_committed_mutation_observers();
        initiated_eviction.store(true, Ordering::Release);
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
                renewal_failure_streak: 0,
                last_success_at: None,
                local_safety_deadline: None,
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
            CommitterLeaseStatus::ValidityUnknown { owner_id, epoch } => {
                return Err(validity_unknown_error(owner_id, *epoch));
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
        let observed_at = self.renewal_clock.now();
        state.last_success_at = Some(observed_at);
        state.local_safety_deadline =
            Some(local_safety_deadline(observed_at, ACQUIRE_LEASE_DURATION));
        state.renewal_failure_streak = 0;
        state.next_renewal_at = normal_renewal_deadline(observed_at);
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
            CommitterLeaseStatus::ValidityUnknown { owner_id, epoch } => {
                Err(validity_unknown_error(owner_id, *epoch))
            }
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
            CommitterLeaseStatus::ValidityUnknown { epoch, .. } => Ok(*epoch),
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
            CommitterLeaseStatus::ValidityUnknown { owner_id, epoch } => {
                Some(validity_unknown_error(owner_id, *epoch))
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
        let (owner_id, epoch) = {
            let state = self
                .state
                .lock()
                .expect("committer lease state lock should not be poisoned");
            match &state.status {
                CommitterLeaseStatus::Held(lease) => (lease.owner_id.clone(), lease.epoch),
                CommitterLeaseStatus::Unacquired
                | CommitterLeaseStatus::Fenced { .. }
                | CommitterLeaseStatus::ValidityUnknown { .. } => {
                    return false;
                }
            }
        };

        let result =
            runtime
                .store
                .renew_committer_lease(owner_id.as_str(), epoch, RENEW_LEASE_DURATION);
        let observed_at = self.renewal_clock.now();
        let mut state = self
            .state
            .lock()
            .expect("committer lease state lock should not be poisoned");
        if !matches!(
            &state.status,
            CommitterLeaseStatus::Held(lease)
                if lease.owner_id == owner_id && lease.epoch == epoch
        ) {
            return false;
        }

        match result {
            Ok(lease) => {
                record_renewal_success(&mut state, lease, observed_at);
                true
            }
            Err(CommitterLeaseError::Fenced { owner_id, epoch }) => {
                state.renewal_failure_count = state.renewal_failure_count.saturating_add(1);
                state.renewal_failure_streak = state.renewal_failure_streak.saturating_add(1);
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
                state.renewal_failure_streak = state.renewal_failure_streak.saturating_add(1);
                let Some(next_renewal_at) = transient_retry_deadline(
                    observed_at,
                    state.local_safety_deadline,
                    &self.owner_id,
                    runtime.tenant_id().as_str(),
                    state.renewal_failure_streak,
                ) else {
                    state.status = CommitterLeaseStatus::ValidityUnknown { owner_id, epoch };
                    drop(state);
                    tracing::error!(
                        tenant = %runtime.tenant_id(),
                        error = %error,
                        "committer lease renewal exhausted its local safety budget; failing closed"
                    );
                    runtime.shutdown_committer();
                    return false;
                };
                state.next_renewal_at = next_renewal_at;
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
            renewal_failure_streak: state.renewal_failure_streak,
            last_success_age_millis: state.last_success_at.map(|last_success_at| {
                u64::try_from(
                    self.renewal_clock
                        .now()
                        .saturating_duration_since(last_success_at)
                        .as_millis(),
                )
                .unwrap_or(u64::MAX)
            }),
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
            CommitterLeaseStatus::ValidityUnknown { epoch, .. } => {
                stats.epoch = *epoch;
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

fn normal_renewal_deadline(observed_at: Instant) -> Instant {
    observed_at
        .checked_add(RENEW_INTERVAL)
        .expect("fixed lease-renewal interval must fit in the monotonic clock")
}

fn record_renewal_success(
    state: &mut CommitterLeaseState,
    lease: CommitterLease,
    observed_at: Instant,
) {
    state.renewal_count = state.renewal_count.saturating_add(1);
    state.renewal_failure_streak = 0;
    state.last_success_at = Some(observed_at);
    state.local_safety_deadline = Some(local_safety_deadline(observed_at, RENEW_LEASE_DURATION));
    state.next_renewal_at = normal_renewal_deadline(observed_at);
    state.status = CommitterLeaseStatus::Held(lease);
}

fn transient_retry_deadline(
    observed_at: Instant,
    local_safety_deadline: Option<Instant>,
    owner_id: &str,
    tenant_id: &str,
    failure_streak: u64,
) -> Option<Instant> {
    let retry_delay = transient_retry_delay(owner_id, tenant_id, failure_streak);
    let remaining_budget = match local_safety_deadline {
        Some(deadline) if deadline > observed_at => {
            deadline.duration_since(observed_at).min(retry_delay)
        }
        Some(_) => return None,
        None => retry_delay,
    };
    observed_at.checked_add(remaining_budget)
}

fn local_safety_deadline(observed_at: Instant, requested_duration: Duration) -> Instant {
    observed_at
        .checked_add(requested_duration.saturating_sub(PROVIDER_EXPIRY_SAFETY_MARGIN))
        .expect("provider-derived local lease safety budget must fit in the monotonic clock")
}

fn transient_retry_delay(owner_id: &str, tenant_id: &str, failure_streak: u64) -> Duration {
    let exponent = u32::try_from(failure_streak.saturating_sub(1).min(2)).unwrap_or(2);
    let base = TRANSIENT_RETRY_BASE
        .checked_mul(1_u32 << exponent)
        .unwrap_or(TRANSIENT_RETRY_CAP)
        .min(TRANSIENT_RETRY_CAP);
    let jitter_ceiling_nanos = base.as_nanos() / 4;
    let hash = owner_id
        .bytes()
        .chain([0xff])
        .chain(tenant_id.bytes())
        .chain(failure_streak.to_le_bytes())
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            hash.wrapping_mul(0x100000001b3) ^ u64::from(byte)
        });
    let jitter_nanos = u128::from(hash) % jitter_ceiling_nanos.saturating_add(1);
    base.saturating_add(Duration::from_nanos(
        u64::try_from(jitter_nanos).unwrap_or(u64::MAX),
    ))
}

fn fenced_error(owner_id: &str, epoch: u64) -> Error {
    Error::CommitterFenced {
        owner_id: owner_id.to_string(),
        epoch,
    }
}

fn validity_unknown_error(owner_id: &str, epoch: u64) -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        format!(
            "committer lease validity is unknown after the local renewal safety budget was exhausted for owner {owner_id} epoch {epoch}"
        ),
    )
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{SequenceNumber, SystemMonotonicClock, TenantId};
    use nimbus_storage::NoopFaultInjector;
    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::Engine;

    fn lease(expires_at: Timestamp) -> CommitterLease {
        CommitterLease {
            owner_id: "lease-owner".to_string(),
            epoch: 7,
            expires_at,
            durable_sequence: SequenceNumber(0),
        }
    }

    fn lease_test_runtime(
        tenant: &str,
    ) -> (
        TempDir,
        Arc<TenantRuntime>,
        Arc<CommitterLeaseLifecycle>,
        Arc<ManualLeaseRenewalClock>,
    ) {
        let data_dir = tempdir().expect("lease test tempdir should build");
        let engine = Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(nimbus_core::ManualWallClock::new(Timestamp(1_000))),
            Arc::new(NoopFaultInjector),
        )
        .expect("engine should create");
        let tenant_id = TenantId::new(tenant).expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let base_runtime = engine
            .tenant_runtime_for_testing(&tenant_id)
            .expect("base runtime should load");
        let clock = Arc::new(ManualLeaseRenewalClock::new());
        let runtime = Arc::new(
            TenantRuntime::from_parts(
                tenant_id,
                1,
                base_runtime.store.clone(),
                base_runtime.read_storage.clone(),
                Arc::new(SystemMonotonicClock),
                clock.clone(),
                Some("lease-owner".to_string()),
            )
            .expect("test runtime should construct"),
        );
        let lifecycle = runtime
            .committer_lease
            .as_ref()
            .expect("test runtime should own a lease lifecycle")
            .clone();
        (data_dir, runtime, lifecycle, clock)
    }

    #[test]
    fn lease_transient_failure_retries_before_local_safety_budget() {
        let last_success = Instant::now();
        for requested_duration in [ACQUIRE_LEASE_DURATION, RENEW_LEASE_DURATION] {
            let safety_deadline = local_safety_deadline(last_success, requested_duration);
            assert_eq!(
                safety_deadline.duration_since(last_success),
                requested_duration.saturating_sub(PROVIDER_EXPIRY_SAFETY_MARGIN)
            );
            let elapsed = requested_duration
                .saturating_sub(PROVIDER_EXPIRY_SAFETY_MARGIN)
                .saturating_sub(Duration::from_millis(1));
            let observed_at = last_success.checked_add(elapsed).unwrap();
            let deadline = transient_retry_deadline(
                observed_at,
                Some(safety_deadline),
                "owner-a",
                "tenant-a",
                8,
            )
            .expect("retry before the safety deadline should remain scheduled");
            assert!(deadline > observed_at);
            assert!(deadline <= safety_deadline);
            assert_eq!(
                transient_retry_deadline(
                    safety_deadline,
                    Some(safety_deadline),
                    "owner-a",
                    "tenant-a",
                    9,
                ),
                None,
                "no retry may be scheduled once the local safety budget is exhausted"
            );
        }
    }

    #[test]
    fn lease_retry_jitter_is_deterministic_and_bounded() {
        for streak in 1..=8 {
            let first = transient_retry_delay("owner-a", "tenant-a", streak);
            let second = transient_retry_delay("owner-a", "tenant-a", streak);
            assert_eq!(first, second);
            assert!(first >= TRANSIENT_RETRY_BASE);
            assert!(first <= TRANSIENT_RETRY_CAP + TRANSIENT_RETRY_CAP / 4);
        }
    }

    #[test]
    fn lease_renewal_success_resets_failure_streak() {
        let observed_at = Instant::now();
        let mut state = CommitterLeaseState {
            status: CommitterLeaseStatus::Held(lease(Timestamp(1))),
            acquire_count: 1,
            renewal_count: 0,
            renewal_failure_count: 3,
            renewal_failure_streak: 3,
            last_success_at: None,
            local_safety_deadline: None,
            next_renewal_at: observed_at,
        };

        record_renewal_success(&mut state, lease(Timestamp(99)), observed_at);

        assert_eq!(state.renewal_count, 1);
        assert_eq!(state.renewal_failure_count, 3);
        assert_eq!(state.renewal_failure_streak, 0);
        assert_eq!(state.last_success_at, Some(observed_at));
        assert_eq!(
            state.local_safety_deadline,
            Some(local_safety_deadline(observed_at, RENEW_LEASE_DURATION))
        );
        assert_eq!(
            state.next_renewal_at,
            observed_at.checked_add(RENEW_INTERVAL).unwrap()
        );
    }

    #[test]
    fn lease_stats_report_monotonic_age_since_last_success() {
        let clock = Arc::new(ManualLeaseRenewalClock::new());
        let lifecycle = CommitterLeaseLifecycle::new("lease-owner".to_string(), clock.clone());
        let observed_at = clock.now();
        {
            let mut state = lifecycle.state.lock().expect("lease state should lock");
            state.status = CommitterLeaseStatus::Held(lease(Timestamp(123_456)));
            state.last_success_at = Some(observed_at);
        }
        clock.advance(Duration::from_millis(2_500));

        assert_eq!(lifecycle.stats().last_success_age_millis, Some(2_500));
    }

    #[test]
    fn lease_stats_never_compare_provider_expiry_to_local_wall_clock() {
        let clock = Arc::new(ManualLeaseRenewalClock::new());
        let lifecycle = CommitterLeaseLifecycle::new("lease-owner".to_string(), clock.clone());
        let observed_at = clock.now();
        {
            let mut state = lifecycle.state.lock().expect("lease state should lock");
            state.status = CommitterLeaseStatus::Held(lease(Timestamp(u64::MAX)));
            state.last_success_at = Some(observed_at);
        }
        clock.advance(Duration::from_millis(42));

        let stats = lifecycle.stats();
        assert_eq!(stats.expires_at, Timestamp(u64::MAX));
        assert_eq!(stats.last_success_age_millis, Some(42));
    }

    #[test]
    fn lease_shutdown_during_provider_error_drains_worker() {
        let (_data_dir, runtime, lifecycle, clock) = lease_test_runtime("lease-provider-error");
        {
            let mut state = lifecycle.state.lock().expect("lease state should lock");
            let observed_at = clock.now();
            state.status = CommitterLeaseStatus::Held(lease(Timestamp(999_999)));
            state.last_success_at = Some(observed_at);
            state.next_renewal_at = observed_at;
        }
        lifecycle.start_worker(&runtime);

        let deadline = Instant::now() + Duration::from_secs(1);
        while lifecycle.stats().renewal_failure_count == 0 {
            assert!(
                Instant::now() < deadline,
                "provider error should be observed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        lifecycle.shutdown();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!lifecycle.stats().renewal_worker_running);
    }

    #[test]
    fn lease_safety_budget_exhaustion_fails_closed() {
        let (_data_dir, runtime, lifecycle, clock) = lease_test_runtime("lease-budget-exhaustion");
        {
            let mut state = lifecycle.state.lock().expect("lease state should lock");
            let observed_at = clock.now();
            state.status = CommitterLeaseStatus::Held(lease(Timestamp(999_999)));
            state.last_success_at = Some(observed_at);
            state.local_safety_deadline = Some(observed_at);
            state.next_renewal_at = observed_at;
        }

        assert!(!lifecycle.renew_once(runtime.as_ref()));
        let error = runtime
            .committer_fenced_error()
            .expect("an exhausted safety budget must block later assignments");
        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Unavailable));
        assert!(
            error
                .storage_message()
                .is_some_and(|message| message.contains("validity is unknown"))
        );
        assert_eq!(lifecycle.source_epoch().unwrap(), 7);
        let stats = lifecycle.stats();
        assert!(!stats.acquired);
        assert!(!stats.fenced);
        assert_eq!(stats.epoch, 7);
        assert_eq!(stats.renewal_failure_count, 1);
        assert_eq!(stats.renewal_failure_streak, 1);
    }
}
