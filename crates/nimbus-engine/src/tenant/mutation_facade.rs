#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use nimbus_core::{Result, SequenceNumber};
use nimbus_storage::JournalProgress;

use super::*;

impl TenantRuntime {
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

    pub(crate) async fn drain_mutation_batch(
        &self,
        max_batch_size: usize,
    ) -> Vec<QueuedMutationRequest> {
        #[cfg(test)]
        self.mutation_journal.wait_before_drain().await;
        let mut batch = self.mutation_journal.drain_batch(max_batch_size).await;
        let batch_limit = max_batch_size.max(1);
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
        self.mutation_journal
            .release_worker(self.mutation_admission.has_pending())
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
        self.mark_durable_head(progress.durable_head);
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
