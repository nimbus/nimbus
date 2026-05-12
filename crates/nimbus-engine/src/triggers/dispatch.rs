#![allow(dead_code)]

use std::collections::BTreeSet;

use nimbus_core::{
    CommitEntry, Document, DocumentEventData, DocumentEventDocument, DocumentEventUpdateMask,
    DocumentLocator, FirestoreCloudEventType, ResourcePathBinding, Result, TriggerCommitMetadata,
    TriggerWriteOrigin, WriteOp, WriteOpType,
};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TriggerCommitCandidate {
    pub event_id: String,
    pub event_type: FirestoreCloudEventType,
    pub binding: ResourcePathBinding,
    pub commit: TriggerCommitMetadata,
    pub data: DocumentEventData,
    pub write_origin: Option<TriggerWriteOrigin>,
}

pub(crate) fn build_trigger_commit_candidates<F>(
    commit: &CommitEntry,
    mut resolve_binding: F,
) -> Result<Vec<TriggerCommitCandidate>>
where
    F: FnMut(&DocumentLocator) -> Result<Option<ResourcePathBinding>>,
{
    let mut candidates = Vec::new();
    for (write_index, write) in commit.writes.iter().enumerate() {
        let locator = DocumentLocator::new(write.table.clone(), write.doc_id.clone());
        let binding = match write.resource_path_binding.clone() {
            Some(binding) => Some(binding),
            None => resolve_binding(&locator)?,
        };
        let Some(binding) = binding else {
            continue;
        };

        for event_type in event_types_for_write(write.op_type) {
            candidates.push(TriggerCommitCandidate {
                event_id: trigger_event_id(commit, write_index, event_type),
                event_type,
                binding: binding.clone(),
                commit: TriggerCommitMetadata::new(commit.sequence, commit.timestamp),
                data: candidate_event_data(write, &binding),
                write_origin: write.trigger_write_origin.clone(),
            });
        }
    }
    Ok(candidates)
}

fn event_types_for_write(op_type: WriteOpType) -> [FirestoreCloudEventType; 2] {
    match op_type {
        WriteOpType::Insert => [
            FirestoreCloudEventType::Created,
            FirestoreCloudEventType::Written,
        ],
        WriteOpType::Update => [
            FirestoreCloudEventType::Updated,
            FirestoreCloudEventType::Written,
        ],
        WriteOpType::Delete => [
            FirestoreCloudEventType::Deleted,
            FirestoreCloudEventType::Written,
        ],
    }
}

fn trigger_event_id(
    commit: &CommitEntry,
    write_index: usize,
    event_type: FirestoreCloudEventType,
) -> String {
    format!(
        "commit:{}:write:{}:{}",
        commit.sequence.0,
        write_index,
        match event_type {
            FirestoreCloudEventType::Created => "created",
            FirestoreCloudEventType::Updated => "updated",
            FirestoreCloudEventType::Deleted => "deleted",
            FirestoreCloudEventType::Written => "written",
        }
    )
}

fn candidate_event_data(write: &WriteOp, binding: &ResourcePathBinding) -> DocumentEventData {
    let value = write
        .current
        .clone()
        .map(|document| DocumentEventDocument::new(binding.document_path.clone(), document));
    let old_value = write
        .previous
        .clone()
        .map(|document| DocumentEventDocument::new(binding.document_path.clone(), document));
    let update_mask = match (&write.previous, &write.current, write.op_type) {
        (Some(previous), Some(current), WriteOpType::Update) => {
            let field_paths = changed_field_paths(previous, current);
            (!field_paths.is_empty()).then(|| DocumentEventUpdateMask::new(field_paths))
        }
        _ => None,
    };
    DocumentEventData::new(value, old_value, update_mask)
}

fn changed_field_paths(previous: &Document, current: &Document) -> Vec<String> {
    let mut paths = BTreeSet::new();
    let keys = previous
        .fields
        .keys()
        .chain(previous.typed_fields.keys())
        .chain(current.fields.keys())
        .chain(current.typed_fields.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let previous_typed = previous.typed_fields.get(&key);
        let current_typed = current.typed_fields.get(&key);
        let previous_value = previous.fields.get(&key);
        let current_value = current.fields.get(&key);
        if previous_typed == current_typed && previous_value == current_value {
            continue;
        }
        match (previous_value, current_value, previous_typed, current_typed) {
            (
                Some(Value::Object(previous_object)),
                Some(Value::Object(current_object)),
                None,
                None,
            ) => {
                collect_object_diff_paths(&key, previous_object, current_object, &mut paths);
            }
            _ => {
                paths.insert(key);
            }
        }
    }
    paths.into_iter().collect()
}

