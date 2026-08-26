use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use nimbus_core::{
    CommitEntry, Error, Result, SequenceNumber, TableName, TenantEventRecord, Timestamp,
};
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
    pub(crate) fn take_committer_receiver(&self) -> tokio::sync::mpsc::Receiver<CommitterMessage> {
        self.committer.take_receiver()
    }

    pub(crate) fn take_publisher_receiver(&self) -> tokio::sync::mpsc::Receiver<PublisherMessage> {
        self.publisher.take_receiver()
    }

    pub(crate) fn take_observer_dispatch_receiver(
        &self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<
        crate::engine::committed_mutations::CommittedMutationObserverMessage,
    > {
        self.observer_dispatch.take_receiver()
    }

    pub(crate) fn enqueue_committed_mutation_observer_dispatch(
        self: &Arc<Self>,
        dispatch: crate::engine::committed_mutations::CommittedMutationObserverDispatch,
    ) -> Result<()> {
        self.observer_dispatch.send(dispatch, Arc::downgrade(self))
    }

    pub(crate) async fn enqueue_committed_mutation_observer_catch_up_dispatch(
        self: &Arc<Self>,
        dispatch: crate::engine::committed_mutations::CommittedMutationObserverDispatch,
    ) -> Result<()> {
        self.observer_dispatch
            .send_when_capacity_available(dispatch, Arc::downgrade(self))
            .await
    }

    pub(crate) fn committed_mutation_observer_catch_up_chunk_size(&self) -> usize {
        self.observer_dispatch.catch_up_chunk_size()
    }

    pub(crate) fn claim_committed_mutation_observer_through(
        &self,
        through: SequenceNumber,
    ) -> SequenceNumber {
        self.observer_dispatch
            .claim_observer_publication_through(through)
    }

    pub(crate) fn request_committed_mutation_observer_catch_up(
        &self,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: crate::engine::ProjectionToken,
    ) -> bool {
        self.observer_dispatch
            .request_catch_up(first_sequence, requested_through, projection_token)
    }

    pub(crate) fn take_committed_mutation_observer_catch_up_request(
        &self,
    ) -> Option<(
        SequenceNumber,
        SequenceNumber,
        crate::engine::ProjectionToken,
    )> {
        self.observer_dispatch.take_catch_up_request()
    }

    pub(crate) fn complete_committed_mutation_observer_catch_up(&self) -> bool {
        self.observer_dispatch.complete_catch_up()
    }

    pub(crate) fn abandon_committed_mutation_observer_catch_up(
        &self,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: crate::engine::ProjectionToken,
    ) {
        self.observer_dispatch.abandon_catch_up(
            first_sequence,
            requested_through,
            projection_token,
        );
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_committed_mutation_observer_catch_up_idle(&self) {
        self.observer_dispatch.wait_for_catch_up_idle().await;
    }

    #[cfg(test)]
    pub(crate) fn record_committed_mutation_observer_catch_up_task_started(&self) {
        self.observer_dispatch.record_catch_up_task_started();
    }

    #[cfg(test)]
    pub(crate) fn record_committed_mutation_observer_catch_up_task_finished(&self) {
        self.observer_dispatch.record_catch_up_task_finished();
    }

    #[cfg(test)]
    pub(crate) fn committed_mutation_observer_catch_up_task_count(&self) -> usize {
        self.observer_dispatch.catch_up_task_count()
    }

    pub(crate) fn record_committed_mutation_observer_catch_up_enqueue_failure(&self) {
        self.observer_dispatch.record_catch_up_enqueue_failure();
    }

    pub(crate) fn close_committed_mutation_observers(&self) {
        self.observer_dispatch.close();
    }

    pub(crate) fn mark_committed_mutation_observers_drained(&self) {
        self.observer_dispatch.mark_drained();
    }

    pub(crate) fn complete_committed_mutation_observer_dispatch(&self, event_count: usize) {
        self.observer_dispatch.complete_dispatch(event_count);
    }

    pub(crate) fn poison_committed_mutation_observers(&self, reason: &str) {
        self.observer_dispatch.poison(reason);
    }

    pub(crate) async fn wait_for_committed_mutation_observers_drained(&self) {
        self.observer_dispatch.wait_drained().await;
    }

    pub(crate) async fn wait_for_committed_mutation_observers_drained_for_eviction(
        &self,
    ) -> Result<()> {
        self.observer_dispatch.wait_drained_for_eviction().await
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn flush_committed_mutation_observers_for_testing(&self) -> Result<()> {
        self.observer_dispatch.fence().await
    }

    pub(crate) fn uses_ordered_publisher(&self) -> bool {
        self.publisher.uses_ordered_publisher()
    }

    pub(crate) async fn lock_publisher_assignment_recovery(
        &self,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        self.publisher.lock_assignment_recovery().await
    }

    pub(crate) fn committer_shutdown_token(&self) -> tokio_util::sync::CancellationToken {
        self.committer.shutdown_token()
    }

    pub(crate) fn shutdown_committer(&self) {
        #[cfg(any(test, feature = "test-hooks"))]
        self.publisher.release_test_pause_for_shutdown();
        self.committer.shutdown();
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_for_ordered_publisher_pause_for_testing(&self) {
        self.publisher.wait_for_test_pause().await;
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn ordered_publisher_pause_handle_for_testing(
        &self,
    ) -> super::OrderedPublisherPauseHandle {
        self.publisher.pause_handle()
    }

    pub(crate) fn publisher_record_transient_error(&self) {
        self.publisher.record_transient_error();
    }

    pub(crate) fn publisher_record_fatal_error(&self) {
        self.publisher.record_fatal_error();
    }

    pub(crate) fn publisher_record_ambiguous_error(&self) {
        self.publisher.record_ambiguous_error();
    }

    pub(crate) async fn send_assigned_publisher_batch(
        &self,
        batch: AssignedPublisherBatch,
    ) -> std::result::Result<(), PublisherQueueError> {
        self.publisher.send(batch).await
    }

    pub(crate) async fn send_publisher_response_fence(
        &self,
        responses: Vec<DeferredPublisherResponse>,
    ) -> std::result::Result<(), Box<super::mutation::PublisherResponseFenceError>> {
        self.publisher.send_response_fence(responses).await
    }

    pub(crate) fn mark_publisher_finished(&self) {
        self.publisher.mark_finished();
    }

    pub(crate) async fn wait_for_publisher_finished(&self) {
        self.publisher.wait_finished().await;
    }

    pub(crate) async fn send_publisher_ordered_opaque_job(
        &self,
        job: super::CommitterJob,
    ) -> std::result::Result<tokio::sync::oneshot::Receiver<()>, (super::CommitterJob, Error)> {
        self.publisher.send_ordered_opaque_job(job).await
    }

    pub(crate) async fn send_queued_committer_batch(
        &self,
        engine: Arc<crate::Engine>,
    ) -> Result<()> {
        let result = self.committer.send_queued_batch_async(engine).await;
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    pub(crate) fn accept_queued_committer_batch(&self, owns_pending_wake: bool) {
        self.committer.accept_queued_batch(owns_pending_wake);
    }

    pub(crate) fn submit_direct_committer_then<T, F, A>(
        &self,
        task: F,
        after_commit: A,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
        A: FnOnce(&T),
    {
        let result =
            self.committer
                .submit_blocking_then(CommitterMessage::DirectCommit, task, after_commit);
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    pub(crate) fn submit_execution_unit_committer_then<T, F, A>(
        &self,
        task: F,
        after_commit: A,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
        A: FnOnce(&T),
    {
        let result = self.committer.submit_blocking_then(
            CommitterMessage::ExecutionUnitCommit,
            task,
            after_commit,
        );
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    pub(crate) fn submit_internal_committer<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let result = self
            .committer
            .submit_blocking(CommitterMessage::InternalCommit, task);
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    pub(crate) async fn submit_internal_committer_async<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let result = self
            .committer
            .submit_async(CommitterMessage::InternalCommit, task)
            .await;
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    pub(crate) async fn submit_journal_progress_committer(
        self: &Arc<Self>,
        progress: JournalProgress,
    ) -> Result<()> {
        let runtime = Arc::clone(self);
        let result = self
            .committer
            .submit_async(CommitterMessage::JournalProgressSync, move || {
                runtime.sync_mutation_journal_progress_in_actor(progress);
                Ok(())
            })
            .await;
        if let Err(error) = &result {
            self.maybe_report_overload_error(error);
        }
        result
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) async fn wait_before_mutation_drain(&self) {
        self.mutation_journal.wait_before_drain().await;
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_queued_mutation_cancellation_observed_for_testing(&self) {
        self.mutation_journal
            .wait_for_queued_cancellation_observed()
            .await;
    }

    #[cfg(test)]
    pub(crate) fn record_queued_mutation_cancellation_observed_for_testing(&self) {
        self.mutation_journal.record_queued_cancellation_observed();
    }

    /// Samples and advances the tenant commit clock on the committer actor.
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
    ) -> Result<()> {
        if let Err(error) = self.mutation_admission.enqueue(request, || {
            self.lifecycle
                .operation_rejection_if_deleted(&self.tenant_id)
        }) {
            self.maybe_report_overload_error(&error);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn fail_and_drain_mutation_queues(&self, error: &Error) {
        let mut requests = self.mutation_admission.drain_all();
        requests.extend(self.mutation_journal.drain_all());
        for request in requests {
            let response = request.response.clone();
            // Drop prepared accounting and the tenant operation guard before
            // waking the caller or waiting on any engine-wide lock.
            drop(request);
            let _ = response.send(Err(error.clone()));
        }
    }

    fn maybe_report_overload_error(&self, error: &nimbus_core::Error) {
        if error.is_overload_class() && self.commit_phases.record_overload_error() {
            tracing::warn!(
                tenant = %self.tenant_id,
                error = %error,
                retryability = ?error.retryability(),
                "mutation overload-class error"
            );
        }
    }

    pub(crate) fn drain_mutation_admission_queue(&self) {
        loop {
            match self.mutation_admission.pop_next_at(Instant::now()) {
                MutationAdmissionDecision::Admit(request) => {
                    if let Err(enqueue_error) = self.mutation_journal.enqueue(request) {
                        let (request, error) = *enqueue_error;
                        self.maybe_report_overload_error(&error);
                        let _ = request.response.send(Err(error));
                    }
                }
                MutationAdmissionDecision::Reject { request, error } => {
                    self.maybe_report_overload_error(&error);
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
    }

    pub(crate) fn mutation_assignment_backlog_depth(&self) -> usize {
        self.mutation_journal
            .queue_depth()
            .saturating_add(self.mutation_admission.queue_depth())
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
                    self.maybe_report_overload_error(&error);
                    let _ = request.response.send(Err(error));
                }
                MutationAdmissionDecision::Empty => break,
            }
        }
        batch
    }

    pub(crate) fn record_mutation_worker_start(&self) {
        self.mutation_journal.record_worker_start();
    }

    pub(crate) fn set_mutation_worker_running(&self, running: bool) {
        self.mutation_journal.set_worker_running(running);
    }

    pub(crate) fn record_mutation_worker_failure(&self) {
        self.mutation_journal.record_worker_failure();
    }

    pub(crate) fn record_provider_catch_up_failure(&self) {
        self.mutation_journal.record_provider_catch_up_failure();
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

    pub(crate) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.mutation_journal.mark_durable_head(sequence);
    }

    pub(crate) fn metadata_retention_controller(
        &self,
    ) -> Arc<crate::engine::metadata_retention::MetadataRetentionController> {
        self.metadata_retention.clone()
    }

    pub(crate) fn shutdown_metadata_retention(&self) {
        self.metadata_retention.shutdown();
    }

    pub(crate) fn wait_for_metadata_retention_finished_blocking(&self) {
        self.metadata_retention.wait_finished_blocking();
    }

    pub(crate) async fn wait_for_metadata_retention_finished(&self) {
        self.metadata_retention.wait_finished().await;
    }

    pub(crate) fn mark_applied_head(&self, sequence: SequenceNumber) {
        debug_assert!(
            sequence <= self.write_log.published_through(),
            "engine applied watermark {} cannot exceed write-log published frontier {}",
            sequence,
            self.write_log.published_through()
        );
        self.mutation_journal.mark_applied_head(sequence);
        self.metadata_retention.notify_progress(sequence);
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn fail_applied_waiters_for_testing(&self, error: Error) {
        self.mutation_journal.fail_applied_waiters(error);
    }

    pub(crate) fn stage_pending_write_log_commits(
        &self,
        commits: impl IntoIterator<Item = CommitEntry>,
        observed_at: Timestamp,
    ) {
        self.write_log.stage_pending(commits, observed_at);
    }

    pub(crate) fn assigned_head(&self) -> SequenceNumber {
        self.write_log.assigned_through()
    }

    pub(crate) fn discard_unpersisted_write_log_suffix(&self, first: SequenceNumber) {
        self.write_log.discard_unpersisted_suffix(first);
    }

    pub(crate) fn publish_write_log_through(&self, applied_head: SequenceNumber) -> SequenceNumber {
        let reader_frontier = self
            .subscription_registry()
            .lowest_active_delivery_sequence(applied_head)
            .min(applied_head);
        self.write_log
            .publish_pending_through(applied_head, self.store.now(), reader_frontier)
    }

    fn observe_write_log_applied_through(&self, applied_head: SequenceNumber) -> SequenceNumber {
        let reader_frontier = self
            .subscription_registry()
            .lowest_active_delivery_sequence(applied_head)
            .min(applied_head);
        self.write_log
            .observe_applied_through(applied_head, self.store.now(), reader_frontier)
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

    pub(crate) fn wait_for_applied_sequence_blocking(
        &self,
        sequence: SequenceNumber,
    ) -> Result<()> {
        self.mutation_journal
            .wait_for_applied_sequence_blocking(sequence)
    }

    pub(crate) fn sync_mutation_journal_progress(self: &Arc<Self>, progress: JournalProgress) {
        let runtime = Arc::clone(self);
        self.submit_internal_committer(move || {
            runtime.sync_mutation_journal_progress_in_actor(progress);
            Ok(())
        })
        .expect("tenant committer should synchronize journal progress");
    }

    /// Async-context form of [`Self::sync_mutation_journal_progress`].
    ///
    /// Async callers enqueue without blocking a Tokio worker and await the
    /// actor's response. This preserves the PPSC2-A provider-listener
    /// deadlock fix without a blocking gate bridge.
    pub(crate) async fn sync_mutation_journal_progress_async(
        self: &Arc<Self>,
        progress: JournalProgress,
    ) -> Result<()> {
        self.submit_journal_progress_committer(progress).await
    }

    /// Observes storage-side heads from within the committer task. Callers
    /// already inside that serial section must use this non-reentrant form.
    /// Observation never claims ownership of pending write-log publication.
    pub(crate) fn sync_mutation_journal_progress_in_actor(&self, progress: JournalProgress) {
        self.sync_mutation_journal_heads(progress);
        let published_frontier = self.observe_write_log_applied_through(progress.applied_head);
        self.mark_applied_head(published_frontier);
    }

    /// Synchronizes progress after this actor path explicitly applied or
    /// recovered the durable prefix named by `progress`.
    pub(crate) fn publish_mutation_journal_progress_in_actor(&self, progress: JournalProgress) {
        self.sync_mutation_journal_heads(progress);
        let published_frontier = self.publish_write_log_through(progress.applied_head);
        self.mark_applied_head(published_frontier);
    }

    fn sync_mutation_journal_heads(&self, progress: JournalProgress) {
        self.write_log
            .observe_assigned_through_without_coverage(progress.durable_head);
        self.write_log
            .rebase_empty_after_recovery(progress.applied_head, progress.durable_head);
        self.mark_durable_head(progress.durable_head);
    }

    pub(crate) fn mutation_admission_stats(&self) -> MutationAdmissionStats {
        self.mutation_admission.stats()
    }

    pub(crate) fn mutation_journal_stats(&self) -> MutationJournalStats {
        let journal_before = self.mutation_journal.frontier_sample();
        let write_log = self.write_log.frontier_sample();
        let journal_after = self.mutation_journal.frontier_sample();
        let frontiers = MutationFrontierStats::reconcile(write_log, journal_before, journal_after);
        let mut stats = self.mutation_journal.stats(frontiers);
        stats.committer_inbox_depth = self.committer.depth();
        stats.committer_inbox_capacity = self.committer.capacity();
        stats.committer_send_timeout_millis =
            u64::try_from(self.committer.send_timeout().as_millis()).unwrap_or(u64::MAX);
        stats.committer_send_timeout_count = self.committer.send_timeout_count();
        let lease = self.committer_lease_stats();
        stats.committer_lease_acquired = lease.acquired;
        stats.committer_lease_epoch = lease.epoch;
        stats.committer_lease_expires_at = lease.expires_at;
        stats.committer_lease_fenced = lease.fenced;
        stats.committer_lease_acquire_count = lease.acquire_count;
        stats.committer_lease_renewal_count = lease.renewal_count;
        stats.committer_lease_renewal_failure_count = lease.renewal_failure_count;
        stats.committer_lease_renewal_failure_streak = lease.renewal_failure_streak;
        stats.committer_lease_last_success_age_millis = lease.last_success_age_millis;
        stats.committer_lease_renewal_worker_running = lease.renewal_worker_running;
        stats.publisher_queue_depth = self.publisher.depth();
        stats.publisher_queue_capacity = self.publisher.capacity();
        stats.publisher_send_timeout_count = self.publisher.send_timeout_count();
        let errors = self.publisher.error_counts();
        stats.publisher_transient_error_count = errors.transient;
        stats.publisher_fatal_error_count = errors.fatal;
        stats.publisher_ambiguous_error_count = errors.ambiguous;
        stats.committer_arm = self.publisher.arm();
        let observer = self.observer_dispatch.stats();
        stats.observer_queue_depth = observer.depth;
        stats.observer_queue_peak_depth = observer.peak_depth;
        stats.observer_queue_capacity = observer.capacity;
        stats.observer_queue_high_watermark = observer.high_watermark;
        stats.observer_queue_high_water_warning_count = observer.high_water_warning_count;
        stats.observer_queue_cap_breach_count = observer.cap_breach_count;
        stats.observer_catch_up_enqueue_failure_count = observer.catch_up_enqueue_failure_count;
        stats.observer_dispatch_poisoned = observer.poisoned;
        stats
    }

    pub(crate) fn publisher_error_counts(&self) -> super::PublisherErrorCounts {
        self.publisher.error_counts()
    }

    pub(crate) fn restore_publisher_error_counts(&self, counts: super::PublisherErrorCounts) {
        self.publisher.restore_error_counts(counts);
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

    #[cfg(any(test, feature = "test-hooks"))]
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
