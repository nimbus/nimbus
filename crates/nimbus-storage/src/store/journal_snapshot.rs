use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use nimbus_core::{
    CommitSequence, CommitTimestamp, Document, DurableMutationRecord, Error,
    HistoricalReadErrorKind, HistoricalReadSnapshot, ReadTimestamp, Result, Schema, SequenceNumber,
    TableId, TableName, TableState, Timestamp,
};
use redb::{ReadableTable, TableError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::keys::document_key;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, TableIdentitySnapshotEntry, deleting_table_namespace,
    hidden_table_namespace,
};
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, CURRENT_INDEX_VERSION_STORAGE_FORMAT,
    CURRENT_STORAGE_FORMAT_VERSION, RetentionGcConfig, StorageFormatVersion,
};

#[cfg(test)]
mod tests;

use super::journal::encode_u64;
use super::schema_rewrite::durable_record_index_keys_in_write_txn;
use super::table_catalog::{ensure_table_id_in_write_txn, export_table_identities_in_read_txn};
use super::{
    APPLIED_SEQUENCE_KEY, DOCUMENTS, EMPTY_TABLE_VALUE, INDEXES, JournalProgress, METADATA,
    NEXT_SEQUENCE_KEY, SCHEDULED_JOB_EXECUTIONS, SCHEMAS, TenantReadSnapshot, TenantStore,
    map_redb_error,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterializedJournalSnapshot {
    pub version: u16,
    pub applied_sequence: SequenceNumber,
    pub durable_head: SequenceNumber,
    pub table_identities: Vec<TableIdentitySnapshotEntry>,
    pub schema: Schema,
    pub documents: Vec<Document>,
    pub scheduled_execution_ids: Vec<String>,
}

pub(crate) const MATERIALIZED_JOURNAL_SNAPSHOT_VERSION: u16 = 3;
pub(crate) const POINT_IN_TIME_RESTORE_ARCHIVE_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointInTimeRestoreTarget {
    Sequence(SequenceNumber),
    Timestamp(Timestamp),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointInTimeRestoreArchive {
    pub version: u16,
    pub target_sequence: SequenceNumber,
    pub target_timestamp: Timestamp,
    pub base_snapshot: MaterializedJournalSnapshot,
    pub journal_tail: Vec<DurableMutationRecord>,
    pub storage_format_version: StorageFormatVersion,
    pub document_version_storage_format: StorageFormatVersion,
    pub index_version_storage_format: StorageFormatVersion,
    pub target_fingerprint: String,
}

impl MaterializedJournalSnapshot {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != MATERIALIZED_JOURNAL_SNAPSHOT_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported materialized journal snapshot version {}",
                self.version
            )));
        }
        if self.applied_sequence.0 > self.durable_head.0 {
            return Err(Error::InvalidInput(format!(
                "materialized journal snapshot applied sequence {} exceeds durable head {}",
                self.applied_sequence.0, self.durable_head.0
            )));
        }
        let mut by_key = BTreeMap::<(&str, &TableName), &TableId>::new();
        let mut by_id = BTreeMap::<&TableId, (&str, &TableName)>::new();
        for identity in &self.table_identities {
            if identity.namespace.is_empty() {
                return Err(Error::InvalidInput(
                    "materialized journal snapshot table identity namespace cannot be empty"
                        .to_string(),
                ));
            }
            let expected_namespace = match identity.state {
                TableState::Active => DEFAULT_TABLE_NAMESPACE.to_string(),
                TableState::Hidden => hidden_table_namespace(&identity.table_id),
                TableState::Deleting => deleting_table_namespace(&identity.table_id),
            };
            if identity.namespace != expected_namespace {
                return Err(Error::InvalidInput(format!(
                    "materialized journal snapshot table identity for table {} id {} has namespace {} but {} state requires {}",
                    identity.table,
                    identity.table_id,
                    identity.namespace,
                    identity.state,
                    expected_namespace
                )));
            }
            let key = (identity.namespace.as_str(), &identity.table);
            if by_key.insert(key, &identity.table_id).is_some() {
                return Err(Error::InvalidInput(format!(
                    "materialized journal snapshot has duplicate table identity for namespace {} table {}",
                    identity.namespace, identity.table
                )));
            }
            if let Some((existing_namespace, existing_table)) =
                by_id.insert(&identity.table_id, key)
            {
                return Err(Error::InvalidInput(format!(
                    "materialized journal snapshot assigns table id {} to both {}.{} and {}.{}",
                    identity.table_id,
                    existing_namespace,
                    existing_table,
                    identity.namespace,
                    identity.table
                )));
            }
        }

        let default_tables = self
            .table_identities
            .iter()
            .filter(|identity| {
                identity.namespace == DEFAULT_TABLE_NAMESPACE
                    && identity.state == TableState::Active
            })
            .map(|identity| identity.table.clone())
            .collect::<BTreeSet<_>>();
        for table_schema in self.schema.tables.values() {
            if !default_tables.contains(&table_schema.table) {
                return Err(Error::InvalidInput(format!(
                    "materialized journal snapshot schema for table {} is missing a table identity",
                    table_schema.table
                )));
            }
        }
        for document in &self.documents {
            if !default_tables.contains(&document.table) {
                return Err(Error::InvalidInput(format!(
                    "materialized journal snapshot document {} in table {} is missing a table identity",
                    document.id, document.table
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn default_table_id(&self, table: &TableName) -> Result<TableId> {
        self.table_identities
            .iter()
            .find(|identity| {
                identity.namespace == DEFAULT_TABLE_NAMESPACE
                    && identity.table == *table
                    && identity.state == TableState::Active
            })
            .map(|identity| identity.table_id.clone())
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "materialized journal snapshot is missing a table identity for table {}",
                    table
                ))
            })
    }

    pub fn canonical_fingerprint(&self) -> Result<String> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.table_identities.sort_by(|left, right| {
            left.namespace
                .cmp(&right.namespace)
                .then_with(|| left.table.as_str().cmp(right.table.as_str()))
                .then_with(|| left.table_id.as_str().cmp(right.table_id.as_str()))
                .then_with(|| left.state.cmp(&right.state))
        });
        canonical.documents.sort_by(|left, right| {
            left.table
                .as_str()
                .cmp(right.table.as_str())
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        canonical.scheduled_execution_ids.sort_unstable();
        let payload = serde_json::to_vec(&canonical)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(payload.as_slice())))
    }

    pub(crate) fn empty_for_point_in_time_base() -> Self {
        Self {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: SequenceNumber(0),
            durable_head: SequenceNumber(0),
            table_identities: Vec::new(),
            schema: Schema::default(),
            documents: Vec::new(),
            scheduled_execution_ids: Vec::new(),
        }
    }
}

