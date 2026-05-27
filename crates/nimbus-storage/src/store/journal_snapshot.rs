use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use nimbus_core::{
    Document, DurableMutationRecord, Error, Result, Schema, SequenceNumber, TableId, TableName,
    TableState,
};
use redb::{ReadableTable, TableError};

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::keys::document_key;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, TableIdentitySnapshotEntry, deleting_table_namespace,
    hidden_table_namespace,
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

#[derive(Debug, Clone, PartialEq)]
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
