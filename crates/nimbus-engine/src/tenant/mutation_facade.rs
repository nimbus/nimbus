use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use nimbus_core::{CommitEntry, Result, SequenceNumber, TableName, TenantEventRecord, Timestamp};
use nimbus_storage::JournalProgress;

use super::*;

pub(crate) struct WriteLogAppendGuard<'a> {
    runtime: &'a TenantRuntime,
    baseline_durable_head: SequenceNumber,
    armed: bool,
}

impl WriteLogAppendGuard<'_> {
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for WriteLogAppendGuard<'_> {
    fn drop(&mut self) {
        if !self.armed || !self.runtime.store.has_process_local_sequence_authority() {
            return;
        }
        let progress = self.runtime.store.journal_progress();
        if append_exit_requires_storage_fallback(self.baseline_durable_head, &progress) {
            self.runtime.write_log.mark_coverage_unknown();
            tracing::warn!(
                tenant = %self.runtime.tenant_id(),
                baseline_durable_head = %self.baseline_durable_head,
                observed_progress = ?progress,
                "write-log coverage became unknown after an ambiguous persistence exit; using storage conflict validation until tenant runtime restart"
            );
        }
    }
}

fn append_exit_requires_storage_fallback(
    baseline_durable_head: SequenceNumber,
    progress: &Result<JournalProgress>,
) -> bool {
    match progress {
        Ok(progress) => progress.durable_head > baseline_durable_head,
        Err(_) => true,
    }
}

impl TenantRuntime {
    /// Samples and advances the tenant commit clock while the sequence gate is held.
    pub(crate) fn assign_commit_timestamp(&self) -> Timestamp {
        let previous = self.last_assigned_commit_timestamp.load(Ordering::Relaxed);
        let timestamp = Timestamp(self.store.now().0.max(previous));
        debug_assert!(
            timestamp.0 >= previous,
            "assigned commit timestamps must be monotonic"
        );
        self.last_assigned_commit_timestamp
            .store(timestamp.0, Ordering::Relaxed);
        timestamp
    }

    pub(crate) fn enqueue_mutation_admission_request(
        &self,
        request: QueuedMutationRequest,
    ) -> Result<bool> {
        self.mutation_admission.enqueue(request)?;
        Ok(self.mutation_journal.try_start_worker())
    }

    pub(crate) fn drain_mutation_admission_queue(&self) {
        loop {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => {
                    if let Err(enqueue_error) = self.mutation_journal.enqueue(request) {
                        let (request, error) = *enqueue_error;
                        let _ = request.response.send(Err(error));
                    }
                }
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
    }

    pub(crate) async fn drain_mutation_batch_adaptive(
        &self,
        base_batch_size: usize,
        max_batch_size: usize,
        coalesce: Duration,
    ) -> Vec<QueuedMutationRequest> {
        #[cfg(test)]
        self.mutation_journal.wait_before_drain().await;

        let base_batch_size = base_batch_size.max(1);
        let max_batch_size = max_batch_size.max(base_batch_size);
        let initial_backlog = self
            .mutation_journal
            .queue_depth()
            .saturating_add(self.mutation_admission.queue_depth());
        if !coalesce.is_zero() && (1..=base_batch_size).contains(&initial_backlog) {
            // Tokio time is intentional: tests may use a paused/advanced Tokio
            // clock, and the committer must never consult ambient wall time for
            // scheduling decisions.
            tokio::time::sleep(coalesce).await;
        }

        // Include arrivals admitted while the optional coalescing window or
        // deterministic pre-drain pause was active before choosing the cap.
        // Do not first transfer them into the bounded journal queue: that
        // queue may already be full while the worker is paused. Draining the
        // journal first and then admitting directly into this batch preserves
        // the admission buffer instead of spuriously rejecting queued work.
        let backlog = self
            .mutation_journal
            .queue_depth()
            .saturating_add(self.mutation_admission.queue_depth());
        let batch_limit = if backlog > base_batch_size {
            max_batch_size
        } else {
            base_batch_size
        };
        let mut batch = self.mutation_journal.drain_batch(batch_limit).await;
        while batch.len() < batch_limit {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => batch.push(request),
                MutationAdmissionDecision::Reject { request, error } => {
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
        batch
    }

    pub(crate) fn release_mutation_worker(&self) -> bool {
        // Pass the admission check as a CLOSURE so `release_worker` evaluates it
        // after clearing `worker_running`. Passing `has_pending()` by value would
        // read the admission queue before the store and reopen a lost-wakeup
        // deadlock (see MutationJournalState::release_worker for the ordering).
        self.mutation_journal
            .release_worker(|| self.mutation_admission.has_pending())
    }

    pub(crate) fn record_mutation_worker_start(&self) {
        self.mutation_journal.record_worker_start();
    }

    pub(crate) fn record_mutation_worker_failure(&self) {
        self.mutation_journal.record_worker_failure();
    }

    pub(crate) fn begin_pending_mutation_response(&self) {
        self.mutation_journal.begin_pending_response();
    }

    pub(crate) fn finish_pending_mutation_response(&self) {
        self.mutation_journal.finish_pending_response();
    }

    pub(crate) fn durable_head(&self) -> SequenceNumber {
        self.mutation_journal.durable_head()
    }

    pub(crate) fn applied_head(&self) -> SequenceNumber {
        self.mutation_journal.applied_head()
    }

    pub(crate) fn lock_mutation_sequence(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_journal.lock_sequence_gate()
    }

    pub(crate) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_durable_head(sequence);
    }

    pub(crate) fn mark_applied_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_applied_head(sequence);
    }

    pub(crate) fn stage_pending_write_log_commits(
        &self,
        commits: impl IntoIterator<Item = CommitEntry>,
        observed_at: Timestamp,
    ) {
        self.write_log.stage_pending(commits, observed_at);
    }

    pub(crate) fn publish_write_log_through(&self, applied_head: SequenceNumber) {
        let reader_frontier = self
            .subscription_registry()
            .lowest_active_delivery_sequence(applied_head)
            .min(applied_head);
        self.write_log
            .publish_pending_through(applied_head, self.store.now(), reader_frontier);
    }

    pub(crate) fn advance_write_log_zero_write_coverage(&self, sequence: SequenceNumber) {
        self.write_log.advance_known_zero_write_through(sequence);
    }

    pub(crate) fn stage_zero_write_record_in_write_log(&self, record: &TenantEventRecord) {
        self.write_log
            .stage_zero_write_record(record, self.store.now());
    }

    pub(crate) fn published_schema_epoch_snapshot(
        &self,
    ) -> std::collections::HashMap<TableName, SequenceNumber> {
        self.write_log.published_schema_epoch_snapshot()
    }

    pub(crate) fn current_schema_epoch(&self, table: &TableName) -> SequenceNumber {
        self.write_log.current_schema_epoch(table)
    }

    /// Arms a fail-safe around a persistence attempt. The caller disarms only
    /// after every returned commit image has been staged in the write log.
    pub(crate) fn arm_write_log_append(&self) -> WriteLogAppendGuard<'_> {
        WriteLogAppendGuard {
            runtime: self,
            baseline_durable_head: self.durable_head(),
            armed: true,
        }
    }

