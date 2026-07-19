use std::collections::HashSet;
use std::sync::Arc;

use nimbus_core::{
    CommitEntry, Document, DocumentId, SequenceNumber, TableName, TenantEventRecord,
};

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

/// Returns the identity of a batch's sole document-bearing commit, if exactly
/// one exists AND every other record in the batch is provably inert. Kinds
/// live only on the unflattened `TenantEventRecord` -- a `CommitEntry` has
/// already erased them down to `writes`, which is why this takes records and
/// must run before that flattening. The provider catch-up path is the only
/// production caller that needs this: it re-reads a raw journal tail that can
/// span more than one originating operation, so its batch can legitimately
/// mix a document write with a zero-write record of ANY kind. A zero-write
/// record is inert only when it carries event detail and every event is
/// `TriggerDelivery` -- the trigger-candidate feed's own delivery-cursor
/// advance, which by construction changes no documents, schema, or policy.
/// Any other zero-write kind (`SchemaChange`, `TableLifecycle`, ...) forces
/// `None`: an access-policy or table-lifecycle change riding along with a
/// document write in the same batch is exactly the case a preserved hint
/// would let a runtime-backed subscription transform skip re-evaluating.
pub(in crate::engine) fn document_bearing_commit_identity(
    records: &[TenantEventRecord],
) -> Option<CommitEntry> {
    let mut document_bearing = records.iter().filter(|record| !record.writes.is_empty());
    let (Some(only), None) = (document_bearing.next(), document_bearing.next()) else {
        return None;
    };
    let other_records_are_provably_inert = records
        .iter()
        .filter(|record| record.writes.is_empty())
        .all(TenantEventRecord::is_provably_inert_trigger_delivery_only);
    other_records_are_provably_inert.then(|| only.as_commit_entry())
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

    /// Enqueues subscription and trigger work without invoking extension
    /// callbacks. The committer uses this as its ordered publication boundary;
    /// observer callbacks remain outside it so they may safely write again.
    pub(crate) fn process_commit_fanout(&self, runtime: Arc<TenantRuntime>, commit: &CommitEntry) {
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
    }

    pub(in crate::engine) fn process_applied_commit_batch_fanout(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
        commit_identity: Option<CommitEntry>,
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
            let work = QueuedSubscriptionWork::new_coalesced(
                affected.subscription_ids,
                latest.sequence,
                // Coalesced batches intentionally omit per-commit identity
                // unless the caller can prove exactly one commit in the
                // batch actually carries writes and everything else is
                // inert. `CommitEntry` alone cannot prove that (it has
                // already lost event-kind information), so identity is
                // supplied by the caller: the live mutation-queue apply path
                // (batches are always real document commits, so `len() == 1`
                // is exact) or the provider catch-up path (kind-aware over
                // the still-unflattened `TenantEventRecord`s, via
                // `document_bearing_commit_identity`).
                commit_identity,
                merge_deleted_documents_for_batch(applied),
            );
            self.dispatch_or_enqueue_subscription_work(runtime.clone(), work);
        }

        if emit_trigger_candidates {
            self.dispatch_or_enqueue_trigger_candidates(runtime.clone(), applied.to_vec());
        }
    }
}
