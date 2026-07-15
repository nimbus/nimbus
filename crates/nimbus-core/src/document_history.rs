use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    CommitSequence, Document, DocumentId, Error, HistoricalReadErrorKind, HistoricalReadShape,
    Result, SequenceNumber, TableId, TenantEventKind, TenantEventRecord, Timestamp,
};

/// Versioned document row visible through a stable table identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentVersion {
    table_id: TableId,
    document_id: DocumentId,
    commit_sequence: CommitSequence,
    commit_timestamp: Timestamp,
    document: Option<Document>,
}

impl DocumentVersion {
    pub fn table_id(&self) -> &TableId {
        &self.table_id
    }

    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    pub fn commit_sequence(&self) -> CommitSequence {
        self.commit_sequence
    }

    pub fn commit_timestamp(&self) -> Timestamp {
        self.commit_timestamp
    }

    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    pub fn is_tombstone(&self) -> bool {
        self.document.is_none()
    }
}

/// Canonical event-derived document-version model for historical point reads.
#[derive(Debug, Clone)]
pub struct DocumentVersionHistory {
    versions: BTreeMap<(TableId, DocumentId, CommitSequence), DocumentVersion>,
    storage_format_generation: u16,
}

impl DocumentVersionHistory {
    pub fn from_records(records: impl IntoIterator<Item = TenantEventRecord>) -> Result<Self> {
        Self::from_records_with_format_generation(records, 1)
    }