    pub(crate) async fn wait_for_applied_sequence_cancellable<Fut>(
        &self,
        sequence: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        self.mutation_journal
            .wait_for_applied_sequence_cancellable(sequence, cancel_wait)
            .await
    }

    pub(crate) fn wait_for_applied_sequence_blocking(&self, sequence: SequenceNumber) {
        self.mutation_journal
            .wait_for_applied_sequence_blocking(sequence);
    }

    pub(crate) fn sync_mutation_journal_progress(&self, progress: JournalProgress) {
        let _sequence_guard = self.lock_mutation_sequence();
        self.sync_mutation_journal_progress_locked(progress);
    }

    /// Async-context form of [`Self::sync_mutation_journal_progress`].
    ///
    /// The gated sync blocks on the sequence gate (a std mutex). Blocking a
    /// tokio worker on it can starve the reactor while a gate holder drives
    /// provider storage I/O through a runtime bridge — a real deadlock
    /// observed on postgres-backed tenants (provider notification listener
    /// vs a direct mutation holding the gate). Async callers must use this
    /// form, which parks the wait on the blocking pool instead.
    pub(crate) async fn sync_mutation_journal_progress_async(
        self: &Arc<Self>,
        progress: JournalProgress,
    ) {
        let runtime = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            runtime.sync_mutation_journal_progress(progress);
        })
        .await
        .expect("sync_mutation_journal_progress task panicked");
    }

    /// Synchronizes recovered heads while the caller holds the mutation
    /// sequence gate. Callers already inside a serial mutation section use
    /// this non-reentrant form; all other callers use the gated wrapper above.
    pub(crate) fn sync_mutation_journal_progress_locked(&self, progress: JournalProgress) {
        self.write_log
            .observe_assigned_through_without_coverage(progress.durable_head);
        self.write_log
            .rebase_empty_after_recovery(progress.applied_head, progress.durable_head);
        self.mark_durable_head(progress.durable_head);
        self.publish_write_log_through(progress.applied_head);
        self.mark_applied_head(progress.applied_head);
    }

    pub(crate) fn mutation_admission_stats(&self) -> MutationAdmissionStats {
        self.mutation_admission.stats()
    }

    pub(crate) fn mutation_journal_stats(&self) -> MutationJournalStats {
        self.mutation_journal.stats()
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_journal_queue_capacity_for_testing(&self, capacity: usize) {
        self.mutation_journal.set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn set_mutation_admission_codel_for_testing(
        &self,
        target: Duration,
        interval: Duration,
    ) {
        self.mutation_admission
            .set_codel_for_testing(target, interval);
    }

    #[cfg(test)]
    pub(crate) fn mutation_journal_pause_handle_for_testing(&self) -> MutationJournalPauseHandle {
        self.mutation_journal.pause_handle()
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn subscription_bootstrap_pause_handle_for_testing(
        &self,
    ) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle::from_state(self.subscription_bootstrap_pause.clone())
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_if_subscription_bootstrap_pause_armed(&self) {
        self.subscription_bootstrap_pause.wait_if_armed().await;
    }
}

#[cfg(test)]
mod write_log_append_guard_tests {
    use nimbus_core::{Error, SequenceNumber};
    use nimbus_storage::JournalProgress;

    use super::append_exit_requires_storage_fallback;

    #[test]
    fn write_log_append_guard_distinguishes_clean_rollback_from_ambiguous_exit() {
        let baseline = SequenceNumber(4);
        let clean_rollback = Ok(JournalProgress {
            durable_head: baseline,
            applied_head: baseline,
        });
        let durable_advance = Ok(JournalProgress {
            durable_head: SequenceNumber(5),
            applied_head: SequenceNumber(5),
        });
        let unknown_progress = Err(Error::Internal("progress unavailable".to_string()));

        assert!(!append_exit_requires_storage_fallback(
            baseline,
            &clean_rollback
        ));
        assert!(append_exit_requires_storage_fallback(
            baseline,
            &durable_advance
        ));
        assert!(append_exit_requires_storage_fallback(
            baseline,
            &unknown_progress
        ));
    }
}
