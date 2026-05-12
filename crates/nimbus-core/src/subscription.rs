use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CommitEntry, Document, SequenceNumber, Timestamp};

/// Stable commit metadata that subscriptions can surface without leaking the
/// full storage commit payload into adapter contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionCommitMetadata {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
}

impl From<&CommitEntry> for SubscriptionCommitMetadata {
    fn from(commit: &CommitEntry) -> Self {
        Self {
            sequence: commit.sequence,
            timestamp: commit.timestamp,
        }
    }
}

/// Protocol-neutral snapshot state emitted for a subscription result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionResultSnapshot {
    pub covered_sequence: SequenceNumber,
    #[serde(default)]
    pub documents: Vec<Document>,
    #[serde(default)]
    pub deleted_documents: Vec<Document>,
    #[serde(default)]
    pub commit: Option<SubscriptionCommitMetadata>,
}

impl SubscriptionResultSnapshot {
    /// Creates the initial snapshot delivered during subscription bootstrap.
    pub fn bootstrap(covered_sequence: SequenceNumber, documents: Vec<Document>) -> Self {
        Self {
            covered_sequence,
            documents,
            deleted_documents: Vec::new(),
            commit: None,
        }
    }

    /// Creates a snapshot produced by reevaluating a subscription at the given
    /// sequence. Exact commit identity is optional because coalesced deliveries
    /// intentionally preserve only sequence/time metadata.
    pub fn from_delivery(
        covered_sequence: SequenceNumber,
        commit: Option<&CommitEntry>,
        documents: Vec<Document>,
        deleted_documents: Vec<Document>,
    ) -> Self {
        Self {
            covered_sequence,
            documents,
            deleted_documents,
            commit: commit.map(SubscriptionCommitMetadata::from),
        }
    }

    /// Converts the current result documents into the external JSON payload.
    pub fn to_json_documents(&self) -> Vec<Value> {
        self.documents.iter().map(Document::to_json).collect()
    }

    /// Converts the current result documents into the external JSON payload by
    /// consuming the snapshot.
    pub fn into_json_documents(self) -> Vec<Value> {
        self.documents
            .into_iter()
            .map(Document::into_json)
            .collect()
    }
}

/// The classification for one document-level change between successive
/// subscription snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionDocumentChangeKind {
    Added,
    Modified,
    Removed,
}

/// One document-level change between successive subscription snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionDocumentChange {
    pub kind: SubscriptionDocumentChangeKind,
    #[serde(default)]
    pub previous: Option<Document>,
    #[serde(default)]
    pub current: Option<Document>,
    #[serde(default)]
    pub old_index: Option<usize>,
    #[serde(default)]
    pub new_index: Option<usize>,
}

impl SubscriptionDocumentChange {
    /// Returns true when the document stayed present in both snapshots but
    /// moved to a different position.
    pub fn order_changed(&self) -> bool {
        matches!((self.old_index, self.new_index), (Some(old), Some(new)) if old != new)
    }

    /// Returns true when the document payload changed between snapshots.
    pub fn content_changed(&self) -> bool {
        matches!(
            (&self.previous, &self.current),
            (Some(previous), Some(current)) if previous != current
        )
    }

    /// Returns the best representative document for this change.
    pub fn document(&self) -> Option<&Document> {
        self.current.as_ref().or(self.previous.as_ref())
    }
}

/// Deterministic change set between two successive subscription snapshots.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SubscriptionSnapshotDiff {
    #[serde(default)]
    pub changes: Vec<SubscriptionDocumentChange>,
}