impl PointInTimeRestoreArchive {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != POINT_IN_TIME_RESTORE_ARCHIVE_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported point-in-time restore archive version {}",
                self.version
            )));
        }
        if self.storage_format_version != CURRENT_STORAGE_FORMAT_VERSION {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                format!(
                    "point-in-time archive storage format {:?} does not match current {:?}",
                    self.storage_format_version, CURRENT_STORAGE_FORMAT_VERSION
                ),
            ));
        }
        if self.document_version_storage_format != CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                format!(
                    "point-in-time archive document-version format {:?} does not match current {:?}",
                    self.document_version_storage_format, CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT
                ),
            ));
        }
        if self.index_version_storage_format != CURRENT_INDEX_VERSION_STORAGE_FORMAT {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                format!(
                    "point-in-time archive index-version format {:?} does not match current {:?}",
                    self.index_version_storage_format, CURRENT_INDEX_VERSION_STORAGE_FORMAT
                ),
            ));
        }
        self.base_snapshot.validate()?;
        if self.target_sequence.0 < self.base_snapshot.applied_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "point-in-time target sequence {} is behind base snapshot sequence {}",
                self.target_sequence.0, self.base_snapshot.applied_sequence.0
            )));
        }
        let mut expected = self.base_snapshot.applied_sequence.0.saturating_add(1);
        for record in &self.journal_tail {
            record.validate_integrity()?;
            if record.sequence.0 != expected {
                return Err(Error::InvalidInput(format!(
                    "point-in-time archive expected journal sequence {}, got {}",
                    expected, record.sequence.0
                )));
            }
            if record.sequence.0 > self.target_sequence.0 {
                return Err(Error::InvalidInput(format!(
                    "point-in-time archive journal sequence {} exceeds target {}",
                    record.sequence.0, self.target_sequence.0
                )));
            }
            expected = expected.saturating_add(1);
        }
        if self.target_sequence.0 > self.base_snapshot.applied_sequence.0
            && self
                .journal_tail
                .last()
                .is_none_or(|record| record.sequence != self.target_sequence)
        {
            return Err(Error::InvalidInput(format!(
                "point-in-time archive is missing target sequence {}",
                self.target_sequence.0
            )));
        }
        Ok(())
    }
}

