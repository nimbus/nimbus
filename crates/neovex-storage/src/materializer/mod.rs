use std::collections::{BTreeMap, BTreeSet};

use neovex_core::{
    Document, DurableMutationRecord, Error, Result, SequenceNumber, WriteOp, WriteOpType,
};

use crate::MaterializedJournalSnapshot;

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
    documents: BTreeMap<neovex_core::DocumentId, Document>,
    scheduled_execution_ids: BTreeSet<String>,
    pending_records: Vec<DurableMutationRecord>,
}

impl ShadowMaterializer {
    pub fn from_checkpoint_and_journal(
        checkpoint: MaterializedJournalSnapshot,
        journal_tail: Vec<DurableMutationRecord>,
        config: ShadowMaterializerConfig,
    ) -> Result<Self> {
        let config = config.validate()?;
        checkpoint.validate()?;

        let documents = checkpoint
            .documents
            .iter()
            .cloned()
            .map(|document| (document.id, document))
            .collect::<BTreeMap<_, _>>();
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
        pending_records: Vec<DurableMutationRecord>,
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

    pub fn apply_records(&mut self, records: Vec<DurableMutationRecord>) -> Result<()> {
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
            schema: self.checkpoint.schema.clone(),
            documents: self.current_documents(),
            scheduled_execution_ids: self.current_scheduled_execution_ids(),
        }
    }

    pub fn current_documents(&self) -> Vec<Document> {
        self.documents.values().cloned().collect()
    }

    pub fn current_scheduled_execution_ids(&self) -> Vec<String> {
        self.scheduled_execution_ids.iter().cloned().collect()
    }

    pub fn manifest(&self) -> &ShadowMaterializerManifest {
        &self.manifest
    }

    pub fn pending_records(&self) -> &[DurableMutationRecord] {
        &self.pending_records
    }

    fn apply_record(&mut self, record: &DurableMutationRecord) -> Result<()> {
        for write in &record.writes {
            self.apply_write(record, write)?;
        }
        if let Some(execution_id) = &record.scheduled_execution_id {
            self.scheduled_execution_ids.insert(execution_id.clone());
        }
        Ok(())
    }

    fn apply_write(&mut self, record: &DurableMutationRecord, write: &WriteOp) -> Result<()> {
        match write.op_type {
            WriteOpType::Insert => match (&write.previous, &write.current) {
                (None, Some(current)) => match self.documents.get(&write.doc_id) {
                    Some(existing) if existing != current => Err(Error::Conflict(format!(
                        "shadow materializer insert replay found conflicting state for document {} at sequence {}",
                        write.doc_id, record.sequence.0
                    ))),
                    Some(_) => Ok(()),
                    None => {
                        self.documents.insert(current.id, current.clone());
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
                    let existing = self.documents.get(&write.doc_id).ok_or_else(|| {
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
                    self.documents.insert(current.id, current.clone());
                    Ok(())
                }
                _ => Err(Error::InvalidInput(format!(
                    "shadow materializer update replay for document {} at sequence {} requires both previous and current snapshots",
                    write.doc_id, record.sequence.0
                ))),
            },
            WriteOpType::Delete => match (&write.previous, &write.current) {
                (Some(previous), None) => match self.documents.remove(&write.doc_id) {
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
