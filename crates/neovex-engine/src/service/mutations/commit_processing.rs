use std::collections::HashSet;
use std::sync::Arc;

use neovex_core::{CommitEntry, Document, DocumentId, TableName};

use crate::subscriptions::{
    QueuedSubscriptionWork, SubscriptionBatchCandidate, dispatch_subscription_work,
};
use crate::{Service, tenant::TenantRuntime};

pub(super) fn candidate_documents_for_commit(commit: &CommitEntry) -> Vec<Document> {
    commit
        .writes
        .iter()
        .filter_map(|write| match write.op_type {
            neovex_core::WriteOpType::Insert => write.current.clone(),
            neovex_core::WriteOpType::Update => None,
            neovex_core::WriteOpType::Delete => write.previous.clone(),
        })
        .collect()
}

fn deleted_documents_for_commit(commit: &CommitEntry) -> Vec<Document> {
    commit
        .writes
        .iter()
        .filter(|write| matches!(write.op_type, neovex_core::WriteOpType::Delete))
        .filter_map(|write| write.previous.clone())
        .collect()
}

fn merge_deleted_documents_for_batch(applied: &[CommitEntry]) -> Vec<Document> {
    let mut seen = HashSet::<(TableName, DocumentId)>::new();
    let mut deleted_documents = Vec::new();
    for commit in applied {
        for document in commit
            .writes
            .iter()
            .filter(|write| matches!(write.op_type, neovex_core::WriteOpType::Delete))
            .filter_map(|write| write.previous.as_ref())
        {
            let key = (document.table.clone(), document.id);
            if seen.insert(key) {
                deleted_documents.push(document.clone());
            }
        }
    }
    deleted_documents
}

impl Service {
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

        runtime.record_subscription_overflow_sync_fallback();
        let stats = dispatch_subscription_work(&runtime, &work);
        runtime.record_subscription_dispatch_stats(stats);
    }

    pub(crate) fn process_commit(&self, runtime: Arc<TenantRuntime>, commit: &CommitEntry) {
        let candidate_documents = candidate_documents_for_commit(commit);
        let subscription_ids = runtime
            .subscriptions
            .affected_subscription_ids(commit, &candidate_documents);
        if subscription_ids.is_empty() {
            return;
        }

        let work = QueuedSubscriptionWork::new_single(
            subscription_ids,
            commit.clone(),
            deleted_documents_for_commit(commit),
        );
        self.dispatch_or_enqueue_subscription_work(runtime, work);
    }

    pub(in crate::service) fn process_applied_commit_batch(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
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
        if affected.subscription_ids.is_empty() {
            return;
        }

        if applied.len() > 1 {
            runtime.record_subscription_coalesced_batch(
                applied.len() as u64,
                affected.merged_wakeup_count,
            );
        }

        let latest = applied
            .last()
            .expect("non-empty applied batch should have a latest commit");
        let work = QueuedSubscriptionWork::new_coalesced(
            affected.subscription_ids,
            latest.sequence,
            // Coalesced batches intentionally omit per-commit identity; only a
            // single applied commit can safely preserve exact commit metadata
            // for downstream consumers.
            (applied.len() == 1).then(|| latest.clone()),
            merge_deleted_documents_for_batch(applied),
        );
        self.dispatch_or_enqueue_subscription_work(runtime, work);
    }
}
