use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::{
    Document, DocumentId, Error, Result, SequenceNumber, TableId, TableName, TableState,
    TenantEventRecord, WriteOp, WriteOpType,
};

use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};
use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry};

const SHADOW_MATERIALIZER_MANIFEST_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowMaterializerConfig {
    pub compaction_threshold_records: usize,
}

impl ShadowMaterializerConfig {
    pub fn validate(self) -> Result<Self> {
        if self.compaction_threshold_records == 0 {
            return Err(Error::InvalidInput(
                "shadow materializer compaction threshold must be greater than zero".to_string(),
            ));
        }
        Ok(self)
    }
}

impl Default for ShadowMaterializerConfig {
    fn default() -> Self {
        Self {
            compaction_threshold_records: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowMaterializerManifest {
    pub version: u16,
    pub checkpoint_sequence: SequenceNumber,
    pub current_sequence: SequenceNumber,
    pub pending_record_count: usize,
    pub compaction_runs: u64,
    pub compaction_threshold_records: usize,
}

impl ShadowMaterializerManifest {
    fn validate(
        &self,
        checkpoint: &MaterializedJournalSnapshot,
        config: ShadowMaterializerConfig,
    ) -> Result<()> {
        if self.version != SHADOW_MATERIALIZER_MANIFEST_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported shadow materializer manifest version {}",
                self.version
            )));
        }
        if self.compaction_threshold_records != config.compaction_threshold_records {
            return Err(Error::InvalidInput(format!(
                "shadow materializer manifest threshold {} does not match config {}",
                self.compaction_threshold_records, config.compaction_threshold_records
            )));
        }
        if self.checkpoint_sequence != checkpoint.applied_sequence {
            return Err(Error::InvalidInput(format!(
                "shadow materializer manifest checkpoint sequence {} does not match snapshot sequence {}",
                self.checkpoint_sequence.0, checkpoint.applied_sequence.0
            )));
        }
        if self.current_sequence.0 < self.checkpoint_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "shadow materializer current sequence {} is behind checkpoint sequence {}",
                self.current_sequence.0, self.checkpoint_sequence.0
            )));
        }

        let pending_record_count = u64::try_from(self.pending_record_count).map_err(|_| {
            Error::InvalidInput(
                "shadow materializer pending record count exceeds supported range".to_string(),
            )
        })?;
        let expected_pending = self.current_sequence.0 - self.checkpoint_sequence.0;
        if pending_record_count != expected_pending {
            return Err(Error::InvalidInput(format!(
                "shadow materializer manifest pending count {} does not match sequence gap {}",
                self.pending_record_count, expected_pending
            )));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ShadowMaterializer {
    config: ShadowMaterializerConfig,
    checkpoint: MaterializedJournalSnapshot,
    manifest: ShadowMaterializerManifest,
    table_identities: BTreeMap<(String, TableName), (TableId, TableState)>,
    documents: BTreeMap<(TableId, DocumentId), Document>,
    scheduled_execution_ids: BTreeSet<String>,
    pending_records: Vec<TenantEventRecord>,
}

impl ShadowMaterializer {
    pub fn from_checkpoint_and_journal(
        checkpoint: MaterializedJournalSnapshot,
        journal_tail: Vec<TenantEventRecord>,
        config: ShadowMaterializerConfig,
    ) -> Result<Self> {
        let config = config.validate()?;
        checkpoint.validate()?;

        let table_identities = checkpoint
            .table_identities
            .iter()
            .map(|identity| {
                (
                    (identity.namespace.clone(), identity.table.clone()),
                    (identity.table_id.clone(), identity.state),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let documents = checkpoint
            .documents
            .iter()
            .cloned()
            .map(|document| {
                let table_id = checkpoint.default_table_id(&document.table)?;
                Ok(((table_id, document.id.clone()), document))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let scheduled_execution_ids = checkpoint
            .scheduled_execution_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut materializer = Self {
            config,
            checkpoint: checkpoint.clone(),
            manifest: ShadowMaterializerManifest {
                version: SHADOW_MATERIALIZER_MANIFEST_VERSION,
                checkpoint_sequence: checkpoint.applied_sequence,
                current_sequence: checkpoint.applied_sequence,
                pending_record_count: 0,
                compaction_runs: 0,
                compaction_threshold_records: config.compaction_threshold_records,
            },
            table_identities,
            documents,
            scheduled_execution_ids,
            pending_records: Vec::new(),
        };
        materializer.apply_records(journal_tail)?;
        materializer.validate_manifest()?;
        Ok(materializer)
    }

    pub fn recover(
        checkpoint: MaterializedJournalSnapshot,
        pending_records: Vec<TenantEventRecord>,
        manifest: ShadowMaterializerManifest,
        config: ShadowMaterializerConfig,
    ) -> Result<Self> {
        let config = config.validate()?;
        checkpoint.validate()?;
        manifest.validate(&checkpoint, config)?;

        let mut recovered = Self::from_checkpoint_and_journal(checkpoint, pending_records, config)?;
        if recovered.manifest.current_sequence != manifest.current_sequence {
            return Err(Error::InvalidInput(
                "shadow materializer manifest current sequence does not match recovered state"
                    .to_string(),
            ));
        }
        recovered.manifest.compaction_runs = recovered
            .manifest
            .compaction_runs
            .max(manifest.compaction_runs);
        recovered.validate_manifest()?;
        Ok(recovered)
    }

    pub fn apply_records(&mut self, records: Vec<TenantEventRecord>) -> Result<()> {
        for record in records {
            record.validate_integrity()?;
            let expected_sequence = self.manifest.current_sequence.0.saturating_add(1);
            if record.sequence.0 != expected_sequence {
                return Err(Error::InvalidInput(format!(
                    "shadow materializer expected sequence {}, got {}",
                    expected_sequence, record.sequence.0
                )));
            }

            self.apply_record(&record)?;
            self.pending_records.push(record.clone());
            self.manifest.current_sequence = record.sequence;
            self.manifest.pending_record_count = self.pending_records.len();
            if self.pending_records.len() >= self.config.compaction_threshold_records {
                self.compact()?;
            }
        }
        self.validate_manifest()?;
        Ok(())
    }

    pub fn checkpoint(&self) -> &MaterializedJournalSnapshot {
        &self.checkpoint
    }

    pub fn current_snapshot(&self) -> MaterializedJournalSnapshot {
        MaterializedJournalSnapshot {
            version: self.checkpoint.version,
            applied_sequence: self.manifest.current_sequence,
            durable_head: self.manifest.current_sequence,
            table_identities: self.current_table_identities(),
            schema: self.checkpoint.schema.clone(),
            documents: self.current_documents(),
            scheduled_execution_ids: self.current_scheduled_execution_ids(),
        }
    }

    pub fn current_documents(&self) -> Vec<Document> {
        let active_table_ids = self
            .table_identities
            .iter()
            .filter(|&((namespace, _), (_, state))| {
                namespace == DEFAULT_TABLE_NAMESPACE && *state == TableState::Active
            })
            .map(|((_, table), (table_id, _))| (table.clone(), table_id.clone()))
            .collect::<BTreeMap<_, _>>();

        self.documents
            .iter()
            .filter_map(|((table_id, _), document)| {
                active_table_ids
                    .get(&document.table)
                    .filter(|active_table_id| *active_table_id == table_id)
                    .map(|_| document.clone())
            })
            .collect()
    }

    pub fn current_table_identities(&self) -> Vec<TableIdentitySnapshotEntry> {
        self.table_identities
            .iter()
            .map(
                |((namespace, table), (table_id, state))| TableIdentitySnapshotEntry {
                    namespace: namespace.clone(),
                    table: table.clone(),
                    table_id: table_id.clone(),
                    state: *state,
                },
            )
            .collect()
    }

    pub fn current_scheduled_execution_ids(&self) -> Vec<String> {
        self.scheduled_execution_ids.iter().cloned().collect()
    }

    pub fn manifest(&self) -> &ShadowMaterializerManifest {
        &self.manifest
    }

    pub fn pending_records(&self) -> &[TenantEventRecord] {
        &self.pending_records
    }

    fn apply_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        for write in &record.writes {
            self.apply_write(record, write)?;
        }
        if let Some(execution_id) = &record.scheduled_execution_id {
            self.scheduled_execution_ids.insert(execution_id.clone());
        }
        Ok(())
    }

    fn apply_write(&mut self, record: &TenantEventRecord, write: &WriteOp) -> Result<()> {
        self.ensure_write_table_identity(write)?;
        let document_key = (write.table_id.clone(), write.doc_id.clone());
        match write.op_type {
            WriteOpType::Insert => match (&write.previous, &write.current) {
                (None, Some(current)) => match self.documents.get(&document_key) {
                    Some(existing) if existing != current => Err(Error::Conflict(format!(
                        "shadow materializer insert replay found conflicting state for document {} at sequence {}",
                        write.doc_id, record.sequence.0
                    ))),
                    Some(_) => Ok(()),
                    None => {
                        self.documents.insert(document_key, current.clone());
                        Ok(())
                    }
                },
                _ => Err(Error::InvalidInput(format!(
                    "shadow materializer insert replay for document {} at sequence {} requires only a current snapshot",
                    write.doc_id, record.sequence.0
                ))),
            },
            WriteOpType::Update => match (&write.previous, &write.current) {
                (Some(previous), Some(current)) => {
                    let existing = self.documents.get(&document_key).ok_or_else(|| {
                        Error::Conflict(format!(
                            "shadow materializer update replay missing document {} at sequence {}",
                            write.doc_id, record.sequence.0
                        ))
                    })?;
                    if existing == current {
                        return Ok(());
                    }
                    if existing != previous {
                        return Err(Error::Conflict(format!(
                            "shadow materializer update replay found conflicting state for document {} at sequence {}",
                            write.doc_id, record.sequence.0
                        )));
                    }
                    self.documents.insert(document_key, current.clone());
                    Ok(())
                }
                _ => Err(Error::InvalidInput(format!(
                    "shadow materializer update replay for document {} at sequence {} requires both previous and current snapshots",
                    write.doc_id, record.sequence.0
                ))),
            },
            WriteOpType::Delete => match (&write.previous, &write.current) {
                (Some(previous), None) => match self.documents.remove(&document_key) {
                    Some(removed) if removed != *previous => Err(Error::Conflict(format!(
                        "shadow materializer delete replay found conflicting state for document {} at sequence {}",
                        write.doc_id, record.sequence.0
                    ))),
                    _ => Ok(()),
                },
                _ => Err(Error::InvalidInput(format!(
                    "shadow materializer delete replay for document {} at sequence {} requires only a previous snapshot",
                    write.doc_id, record.sequence.0
                ))),
            },
        }
    }

    fn ensure_write_table_identity(&mut self, write: &WriteOp) -> Result<()> {
        let key = (DEFAULT_TABLE_NAMESPACE.to_string(), write.table.clone());
        let hidden_key = (hidden_table_namespace(&write.table_id), write.table.clone());
        let staged_hidden = match self.table_identities.get(&hidden_key) {
            Some((hidden_id, TableState::Hidden)) if hidden_id == &write.table_id => true,
            Some((hidden_id, state)) => {
                return Err(Error::Conflict(format!(
                    "shadow materializer hidden slot for table {} id {} contains {} in {} state",
                    write.table, write.table_id, hidden_id, state
                )));
            }
            None => false,
        };
        match self.table_identities.get(&key).cloned() {
            Some((existing, TableState::Active)) if existing == write.table_id => {
                if staged_hidden {
                    return Err(Error::Conflict(format!(
                        "shadow materializer table {} already has active table id {} and a duplicate hidden slot",
                        write.table, write.table_id
                    )));
                }
                Ok(())
            }
            Some((existing, state)) if existing == write.table_id => Err(Error::Conflict(format!(
                "shadow materializer table {} is assigned table id {} in {} state, journal references it at document {}",
                write.table, existing, state, write.doc_id
            ))),
            Some((existing, TableState::Active)) => {
                self.ensure_table_id_is_unassigned(&write.table_id, Some(&hidden_key))?;
                let deleting_key = (deleting_table_namespace(&existing), write.table.clone());
                match self.table_identities.get(&deleting_key) {
                    Some((deleting_id, TableState::Deleting)) if deleting_id == &existing => {}
                    Some((deleting_id, state)) => {
                        return Err(Error::Conflict(format!(
                            "shadow materializer cannot retire table {} id {} because deleting slot holds {} in {} state",
                            write.table, existing, deleting_id, state
                        )));
                    }
                    None => {
                        self.table_identities
                            .insert(deleting_key, (existing, TableState::Deleting));
                    }
                }
                if staged_hidden {
                    self.table_identities.remove(&hidden_key);
                }
                self.table_identities
                    .insert(key, (write.table_id.clone(), TableState::Active));
                Ok(())
            }
            Some((existing, state)) => Err(Error::Conflict(format!(
                "shadow materializer table {} is assigned table id {} in {} state, journal references {} at document {}",
                write.table, existing, state, write.table_id, write.doc_id
            ))),
            None => {
                self.ensure_table_id_is_unassigned(&write.table_id, Some(&hidden_key))?;
                if staged_hidden {
                    self.table_identities.remove(&hidden_key);
                }
                self.table_identities
                    .insert(key, (write.table_id.clone(), TableState::Active));
                Ok(())
            }
        }
    }

    fn ensure_table_id_is_unassigned(
        &self,
        table_id: &TableId,
        allowed_key: Option<&(String, TableName)>,
    ) -> Result<()> {
        for (key, (existing_id, state)) in &self.table_identities {
            if existing_id == table_id && Some(key) != allowed_key {
                return Err(Error::Conflict(format!(
                    "shadow materializer table id {} is already assigned to namespace {} table {} in {} state",
                    table_id, key.0, key.1, state
                )));
            }
        }
        Ok(())
    }

    fn compact(&mut self) -> Result<()> {
        self.checkpoint = self.current_snapshot();
        self.pending_records.clear();
        self.manifest.checkpoint_sequence = self.checkpoint.applied_sequence;
        self.manifest.pending_record_count = 0;
        self.manifest.compaction_runs = self.manifest.compaction_runs.saturating_add(1);
        self.validate_manifest()?;
        Ok(())
    }

    fn validate_manifest(&self) -> Result<()> {
        self.manifest.validate(&self.checkpoint, self.config)?;
        if self.pending_records.len() != self.manifest.pending_record_count {
            return Err(Error::InvalidInput(format!(
                "shadow materializer manifest pending count {} does not match buffered tail length {}",
                self.manifest.pending_record_count,
                self.pending_records.len()
            )));
        }
        if let Some(first_record) = self.pending_records.first() {
            let expected_first = self.manifest.checkpoint_sequence.0.saturating_add(1);
            if first_record.sequence.0 != expected_first {
                return Err(Error::InvalidInput(format!(
                    "shadow materializer pending tail starts at sequence {} instead of {}",
                    first_record.sequence.0, expected_first
                )));
            }
        }
        if let Some(last_record) = self.pending_records.last() {
            if last_record.sequence != self.manifest.current_sequence {
                return Err(Error::InvalidInput(format!(
                    "shadow materializer pending tail ends at sequence {} instead of manifest current sequence {}",
                    last_record.sequence.0, self.manifest.current_sequence.0
                )));
            }
        } else if self.manifest.current_sequence != self.manifest.checkpoint_sequence {
            return Err(Error::InvalidInput(format!(
                "shadow materializer has no pending tail but current sequence {} differs from checkpoint sequence {}",
                self.manifest.current_sequence.0, self.manifest.checkpoint_sequence.0
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn shadow_materializer_keys_documents_by_table_id_and_document_id() {
        let shared_id =
            DocumentId::from_key("shared-document-id").expect("document id should parse");
        let messages = TableName::new("messages").expect("table should parse");
        let users = TableName::new("users").expect("table should parse");
        let messages_id = TableId::new();
        let users_id = TableId::new();
        let message = Document {
            id: shared_id.clone(),
            table: messages.clone(),
            creation_time: nimbus_core::Timestamp(1),
            update_time: nimbus_core::Timestamp(1),
            fields: serde_json::Map::from_iter([("kind".to_string(), json!("message"))]),
            typed_fields: Default::default(),
        };
        let user = Document {
            id: shared_id.clone(),
            table: users.clone(),
            creation_time: nimbus_core::Timestamp(2),
            update_time: nimbus_core::Timestamp(2),
            fields: serde_json::Map::from_iter([("kind".to_string(), json!("user"))]),
            typed_fields: Default::default(),
        };
        let checkpoint = MaterializedJournalSnapshot {
            version: crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: SequenceNumber(0),
            durable_head: SequenceNumber(0),
            table_identities: Vec::new(),
            schema: nimbus_core::Schema::default(),
            documents: Vec::new(),
            scheduled_execution_ids: Vec::new(),
        };
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(1),
                nimbus_core::Timestamp(10),
                vec![WriteOp {
                    table: messages,
                    table_id: messages_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: shared_id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(message),
                }],
                None,
            )
            .expect("message record should build"),
            TenantEventRecord::new(
                SequenceNumber(2),
                nimbus_core::Timestamp(11),
                vec![WriteOp {
                    table: users,
                    table_id: users_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: shared_id,
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(user),
                }],
                None,
            )
            .expect("user record should build"),
        ];

        let materializer = ShadowMaterializer::from_checkpoint_and_journal(
            checkpoint,
            records,
            ShadowMaterializerConfig {
                compaction_threshold_records: 8,
            },
        )
        .expect("materializer should apply records with shared document ids");

        let documents = materializer.current_documents();
        assert_eq!(documents.len(), 2);
        assert_eq!(materializer.current_table_identities().len(), 2);
        assert!(
            materializer
                .current_table_identities()
                .iter()
                .any(|identity| identity.table_id == messages_id)
        );
        assert!(
            materializer
                .current_table_identities()
                .iter()
                .any(|identity| identity.table_id == users_id)
        );
    }

    #[test]
    fn shadow_materializer_promotes_recreated_table_and_exports_only_active_documents() {
        let table = TableName::new("tasks").expect("table should parse");
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let old_document = Document {
            id: DocumentId::from_key("old-task").expect("document id should parse"),
            table: table.clone(),
            creation_time: nimbus_core::Timestamp(1),
            update_time: nimbus_core::Timestamp(1),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
            typed_fields: Default::default(),
        };
        let new_document = Document {
            id: DocumentId::from_key("new-task").expect("document id should parse"),
            table: table.clone(),
            creation_time: nimbus_core::Timestamp(2),
            update_time: nimbus_core::Timestamp(2),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
            typed_fields: Default::default(),
        };
        let checkpoint = MaterializedJournalSnapshot {
            version: crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: SequenceNumber(1),
            durable_head: SequenceNumber(1),
            table_identities: vec![
                TableIdentitySnapshotEntry::default_namespace(table.clone(), old_table_id.clone()),
                TableIdentitySnapshotEntry {
                    namespace: hidden_table_namespace(&new_table_id),
                    table: table.clone(),
                    table_id: new_table_id.clone(),
                    state: TableState::Hidden,
                },
            ],
            schema: nimbus_core::Schema::default(),
            documents: vec![old_document.clone()],
            scheduled_execution_ids: Vec::new(),
        };
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(2),
                nimbus_core::Timestamp(10),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: new_table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: new_document.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(new_document.clone()),
                }],
                None,
            )
            .expect("replacement write should build"),
        ];

        let materializer = ShadowMaterializer::from_checkpoint_and_journal(
            checkpoint,
            records,
            ShadowMaterializerConfig {
                compaction_threshold_records: 8,
            },
        )
        .expect("materializer should promote the hidden replacement identity");

        let identities = materializer.current_table_identities();
        assert!(identities.iter().any(|identity| {
            identity.namespace == DEFAULT_TABLE_NAMESPACE
                && identity.table == table
                && identity.table_id == new_table_id
                && identity.state == TableState::Active
        }));
        assert!(identities.iter().any(|identity| {
            identity.namespace == deleting_table_namespace(&old_table_id)
                && identity.table == table
                && identity.table_id == old_table_id
                && identity.state == TableState::Deleting
        }));
        assert!(!identities.iter().any(|identity| {
            identity.namespace == hidden_table_namespace(&new_table_id)
                && identity.table_id == new_table_id
        }));
        assert_eq!(materializer.current_documents(), vec![new_document]);
        materializer
            .current_snapshot()
            .validate()
            .expect("current snapshot should validate after table recreation");
    }
}