fn collect_object_diff_paths(
    prefix: &str,
    previous: &Map<String, Value>,
    current: &Map<String, Value>,
    paths: &mut BTreeSet<String>,
) {
    let keys = previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let path = format!("{prefix}.{key}");
        match (previous.get(&key), current.get(&key)) {
            (Some(Value::Object(previous_child)), Some(Value::Object(current_child))) => {
                collect_object_diff_paths(&path, previous_child, current_child, paths);
            }
            (left, right) if left == right => {}
            _ => {
                paths.insert(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{DocumentId, DocumentPath, SequenceNumber, TableName, Timestamp};
    use serde_json::json;

    use super::*;

    fn sample_binding() -> ResourcePathBinding {
        ResourcePathBinding::new(
            DocumentLocator::new(
                TableName::new("cities_store").expect("table should parse"),
                DocumentId::from_key("SF").expect("id should parse"),
            ),
            DocumentPath::from_segments(["cities", "SF"]).expect("path should parse"),
        )
    }

    fn sample_document(name: &str, population: i64) -> Document {
        Document::with_id(
            DocumentId::from_key("SF").expect("id should parse"),
            TableName::new("cities_store").expect("table should parse"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!(name)),
                ("population".to_string(), json!(population)),
            ]),
        )
    }

    #[test]
    fn insert_candidates_emit_created_and_written_with_deterministic_ids() {
        let binding = sample_binding();
        let commit = CommitEntry {
            sequence: SequenceNumber(7),
            timestamp: Timestamp(42),
            writes: vec![WriteOp {
                table: binding.locator.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: binding.locator.id.clone(),
                resource_path_binding: Some(binding.clone()),
                trigger_write_origin: None,
                previous: None,
                current: Some(sample_document("San Francisco", 10)),
            }],
        };

        let candidates = build_trigger_commit_candidates(&commit, |_| Ok(None))
            .expect("candidate build should succeed");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].event_id, "commit:7:write:0:created");
        assert_eq!(candidates[0].event_type, FirestoreCloudEventType::Created);
        assert_eq!(candidates[1].event_id, "commit:7:write:0:written");
        assert_eq!(candidates[1].binding.document_path, binding.document_path);
    }

    #[test]
    fn update_candidates_fall_back_to_locator_lookup_and_capture_nested_mask() {
        let binding = sample_binding();
        let mut previous = sample_document("San Francisco", 10);
        previous.fields.insert(
            "profile".to_string(),
            json!({ "nickname": "sf", "rank": 1 }),
        );
        let mut current = previous.clone();
        current.fields.insert(
            "profile".to_string(),
            json!({ "nickname": "bay", "rank": 1 }),
        );
        let commit = CommitEntry {
            sequence: SequenceNumber(8),
            timestamp: Timestamp(43),
            writes: vec![WriteOp {
                table: binding.locator.table.clone(),
                op_type: WriteOpType::Update,
                doc_id: binding.locator.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(previous),
                current: Some(current),
            }],
        };

        let candidates = build_trigger_commit_candidates(&commit, |locator| {
            assert_eq!(locator, &binding.locator);
            Ok(Some(binding.clone()))
        })
        .expect("candidate build should succeed");

        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0].data.update_mask,
            Some(DocumentEventUpdateMask::new(vec![
                "profile.nickname".to_string()
            ]))
        );
    }

    #[test]
    fn delete_candidates_preserve_bound_path_identity_from_commit_record() {
        let binding = sample_binding();
        let commit = CommitEntry {
            sequence: SequenceNumber(9),
            timestamp: Timestamp(44),
            writes: vec![WriteOp {
                table: binding.locator.table.clone(),
                op_type: WriteOpType::Delete,
                doc_id: binding.locator.id.clone(),
                resource_path_binding: Some(binding.clone()),
                trigger_write_origin: None,
                previous: Some(sample_document("San Francisco", 10)),
                current: None,
            }],
        };

        let candidates = build_trigger_commit_candidates(&commit, |_| {
            panic!("delete candidate should not need live path lookup")
        })
        .expect("candidate build should succeed");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].event_type, FirestoreCloudEventType::Deleted);
        assert!(candidates[0].data.value.is_none());
        assert_eq!(
            candidates[0]
                .data
                .old_value
                .as_ref()
                .expect("delete should keep old value")
                .path,
            binding.document_path
        );
    }
}