impl SubscriptionSnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Computes document-level changes between successive subscription snapshots.
///
/// The diff is deterministic: removals are emitted in prior-snapshot order,
/// then additions or modifications are emitted in current-snapshot order.
pub fn diff_subscription_snapshots(
    previous: Option<&SubscriptionResultSnapshot>,
    current: &SubscriptionResultSnapshot,
) -> SubscriptionSnapshotDiff {
    let Some(previous) = previous else {
        return SubscriptionSnapshotDiff {
            changes: current
                .documents
                .iter()
                .enumerate()
                .map(|(new_index, document)| SubscriptionDocumentChange {
                    kind: SubscriptionDocumentChangeKind::Added,
                    previous: None,
                    current: Some(document.clone()),
                    old_index: None,
                    new_index: Some(new_index),
                })
                .collect(),
        };
    };

    let previous_positions = previous
        .documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            (
                (document.table.clone(), document.id.clone()),
                (index, document),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let current_positions = current
        .documents
        .iter()
        .enumerate()
        .map(|(index, document)| {
            (
                (document.table.clone(), document.id.clone()),
                (index, document),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();

    for (old_index, document) in previous.documents.iter().enumerate() {
        let key = (document.table.clone(), document.id.clone());
        if !current_positions.contains_key(&key) {
            changes.push(SubscriptionDocumentChange {
                kind: SubscriptionDocumentChangeKind::Removed,
                previous: Some(document.clone()),
                current: None,
                old_index: Some(old_index),
                new_index: None,
            });
        }
    }

    for (new_index, document) in current.documents.iter().enumerate() {
        let key = (document.table.clone(), document.id.clone());
        match previous_positions.get(&key) {
            None => changes.push(SubscriptionDocumentChange {
                kind: SubscriptionDocumentChangeKind::Added,
                previous: None,
                current: Some(document.clone()),
                old_index: None,
                new_index: Some(new_index),
            }),
            Some((old_index, previous_document))
                if *old_index != new_index || *previous_document != document =>
            {
                changes.push(SubscriptionDocumentChange {
                    kind: SubscriptionDocumentChangeKind::Modified,
                    previous: Some((*previous_document).clone()),
                    current: Some(document.clone()),
                    old_index: Some(*old_index),
                    new_index: Some(new_index),
                });
            }
            Some(_) => {}
        }
    }

    SubscriptionSnapshotDiff { changes }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{DocumentId, TableName};

    use super::*;

    #[test]
    fn bootstrap_snapshot_preserves_sequence_without_commit_metadata() {
        let document = Document::with_id(
            DocumentId::from_key("東京".to_string()).expect("unicode document id should parse"),
            TableName::new("tasks").expect("table should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        );

        let snapshot = SubscriptionResultSnapshot::bootstrap(SequenceNumber(7), vec![document]);

        assert_eq!(snapshot.covered_sequence, SequenceNumber(7));
        assert!(snapshot.commit.is_none());
        assert!(snapshot.deleted_documents.is_empty());
        assert_eq!(snapshot.to_json_documents()[0]["title"], json!("Hello"));
    }

    #[test]
    fn delivery_snapshot_maps_commit_to_stable_metadata() {
        let table = TableName::new("tasks").expect("table should be valid");
        let document = Document::with_id(
            DocumentId::from_key("cities-SF".to_string()).expect("fixture id should parse"),
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("City"))]),
        );
        let deleted = Document::with_id(
            DocumentId::from_key("東京".to_string()).expect("unicode document id should parse"),
            table,
            serde_json::Map::from_iter([("title".to_string(), json!("Removed"))]),
        );
        let commit = CommitEntry {
            sequence: SequenceNumber(11),
            timestamp: Timestamp(111),
            writes: Vec::new(),
        };

        let snapshot = SubscriptionResultSnapshot::from_delivery(
            SequenceNumber(12),
            Some(&commit),
            vec![document],
            vec![deleted.clone()],
        );

        assert_eq!(snapshot.covered_sequence, SequenceNumber(12));
        assert_eq!(
            snapshot.commit,
            Some(SubscriptionCommitMetadata {
                sequence: SequenceNumber(11),
                timestamp: Timestamp(111),
            })
        );
        assert_eq!(snapshot.deleted_documents, vec![deleted]);
    }

    #[test]
    fn diff_from_empty_marks_current_documents_added_in_order() {
        let table = TableName::new("tasks").expect("table should be valid");
        let current = SubscriptionResultSnapshot::bootstrap(
            SequenceNumber(3),
            vec![
                Document::with_id(
                    DocumentId::from_key("a".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
                ),
                Document::with_id(
                    DocumentId::from_key("b".to_string()).expect("id should parse"),
                    table,
                    serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
                ),
            ],
        );

        let diff = diff_subscription_snapshots(None, &current);

        assert_eq!(diff.changes.len(), 2);
        assert_eq!(
            diff.changes
                .iter()
                .map(|change| change.kind)
                .collect::<Vec<_>>(),
            vec![
                SubscriptionDocumentChangeKind::Added,
                SubscriptionDocumentChangeKind::Added,
            ]
        );
        assert_eq!(
            diff.changes
                .iter()
                .map(|change| change.new_index)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1)]
        );
    }

    #[test]
    fn diff_to_empty_marks_previous_documents_removed_in_order() {
        let table = TableName::new("tasks").expect("table should be valid");
        let previous = SubscriptionResultSnapshot::bootstrap(
            SequenceNumber(4),
            vec![
                Document::with_id(
                    DocumentId::from_key("a".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
                ),
                Document::with_id(
                    DocumentId::from_key("b".to_string()).expect("id should parse"),
                    table,
                    serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
                ),
            ],
        );
        let current = SubscriptionResultSnapshot::bootstrap(SequenceNumber(5), Vec::new());

        let diff = diff_subscription_snapshots(Some(&previous), &current);

        assert_eq!(diff.changes.len(), 2);
        assert_eq!(
            diff.changes
                .iter()
                .map(|change| change.kind)
                .collect::<Vec<_>>(),
            vec![
                SubscriptionDocumentChangeKind::Removed,
                SubscriptionDocumentChangeKind::Removed,
            ]
        );
        assert_eq!(
            diff.changes
                .iter()
                .map(|change| change.old_index)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1)]
        );
    }

    #[test]
    fn diff_classifies_added_modified_removed_and_ordering_changes() {
        let table = TableName::new("tasks").expect("table should be valid");
        let previous = SubscriptionResultSnapshot::bootstrap(
            SequenceNumber(6),
            vec![
                Document::with_id(
                    DocumentId::from_key("a".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
                ),
                Document::with_id(
                    DocumentId::from_key("b".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
                ),
            ],
        );
        let current = SubscriptionResultSnapshot::bootstrap(
            SequenceNumber(7),
            vec![
                Document::with_id(
                    DocumentId::from_key("b".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("B2"))]),
                ),
                Document::with_id(
                    DocumentId::from_key("c".to_string()).expect("id should parse"),
                    table,
                    serde_json::Map::from_iter([("title".to_string(), json!("C"))]),
                ),
            ],
        );

        let diff = diff_subscription_snapshots(Some(&previous), &current);

        assert_eq!(diff.changes.len(), 3);
        assert_eq!(
            diff.changes[0].kind,
            SubscriptionDocumentChangeKind::Removed
        );
        assert_eq!(diff.changes[0].old_index, Some(0));
        assert_eq!(
            diff.changes[0]
                .document()
                .expect("removed change should retain a document")
                .get_field("title"),
            Some(&json!("A"))
        );
        assert_eq!(
            diff.changes[1].kind,
            SubscriptionDocumentChangeKind::Modified
        );
        assert!(diff.changes[1].order_changed());
        assert!(diff.changes[1].content_changed());
        assert_eq!(diff.changes[1].old_index, Some(1));
        assert_eq!(diff.changes[1].new_index, Some(0));
        assert_eq!(diff.changes[2].kind, SubscriptionDocumentChangeKind::Added);
        assert_eq!(diff.changes[2].new_index, Some(1));
    }

    #[test]
    fn diff_is_stable_across_consecutive_identical_snapshots() {
        let table = TableName::new("tasks").expect("table should be valid");
        let snapshot = SubscriptionResultSnapshot::bootstrap(
            SequenceNumber(8),
            vec![
                Document::with_id(
                    DocumentId::from_key("a".to_string()).expect("id should parse"),
                    table.clone(),
                    serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
                ),
                Document::with_id(
                    DocumentId::from_key("b".to_string()).expect("id should parse"),
                    table,
                    serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
                ),
            ],
        );

        let first = diff_subscription_snapshots(None, &snapshot);
        let second = diff_subscription_snapshots(Some(&snapshot), &snapshot);

        assert_eq!(first.changes.len(), 2);
        assert!(second.is_empty());
    }
}