impl TenantStore {
    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        self.read_snapshot()?.export_materialized_journal_snapshot()
    }

    pub fn restore_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
    ) -> Result<()> {
        snapshot.validate()?;
        self.ensure_materialized_journal_restore_target_is_empty()?;

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        for identity in &snapshot.table_identities {
            ensure_table_id_in_write_txn(&write_txn, identity)?;
        }
        {
            let mut schema_table = write_txn.open_table(SCHEMAS).map_err(map_redb_error)?;
            for table_schema in snapshot.schema.tables.values() {
                let payload = rmp_serde::to_vec(table_schema)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                schema_table
                    .insert(table_schema.table.as_str(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            for document in &snapshot.documents {
                let table_id = snapshot.default_table_id(&document.table)?;
                let payload = encode_document_msgpack(document)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                let key = document_key(&table_id, &document.id);
                documents
                    .insert(key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for document in &snapshot.documents {
                let Some(table_schema) = snapshot.schema.get_table(&document.table) else {
                    continue;
                };
                for key in
                    durable_record_index_keys_in_write_txn(&write_txn, document, table_schema)?
                {
                    index_table
                        .insert(key.as_slice(), EMPTY_TABLE_VALUE)
                        .map_err(map_redb_error)?;
                }
            }
        }
        {
            let mut executions = write_txn
                .open_table(SCHEDULED_JOB_EXECUTIONS)
                .map_err(map_redb_error)?;
            for execution_id in &snapshot.scheduled_execution_ids {
                executions
                    .insert(execution_id.as_str(), EMPTY_TABLE_VALUE)
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
            metadata
                .insert(
                    NEXT_SEQUENCE_KEY,
                    encode_u64(snapshot.applied_sequence.0.saturating_add(1)).as_slice(),
                )
                .map_err(map_redb_error)?;
            metadata
                .insert(
                    APPLIED_SEQUENCE_KEY,
                    encode_u64(snapshot.applied_sequence.0).as_slice(),
                )
                .map_err(map_redb_error)?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    pub fn rebuild_materialized_journal_from_snapshot(
        &self,
        snapshot: &MaterializedJournalSnapshot,
        journal_tail: &[DurableMutationRecord],
        target_sequence: Option<SequenceNumber>,
    ) -> Result<JournalProgress> {
        snapshot.validate()?;
        let available_head = journal_tail
            .last()
            .map(|record| record.sequence)
            .unwrap_or(snapshot.applied_sequence);
        if let Some(target_sequence) = target_sequence {
            if target_sequence.0 < snapshot.applied_sequence.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is behind snapshot sequence {}",
                    target_sequence.0, snapshot.applied_sequence.0
                )));
            }
            if target_sequence.0 > available_head.0 {
                return Err(Error::InvalidInput(format!(
                    "rebuild target sequence {} is beyond available journal head {}",
                    target_sequence.0, available_head.0
                )));
            }
        } else if available_head.0 < snapshot.durable_head.0 {
            return Err(Error::InvalidInput(format!(
                "journal tail is incomplete for snapshot boundary: available head {} is behind snapshot durable head {}",
                available_head.0, snapshot.durable_head.0
            )));
        }

        self.restore_materialized_journal_from_snapshot(snapshot)?;
        let replay_target = target_sequence.unwrap_or_else(|| {
            journal_tail
                .last()
                .map(|record| record.sequence)
                .unwrap_or(snapshot.applied_sequence)
        });
        let tail = journal_tail
            .iter()
            .filter(|record| {
                record.sequence.0 > snapshot.applied_sequence.0
                    && record.sequence.0 <= replay_target.0
            })
            .cloned()
            .collect::<Vec<_>>();
        self.append_durable_records_batch(&tail)?;
        self.recover_durable_journal()
    }

    pub fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        let records = self.read_durable_journal_from(SequenceNumber(1))?;
        let progress = self.journal_progress()?;
        let watermarks = self.retention_gc_watermarks(retention_config)?;
        build_point_in_time_restore_archive(
            target,
            records,
            progress.durable_head,
            watermarks.document_versions.safe_prune_before,
        )
    }

    pub fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        archive.validate()?;
        let progress = self.rebuild_materialized_journal_from_snapshot(
            &archive.base_snapshot,
            &archive.journal_tail,
            Some(archive.target_sequence),
        )?;
        let restored_fingerprint = self
            .export_materialized_journal_snapshot()?
            .canonical_fingerprint()?;
        if restored_fingerprint != archive.target_fingerprint {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore fingerprint mismatch: restored {} expected {}",
                    restored_fingerprint, archive.target_fingerprint
                ),
            ));
        }
        Ok(progress)
    }

    fn ensure_materialized_journal_restore_target_is_empty(&self) -> Result<()> {
        let snapshot = self.read_snapshot()?;
        let progress = snapshot.journal_progress()?;
        if progress.durable_head.0 != 0
            || progress.applied_head.0 != 0
            || !snapshot.documents()?.is_empty()
            || !snapshot.load_schema()?.tables.is_empty()
            || !snapshot.table_identities()?.is_empty()
            || !snapshot.scheduled_execution_ids()?.is_empty()
        {
            return Err(Error::Internal(
                "materialized journal snapshot restore requires an empty tenant store".to_string(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn resolve_point_in_time_target(
    target: PointInTimeRestoreTarget,
    records: &[DurableMutationRecord],
    durable_head: SequenceNumber,
) -> Result<(SequenceNumber, Timestamp)> {
    match target {
        PointInTimeRestoreTarget::Sequence(sequence) => {
            if sequence.0 > durable_head.0 {
                return Err(Error::InvalidInput(format!(
                    "point-in-time target sequence {} is beyond durable head {}",
                    sequence.0, durable_head.0
                )));
            }
            let timestamp = if sequence.0 == 0 {
                Timestamp(0)
            } else {
                records
                    .iter()
                    .find(|record| record.sequence == sequence)
                    .map(|record| record.timestamp)
                    .ok_or_else(|| {
                        Error::historical_read(
                            HistoricalReadErrorKind::TimestampOutOfRange,
                            format!(
                                "point-in-time target sequence {} is not retained",
                                sequence.0
                            ),
                        )
                    })?
            };
            Ok((sequence, timestamp))
        }
        PointInTimeRestoreTarget::Timestamp(timestamp) => {
            let snapshot = HistoricalReadSnapshot::resolve_at_or_before(
                ReadTimestamp::new(timestamp),
                records.iter().map(|record| {
                    (
                        CommitTimestamp::new(record.timestamp),
                        CommitSequence::new(record.sequence),
                    )
                }),
            )?;
            Ok((
                snapshot.sequence().sequence(),
                snapshot.commit_timestamp().timestamp(),
            ))
        }
    }
}

pub(crate) fn build_point_in_time_restore_archive(
    target: PointInTimeRestoreTarget,
    records: Vec<DurableMutationRecord>,
    durable_head: SequenceNumber,
    retention_floor: SequenceNumber,
) -> Result<PointInTimeRestoreArchive> {
    let (target_sequence, target_timestamp) =
        resolve_point_in_time_target(target, &records, durable_head)?;
    if target_sequence.0 < retention_floor.0 {
        return Err(Error::historical_read(
            HistoricalReadErrorKind::RetentionExpired,
            format!(
                "point-in-time target sequence {} is older than retention floor {}",
                target_sequence.0, retention_floor.0
            ),
        ));
    }
    let base_snapshot = MaterializedJournalSnapshot::empty_for_point_in_time_base();
    let journal_tail = records
        .into_iter()
        .filter(|record| {
            record.sequence.0 > base_snapshot.applied_sequence.0
                && record.sequence.0 <= target_sequence.0
        })
        .collect::<Vec<_>>();
    let target_fingerprint = materialized_snapshot_fingerprint_after_rebuild(
        &base_snapshot,
        &journal_tail,
        target_sequence,
    )?;

    Ok(PointInTimeRestoreArchive {
        version: POINT_IN_TIME_RESTORE_ARCHIVE_VERSION,
        target_sequence,
        target_timestamp,
        base_snapshot,
        journal_tail,
        storage_format_version: CURRENT_STORAGE_FORMAT_VERSION,
        document_version_storage_format: CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT,
        index_version_storage_format: CURRENT_INDEX_VERSION_STORAGE_FORMAT,
        target_fingerprint,
    })
}

pub(crate) fn validate_point_in_time_archive_for_journal_replay_import(
    archive: &PointInTimeRestoreArchive,
) -> Result<()> {
    archive.validate()?;
    validate_materialized_journal_replay_base_is_empty(&archive.base_snapshot)
}

pub(crate) fn validate_materialized_journal_replay_base_is_empty(
    snapshot: &MaterializedJournalSnapshot,
) -> Result<()> {
    if snapshot.applied_sequence.0 != 0
        || snapshot.durable_head.0 != 0
        || !snapshot.table_identities.is_empty()
        || !snapshot.schema.tables.is_empty()
        || !snapshot.documents.is_empty()
        || !snapshot.scheduled_execution_ids.is_empty()
    {
        return Err(Error::InvalidInput(
            "journal-replay restore requires an empty sequence-0 materialized snapshot".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn materialized_snapshot_fingerprint_after_rebuild(
    base_snapshot: &MaterializedJournalSnapshot,
    journal_tail: &[DurableMutationRecord],
    target_sequence: SequenceNumber,
) -> Result<String> {
    let restored = TenantStore::create_in_memory()?;
    restored.rebuild_materialized_journal_from_snapshot(
        base_snapshot,
        journal_tail,
        Some(target_sequence),
    )?;
    restored
        .export_materialized_journal_snapshot()?
        .canonical_fingerprint()
}

impl TenantReadSnapshot {
    pub fn table_identities(&self) -> Result<Vec<TableIdentitySnapshotEntry>> {
        export_table_identities_in_read_txn(&self.read_txn)
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        let total_started = Instant::now();
        let progress_started = Instant::now();
        let progress = self.journal_progress()?;
        let progress_elapsed = progress_started.elapsed();
        let schema_started = Instant::now();
        let schema = self.load_schema()?;
        let schema_elapsed = schema_started.elapsed();
        let table_identity_started = Instant::now();
        let table_identities = self.table_identities()?;
        let table_identity_elapsed = table_identity_started.elapsed();
        let documents_started = Instant::now();
        let documents = self.documents()?;
        let documents_elapsed = documents_started.elapsed();
        let scheduled_started = Instant::now();
        let scheduled_execution_ids = self.scheduled_execution_ids()?;
        let scheduled_elapsed = scheduled_started.elapsed();
        maybe_emit_redb_journal_profile(format_args!(
            "redb-journal-profile op=export-snapshot progress={:?} schema={:?} table_identities={:?} documents={:?} scheduled_execution_ids={:?} table_identity_count={} document_count={} scheduled_execution_count={} total={:?}",
            progress_elapsed,
            schema_elapsed,
            table_identity_elapsed,
            documents_elapsed,
            scheduled_elapsed,
            table_identities.len(),
            documents.len(),
            scheduled_execution_ids.len(),
            total_started.elapsed(),
        ));
        Ok(MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: progress.applied_head,
            durable_head: progress.durable_head,
            table_identities,
            schema,
            documents,
            scheduled_execution_ids,
        })
    }

    pub fn documents(&self) -> Result<Vec<Document>> {
        let total_started = Instant::now();
        let open_table_started = Instant::now();
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => {
                maybe_emit_redb_journal_profile(format_args!(
                    "redb-journal-profile op=documents open_table={:?} iterate={:?} documents=0 total={:?}",
                    open_table_started.elapsed(),
                    std::time::Duration::ZERO,
                    total_started.elapsed(),
                ));
                return Ok(Vec::new());
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let open_table_elapsed = open_table_started.elapsed();

        let mut documents = Vec::new();
        let iterate_started = Instant::now();
        let mut next_item_elapsed = std::time::Duration::ZERO;
        let mut decode_elapsed = std::time::Duration::ZERO;
        let mut iter = table_handle.iter().map_err(map_redb_error)?;
        loop {
            let next_item_started = Instant::now();
            let Some(item) = iter.next() else {
                break;
            };
            next_item_elapsed += next_item_started.elapsed();
            let (_, value) = item.map_err(map_redb_error)?;
            let decode_started = Instant::now();
            documents.push(
                decode_document_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            );
            decode_elapsed += decode_started.elapsed();
        }
        let iterate_elapsed = iterate_started.elapsed();
        maybe_emit_redb_journal_profile(format_args!(
            "redb-journal-profile op=documents open_table={:?} iterate={:?} next_item={:?} decode={:?} documents={} total={:?}",
            open_table_elapsed,
            iterate_elapsed,
            next_item_elapsed,
            decode_elapsed,
            documents.len(),
            total_started.elapsed(),
        ));

        Ok(documents)
    }

    pub fn scheduled_execution_ids(&self) -> Result<Vec<String>> {
        let total_started = Instant::now();
        let open_table_started = Instant::now();
        let table_handle = match self.read_txn.open_table(SCHEDULED_JOB_EXECUTIONS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => {
                maybe_emit_redb_journal_profile(format_args!(
                    "redb-journal-profile op=scheduled-executions open_table={:?} iterate={:?} scheduled_execution_ids=0 total={:?}",
                    open_table_started.elapsed(),
                    std::time::Duration::ZERO,
                    total_started.elapsed(),
                ));
                return Ok(Vec::new());
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let open_table_elapsed = open_table_started.elapsed();

        let mut execution_ids = Vec::new();
        let iterate_started = Instant::now();
        for item in table_handle.iter().map_err(map_redb_error)? {
            let (key, _) = item.map_err(map_redb_error)?;
            execution_ids.push(key.value().to_string());
        }
        let iterate_elapsed = iterate_started.elapsed();
        execution_ids.sort_unstable();
        maybe_emit_redb_journal_profile(format_args!(
            "redb-journal-profile op=scheduled-executions open_table={:?} iterate={:?} scheduled_execution_ids={} total={:?}",
            open_table_elapsed,
            iterate_elapsed,
            execution_ids.len(),
            total_started.elapsed(),
        ));
        Ok(execution_ids)
    }
}

fn maybe_emit_redb_journal_profile(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NIMBUS_REDB_JOURNAL_PROFILE").is_none() {
        return;
    }

    eprintln!("{args}");
}