    pub fn from_records_with_format_generation(
        records: impl IntoIterator<Item = TenantEventRecord>,
        storage_format_generation: u16,
    ) -> Result<Self> {
        if storage_format_generation == 0 {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                "document history storage format generation cannot be zero",
            ));
        }

        let mut records = records.into_iter().collect::<Vec<_>>();
        records.sort_by_key(|record| record.sequence);
        let mut versions = BTreeMap::new();
        let mut previous_sequence = None;
        for record in &records {
            record.validate_integrity()?;
            if previous_sequence == Some(record.sequence) {
                return Err(Error::conflict(format!(
                    "duplicate tenant event sequence {} in document history",
                    record.sequence
                )));
            }
            previous_sequence = Some(record.sequence);
            for event in record.events() {
                if let TenantEventKind::DocumentWrite { writes } = event {
                    for write in writes {
                        let document_id = write.doc_id.clone();
                        let version = DocumentVersion {
                            table_id: write.table_id.clone(),
                            document_id: document_id.clone(),
                            commit_sequence: CommitSequence::new(record.sequence),
                            commit_timestamp: record.timestamp,
                            document: write.current.clone(),
                        };
                        versions.insert(
                            (write.table_id.clone(), document_id, version.commit_sequence),
                            version,
                        );
                    }
                }
            }
        }

        Ok(Self {
            versions,
            storage_format_generation,
        })
    }

    pub fn get_at(
        &self,
        read_shape: &HistoricalReadShape,
        document_id: &DocumentId,
    ) -> Result<Option<Document>> {
        let start = (
            read_shape.table_id().clone(),
            document_id.clone(),
            CommitSequence::new(SequenceNumber(0)),
        );
        let end = (
            read_shape.table_id().clone(),
            document_id.clone(),
            read_shape.read_snapshot().sequence(),
        );
        Ok(self
            .versions
            .range(start..=end)
            .next_back()
            .and_then(|(_, version)| version.document.clone()))
    }

    pub fn latest_version(
        &self,
        table_id: &TableId,
        document_id: &DocumentId,
    ) -> Option<&DocumentVersion> {
        let start = (
            table_id.clone(),
            document_id.clone(),
            CommitSequence::new(SequenceNumber(0)),
        );
        let end = (
            table_id.clone(),
            document_id.clone(),
            CommitSequence::new(SequenceNumber(u64::MAX)),
        );
        self.versions
            .range(start..=end)
            .next_back()
            .map(|(_, version)| version)
    }

    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    pub fn storage_format_generation(&self) -> u16 {
        self.storage_format_generation
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use crate::{
        CommitTimestamp, HistoricalReadSnapshot, IndexDefinition, ReadTimestamp, SchemaChangeEvent,
        TableName, TableSchema, Timestamp, VersionedRegistry, WriteOp, WriteOpType,
    };

    use super::*;

    fn table(name: &str) -> TableName {
        TableName::new(name).expect("table name should build")
    }

    fn document(table: &TableName, id: &DocumentId, body: &str) -> Document {
        let mut fields = Map::new();
        fields.insert("body".to_string(), json!(body));
        Document::with_id(id.clone(), table.clone(), fields)
    }

    fn snapshot(sequence: u64) -> HistoricalReadSnapshot {
        HistoricalReadSnapshot::new(
            ReadTimestamp::new(Timestamp(sequence * 100)),
            CommitSequence::new(SequenceNumber(sequence)),
            CommitTimestamp::new(Timestamp(sequence * 100)),
        )
    }

    fn write_record(
        sequence: u64,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        previous: Option<Document>,
        current: Option<Document>,
        op_type: WriteOpType,
    ) -> TenantEventRecord {
        TenantEventRecord::new(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            vec![WriteOp {
                table: table.clone(),
                table_id: table_id.clone(),
                op_type,
                doc_id: document_id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous,
                current,
            }],
            None,
        )
        .expect("document write record should build")
    }

    fn registry_shape(
        table: &TableName,
        table_id: &TableId,
        snapshot: HistoricalReadSnapshot,
    ) -> HistoricalReadShape {
        let registry = VersionedRegistry::from_records([TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: TableSchema {
                    table: table.clone(),
                    fields: Vec::new(),
                    indexes: vec![IndexDefinition::new("by_body", ["body"])],
                    access_policy: None,
                },
            },
        )
        .expect("schema record should build")])
        .expect("registry should build");
        registry
            .read_shape_at(table, snapshot)
            .expect("shape should resolve")
            .expect("table should exist")
    }

    #[test]
    fn point_reads_follow_insert_update_delete_history() {
        let table = table("messages");
        let table_id = TableId::new();
        let document_id = DocumentId::from_key("m1").unwrap();
        let v1 = document(&table, &document_id, "hello");
        let v2 = document(&table, &document_id, "goodbye");
        let history = DocumentVersionHistory::from_records([
            write_record(
                2,
                &table,
                &table_id,
                &document_id,
                None,
                Some(v1.clone()),
                WriteOpType::Insert,
            ),
            write_record(
                3,
                &table,
                &table_id,
                &document_id,
                Some(v1.clone()),
                Some(v2.clone()),
                WriteOpType::Update,
            ),
            write_record(
                4,
                &table,
                &table_id,
                &document_id,
                Some(v2.clone()),
                None,
                WriteOpType::Delete,
            ),
        ])
        .unwrap();

        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &table_id, snapshot(1)),
                    &document_id
                )
                .unwrap(),
            None
        );
        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &table_id, snapshot(2)),
                    &document_id
                )
                .unwrap(),
            Some(v1)
        );
        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &table_id, snapshot(3)),
                    &document_id
                )
                .unwrap(),
            Some(v2)
        );
        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &table_id, snapshot(4)),
                    &document_id
                )
                .unwrap(),
            None
        );
        assert_eq!(history.version_count(), 3);
    }

    #[test]
    fn table_identity_replacement_does_not_leak_old_document_history() {
        let table = table("tasks");
        let document_id = DocumentId::from_key("same-id").unwrap();
        let old_table_id = TableId::new();
        let replacement_table_id = TableId::new();
        let old_doc = document(&table, &document_id, "old");
        let new_doc = document(&table, &document_id, "new");
        let history = DocumentVersionHistory::from_records([
            write_record(
                1,
                &table,
                &old_table_id,
                &document_id,
                None,
                Some(old_doc.clone()),
                WriteOpType::Insert,
            ),
            write_record(
                2,
                &table,
                &replacement_table_id,
                &document_id,
                None,
                Some(new_doc.clone()),
                WriteOpType::Insert,
            ),
        ])
        .unwrap();

        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &old_table_id, snapshot(2)),
                    &document_id
                )
                .unwrap(),
            Some(old_doc)
        );
        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &replacement_table_id, snapshot(2)),
                    &document_id
                )
                .unwrap(),
            Some(new_doc)
        );
    }

    #[test]
    fn latest_version_matches_latest_visible_document() {
        let table = table("latest");
        let table_id = TableId::new();
        let document_id = DocumentId::from_key("doc").unwrap();
        let v1 = document(&table, &document_id, "one");
        let v2 = document(&table, &document_id, "two");
        let history = DocumentVersionHistory::from_records([
            write_record(
                1,
                &table,
                &table_id,
                &document_id,
                None,
                Some(v1),
                WriteOpType::Insert,
            ),
            write_record(
                2,
                &table,
                &table_id,
                &document_id,
                None,
                Some(v2.clone()),
                WriteOpType::Update,
            ),
        ])
        .unwrap();

        assert_eq!(
            history
                .latest_version(&table_id, &document_id)
                .and_then(DocumentVersion::document),
            Some(&v2)
        );
        assert_eq!(
            history
                .get_at(
                    &registry_shape(&table, &table_id, snapshot(2)),
                    &document_id
                )
                .unwrap(),
            Some(v2)
        );
    }

    #[test]
    fn document_history_rejects_duplicate_sequences_and_unknown_format() {
        let table = table("bad");
        let table_id = TableId::new();
        let document_id = DocumentId::from_key("doc").unwrap();
        let first = write_record(
            1,
            &table,
            &table_id,
            &document_id,
            None,
            Some(document(&table, &document_id, "one")),
            WriteOpType::Insert,
        );
        let duplicate =
            TenantEventRecord::barrier(SequenceNumber(1), Timestamp(100), "same".into())
                .expect("barrier should build");
        let duplicate_error = DocumentVersionHistory::from_records([first, duplicate]).unwrap_err();
        let format_error = DocumentVersionHistory::from_records_with_format_generation([], 0)
            .expect_err("format generation zero should fail");

        assert!(
            matches!(duplicate_error, Error::Conflict { message, .. } if message.contains("duplicate"))
        );
        assert_eq!(
            format_error.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
    }
}
