use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use nimbus_core::{CommitEntry, Document, SequenceNumber};

#[derive(Debug, Clone)]
pub(crate) struct QueuedSubscriptionWork {
    pub subscription_ids: Vec<u64>,
    pub delivery_sequence: SequenceNumber,
    pub commit: Option<CommitEntry>,
    pub deleted_documents: Vec<Document>,
    pub enqueued_at: Instant,
}

impl QueuedSubscriptionWork {
    pub(crate) fn new_single(
        subscription_ids: Vec<u64>,
        commit: CommitEntry,
        deleted_documents: Vec<Document>,
    ) -> Self {
        Self {
            subscription_ids,
            delivery_sequence: commit.sequence,
            commit: Some(commit),
            deleted_documents,
            enqueued_at: Instant::now(),
        }
    }

    pub(crate) fn new_coalesced(
        subscription_ids: Vec<u64>,
        delivery_sequence: SequenceNumber,
        commit: Option<CommitEntry>,
        deleted_documents: Vec<Document>,
    ) -> Self {
        Self {
            subscription_ids,
            delivery_sequence,
            commit,
            deleted_documents,
            enqueued_at: Instant::now(),
        }
    }
}

pub(crate) fn merge_queued_subscription_work(
    batch: Vec<QueuedSubscriptionWork>,
) -> (QueuedSubscriptionWork, u64) {
    let mut batch_iter = batch.into_iter();
    let mut first = batch_iter
        .next()
        .expect("queued subscription merge requires at least one work item");
    let mut merged_subscription_ids = first
        .subscription_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut delivery_sequence = first.delivery_sequence;
    let mut deleted_documents = first
        .deleted_documents
        .into_iter()
        .map(|document| (document.id.clone(), document))
        .collect::<BTreeMap<_, _>>();
    let mut earliest_enqueued_at = first.enqueued_at;
    let mut merged_count = 0_u64;
    // Track how many merged work items actually carry commit identity,
    // separately from `merged_count` (total items merged). A `Some` commit
    // here is already guaranteed document-bearing by construction --
    // `new_single` only ever wraps a real commit, and the coalesced-batch
    // site only preserves `Some` when exactly one document-bearing commit
    // was in that batch -- so it is safe to preserve identity whenever
    // exactly one merged item carries it, regardless of how many total
    // items (including commit-less, e.g. multi-commit-coalesced) merged.
    let mut document_bearing_commit = first.commit.take();
    let mut document_bearing_commit_count = u64::from(document_bearing_commit.is_some());

    for work in batch_iter {
        merged_count = merged_count.saturating_add(1);
        delivery_sequence = delivery_sequence.max(work.delivery_sequence);
        earliest_enqueued_at = earliest_enqueued_at.min(work.enqueued_at);
        merged_subscription_ids.extend(work.subscription_ids);
        if let Some(commit) = work.commit {
            document_bearing_commit_count += 1;
            document_bearing_commit = Some(commit);
        }
        for document in work.deleted_documents {
            deleted_documents.insert(document.id.clone(), document);
        }
    }

    let commit = (document_bearing_commit_count == 1)
        .then_some(document_bearing_commit)
        .flatten();
    (
        QueuedSubscriptionWork {
            subscription_ids: merged_subscription_ids.into_iter().collect(),
            delivery_sequence,
            commit,
            deleted_documents: deleted_documents.into_values().collect(),
            enqueued_at: earliest_enqueued_at,
        },
        merged_count,
    )
}
