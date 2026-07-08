use std::collections::HashSet;
use std::sync::Arc;

use nimbus_core::{CommitEntry, Document, DocumentId, SequenceNumber, TableName};

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionBatchCandidate, dispatch_subscription_work,
};
use crate::{Engine, tenant::TenantRuntime};

pub(super) fn candidate_documents_for_commit(commit: &CommitEntry) -> Vec<Document> {
    commit
        .writes
        .iter()
        .filter_map(|write| match write.op_type {
            nimbus_core::WriteOpType::Insert => write.current.clone(),
            nimbus_core::WriteOpType::Update => None,
            nimbus_core::WriteOpType::Delete => write.previous.clone(),
        })
        .collect()
}

fn deleted_documents_for_commit(commit: &CommitEntry) -> Vec<Document> {
    commit
        .writes
        .iter()
        .filter(|write| matches!(write.op_type, nimbus_core::WriteOpType::Delete))
        .filter_map(|write| write.previous.clone())
        .collect()
}

/// Returns the identity of the batch's sole document-bearing commit, if
/// exactly one exists. Zero-write commits (e.g. a trigger-candidate delivery
/// cursor advance) share the same commit log and sequence space as real
/// document commits and can land in the same coalesced batch, but they never
/// carry a document write and so can never themselves be the subject of a
/// subscription update -- they must not count toward "more than one commit"
/// when deciding whether to preserve exact commit metadata.
fn document_bearing_commit_identity(applied: &[CommitEntry]) -> Option<CommitEntry> {
    let mut document_bearing = applied.iter().filter(|commit| !commit.writes.is_empty());
    match (document_bearing.next(), document_bearing.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

fn merge_deleted_documents_for_batch(applied: &[CommitEntry]) -> Vec<Document> {
    let mut seen = HashSet::<(TableName, DocumentId)>::new();
    let mut deleted_documents = Vec::new();
    for commit in applied {
        for document in commit
            .writes
            .iter()
            .filter(|write| matches!(write.op_type, nimbus_core::WriteOpType::Delete))
            .filter_map(|write| write.previous.as_ref())
        {
            let key = (document.table.clone(), document.id.clone());
            if seen.insert(key) {
                deleted_documents.push(document.clone());
            }
        }
    }
    deleted_documents
}

impl Engine {
    pub(crate) fn dispatch_or_enqueue_trigger_candidates(
        &self,
        runtime: Arc<TenantRuntime>,
        commits: Vec<CommitEntry>,
    ) {
        if commits.is_empty() {
            return;
        }
        runtime.ensure_trigger_candidate_worker_started();
        runtime.enqueue_trigger_commit_batch(commits);
    }

    pub(crate) fn bootstrap_trigger_candidate_feed(
        &self,
        runtime: Arc<TenantRuntime>,
    ) -> nimbus_core::Result<()> {
        let cursor = runtime.store.trigger_delivery_cursor()?;
        let next_sequence = SequenceNumber(cursor.materialized_through.0.saturating_add(1));
        if next_sequence.0 > runtime.applied_head().0 {
            return Ok(());
        }
        let commits = runtime.store.read_commit_log_from(next_sequence)?;
        self.dispatch_or_enqueue_trigger_candidates(runtime, commits);
        Ok(())
    }

    pub(crate) fn bootstrap_trigger_execution(
        &self,
        runtime: Arc<TenantRuntime>,
    ) -> nimbus_core::Result<()> {
        let Some(executor) = self.trigger_invocation_executor() else {
            return Ok(());
        };
        runtime.ensure_trigger_execution_worker_started(self.clock.clone(), executor);
        let scheduled = runtime
            .store
            .list_trigger_invocations()?
            .into_iter()
            .filter_map(|record| match record.state {
                nimbus_core::TriggerInvocationState::Pending => {
                    Some((record.key, nimbus_core::Timestamp(0)))
                }
                nimbus_core::TriggerInvocationState::RetryPending {
                    next_attempt_at, ..
                } => Some((record.key, next_attempt_at)),
                nimbus_core::TriggerInvocationState::Running { .. } => {
                    Some((record.key, nimbus_core::Timestamp(0)))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        runtime.enqueue_trigger_invocation_scheduled(scheduled);
        Ok(())
    }

    pub(crate) fn dispatch_or_enqueue_subscription_work(
        &self,
        runtime: Arc<TenantRuntime>,
        work: QueuedSubscriptionWork,
    ) {
        runtime.ensure_subscription_delivery_worker_started();
        let work = match runtime.enqueue_subscription_work(work) {
            Ok(()) => return,
            Err(work) => work,
        };

        let metrics = runtime.subscription_delivery_metrics();
        metrics.record_overflow_sync_fallback();
        let stats = dispatch_subscription_work(&runtime, &work);
        metrics.record_dispatch_stats(stats);
    }

    pub(crate) fn process_commit(&self, runtime: Arc<TenantRuntime>, commit: &CommitEntry) {
        let candidate_documents = candidate_documents_for_commit(commit);
        let subscription_ids = runtime
            .subscriptions
            .affected_subscription_ids(commit, &candidate_documents);
        if !subscription_ids.is_empty() {
            let work = QueuedSubscriptionWork::new_single(
                subscription_ids,
                commit.clone(),
                deleted_documents_for_commit(commit),
            );
            self.dispatch_or_enqueue_subscription_work(runtime.clone(), work);
        }
        self.dispatch_or_enqueue_trigger_candidates(runtime.clone(), vec![commit.clone()]);
        self.notify_committed_mutation_observers(runtime.as_ref(), commit);
    }

    pub(in crate::engine) fn process_applied_commit_batch(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
        emit_trigger_candidates: bool,
    ) {
        if applied.is_empty() {
            return;
        }

        let batch_candidate_documents = applied
            .iter()
            .map(candidate_documents_for_commit)
            .collect::<Vec<_>>();
        let batch_candidates = applied
            .iter()
            .zip(batch_candidate_documents.iter())
            .map(|(commit, candidate_documents)| SubscriptionBatchCandidate {
                commit,
                candidate_documents,
            })
            .collect::<Vec<_>>();
        let affected = runtime
            .subscriptions
            .affected_subscription_ids_for_batch(&batch_candidates);
        if !affected.subscription_ids.is_empty() {
            if applied.len() > 1 {
                runtime
                    .subscription_delivery_metrics()
                    .record_coalesced_batch(applied.len() as u64, affected.merged_wakeup_count);
            }

            let latest = applied
                .last()
                .expect("non-empty applied batch should have a latest commit");
            let commit_identity = document_bearing_commit_identity(applied);
            let work = QueuedSubscriptionWork::new_coalesced(
                affected.subscription_ids,
                latest.sequence,
                // Coalesced batches intentionally omit per-commit identity unless
                // exactly one commit in the batch actually carries writes. A
                // zero-write commit (e.g. the trigger-candidate feed's own
                // delivery-cursor advance) shares this same commit log and
                // sequence space with real document commits, so it can land in
                // `applied` alongside a single real commit; counting it toward
                // "more than one commit" would erase that lone real commit's
                // identity even though nothing else in the batch could ever be
                // the subject of a subscription update.
                commit_identity,
                merge_deleted_documents_for_batch(applied),
            );
            self.dispatch_or_enqueue_subscription_work(runtime.clone(), work);
        }

        if emit_trigger_candidates {
            self.dispatch_or_enqueue_trigger_candidates(runtime.clone(), applied.to_vec());
        }
        for commit in applied {
            self.notify_committed_mutation_observers(runtime.as_ref(), commit);
        }
    }
}
