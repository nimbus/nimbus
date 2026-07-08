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
    let first = batch_iter
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

    for work in batch_iter {
        merged_count = merged_count.saturating_add(1);
        delivery_sequence = delivery_sequence.max(work.delivery_sequence);
        earliest_enqueued_at = earliest_enqueued_at.min(work.enqueued_at);
        merged_subscription_ids.extend(work.subscription_ids);
        for document in work.deleted_documents {
            deleted_documents.insert(document.id.clone(), document);
        }
    }

    // Commit identity survives a merge ONLY when nothing merged. At this
    // layer a `commit: None` work item is irrecoverably ambiguous: it is not
    // "zero-write" (zero-write commits never enqueue subscription work at
    // all) but "identity unknown" -- a multi-document-commit coalesced batch
    // or a bootstrap catch-up, either of which hides document-bearing
    // commits. Preserving a lone `Some` across such a merge would forward a
    // PARTIAL commit hint, and downstream runtime-backed subscription
    // transforms use that hint to skip re-evaluation when it does not
    // intersect their read set -- a missed client update. The
    // zero-write-rides-along case is handled where actual `writes` are
    // inspectable: `document_bearing_commit_identity` at the applied-batch
    // site (engine/mutations/commit_processing.rs).
    let commit = (merged_count == 0).then_some(first.commit).flatten();
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

#[cfg(test)]
mod tests {
    use nimbus_core::{Timestamp, WriteOp, WriteOpType};

    use super::*;

    fn document_commit(sequence: u64) -> CommitEntry {
        CommitEntry {
            sequence: SequenceNumber(sequence),
            timestamp: Timestamp(sequence * 10),
            writes: vec![WriteOp {
                table: nimbus_core::TableName::new("tasks").expect("table name should be valid"),
                table_id: nimbus_core::TableId::new(),
                op_type: WriteOpType::Insert,
                doc_id: nimbus_core::DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: None,
            }],
        }
    }

    #[test]
    fn merge_preserves_identity_only_when_nothing_merged() {
        let commit = document_commit(7);
        let (merged, merged_count) =
            merge_queued_subscription_work(vec![QueuedSubscriptionWork::new_single(
                vec![1],
                commit.clone(),
                Vec::new(),
            )]);
        assert_eq!(merged_count, 0);
        assert_eq!(
            merged.commit.as_ref().map(|c| c.sequence),
            Some(commit.sequence),
            "a lone work item must keep its commit identity"
        );
    }

    /// Regression for the review-confirmed P1: a commit-less work item is
    /// "identity unknown" (multi-document-commit coalesced batch, bootstrap
    /// catch-up), NOT "zero-write" -- it can hide document-bearing commits.
    /// Preserving a lone `Some` across such a merge forwards a PARTIAL
    /// commit hint, and runtime-backed subscription transforms skip
    /// re-evaluation when that hint does not intersect their read set
    /// (forwarding.rs `Ok(None) => continue`) -- a missed client update.
    #[test]
    fn merge_with_identity_unknown_work_drops_the_commit_hint() {
        let (merged, merged_count) = merge_queued_subscription_work(vec![
            QueuedSubscriptionWork::new_single(vec![1], document_commit(7), Vec::new()),
            QueuedSubscriptionWork::new_coalesced(
                vec![1],
                SequenceNumber(9),
                // Identity-unknown: e.g. a coalesced batch that hid several
                // document-bearing commits behind a dropped identity.
                None,
                Vec::new(),
            ),
        ]);
        assert_eq!(merged_count, 1);
        assert_eq!(
            merged.commit, None,
            "merging identity-unknown work must drop the commit hint"
        );
        assert_eq!(merged.delivery_sequence, SequenceNumber(9));
    }

    #[test]
    fn merge_of_two_identified_commits_drops_the_commit_hint() {
        let (merged, merged_count) = merge_queued_subscription_work(vec![
            QueuedSubscriptionWork::new_single(vec![1], document_commit(7), Vec::new()),
            QueuedSubscriptionWork::new_single(vec![2], document_commit(8), Vec::new()),
        ]);
        assert_eq!(merged_count, 1);
        assert_eq!(
            merged.commit, None,
            "two distinct document commits cannot share one identity"
        );
    }
}
