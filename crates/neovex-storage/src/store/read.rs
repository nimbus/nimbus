use neovex_core::{
    Document, DocumentId, Error, Filter, Result, Schema, SequenceNumber, TableName, TableSchema,
    Timestamp,
};
use redb::{ReadableTable, TableError};
use std::time::{Duration, Instant};

use crate::document_codec::decode_document_msgpack;
use crate::keys::{document_key, prefix_end, table_prefix};

use super::journal::decode_u64;
use super::scan::ScanPushdown;
use super::{
    APPLIED_SEQUENCE_KEY, DOCUMENTS, JournalProgress, METADATA, NEXT_SEQUENCE_KEY,
    SCHEDULED_JOB_EXECUTIONS, SCHEMAS, TenantReadSnapshot, TenantStore, map_redb_error,
};

impl TenantStore {
    pub fn read_snapshot(&self) -> Result<TenantReadSnapshot> {
        Ok(TenantReadSnapshot {
            read_txn: self.db.begin_read().map_err(map_redb_error)?,
            scan_metrics: self.scan_metrics.clone(),
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.read_snapshot()?.get(table, id)
    }

    pub fn scan_table(&self, table: &TableName) -> Result<Vec<Document>> {
        self.scan_table_cancellable(table, &mut || Ok(()))
    }

    pub fn scan_table_cancellable(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.scan_table_matching_cancellable(table, check_cancel, |_document| Ok(true))
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_cancellable(table, check_cancel, |document| {
                include_document(document)
            })
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                |document| include_document(document),
            )
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        self.read_snapshot()?
            .scheduled_execution_exists(execution_id)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        self.read_snapshot()?.latest_sequence()
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        self.read_snapshot()?.applied_sequence()
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        self.read_snapshot()?.journal_progress()
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    #[cfg(test)]
    pub(crate) fn scan_stats(&self) -> super::ScanStats {
        self.scan_metrics.stats()
    }
}

impl TenantReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        let total_started = Instant::now();
        let open_table_started = Instant::now();
        let table_handle = match self.read_txn.open_table(SCHEMAS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => {
                maybe_emit_redb_read_profile(format_args!(
                    "redb-read-profile op=load-schema open_table={:?} iterate={:?} tables=0 total={:?}",
                    open_table_started.elapsed(),
                    Duration::ZERO,
                    total_started.elapsed(),
                ));
                return Ok(Schema::default());
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let open_table_elapsed = open_table_started.elapsed();

        let mut schema = Schema::default();
        let iterate_started = Instant::now();
        for item in table_handle.iter().map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            let table_schema: TableSchema = rmp_serde::from_slice(value.value())
                .map_err(|error| Error::Serialization(error.to_string()))?;
            schema
                .tables
                .insert(table_schema.table.clone(), table_schema);
        }
        let iterate_elapsed = iterate_started.elapsed();
        maybe_emit_redb_read_profile(format_args!(
            "redb-read-profile op=load-schema open_table={:?} iterate={:?} tables={} total={:?}",
            open_table_elapsed,
            iterate_elapsed,
            schema.tables.len(),
            total_started.elapsed(),
        ));

        Ok(schema)
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let table_handle = match self.read_txn.open_table(SCHEDULED_JOB_EXECUTIONS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(false),
            Err(error) => return Err(map_redb_error(error)),
        };

        Ok(table_handle
            .get(execution_id)
            .map_err(map_redb_error)?
            .is_some())
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };

        let key = document_key(table, id);
        match table_handle.get(key.as_slice()).map_err(map_redb_error)? {
            Some(value) => Ok(Some(
                decode_document_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?,
            )),
            None => Ok(None),
        }
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.scan_table_matching_with_pushdown_cancellable(
            table,
            None,
            check_cancel,
            include_document,
        )
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let pushdown = ScanPushdown::compile(filters);
        self.scan_table_matching_with_pushdown_cancellable(
            table,
            pushdown.as_ref(),
            check_cancel,
            include_document,
        )
    }

    fn scan_table_matching_with_pushdown_cancellable<F>(
        &self,
        table: &TableName,
        pushdown: Option<&ScanPushdown>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let table_handle = match self.read_txn.open_table(DOCUMENTS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let start = table_prefix(table);
        let mut documents = Vec::new();
        match prefix_end(&start) {
            Some(end) => {
                let iter = table_handle
                    .range(start.as_slice()..end.as_slice())
                    .map_err(map_redb_error)?;
                for item in iter {
                    check_cancel()?;
                    let (_, value) = item.map_err(map_redb_error)?;
                    self.scan_metrics.record_scanned_row();
                    if pushdown
                        .is_some_and(|pushdown| pushdown.rejects_document_bytes(value.value()))
                    {
                        self.scan_metrics.record_pushdown_rejected_row();
                        continue;
                    }
                    self.scan_metrics.record_decoded_row();
                    let document = decode_document_msgpack(value.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if include_document(&document)? {
                        documents.push(document);
                    }
                }
            }
            None => {
                let iter = table_handle
                    .range(start.as_slice()..)
                    .map_err(map_redb_error)?;
                for item in iter {
                    check_cancel()?;
                    let (key, value) = item.map_err(map_redb_error)?;
                    if !key.value().starts_with(&start) {
                        break;
                    }
                    self.scan_metrics.record_scanned_row();
                    if pushdown
                        .is_some_and(|pushdown| pushdown.rejects_document_bytes(value.value()))
                    {
                        self.scan_metrics.record_pushdown_rejected_row();
                        continue;
                    }
                    self.scan_metrics.record_decoded_row();
                    let document = decode_document_msgpack(value.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if include_document(&document)? {
                        documents.push(document);
                    }
                }
            }
        }
        Ok(documents)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        let table_handle = match self.read_txn.open_table(METADATA) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(SequenceNumber(0)),
            Err(error) => return Err(map_redb_error(error)),
        };

        let next = match table_handle
            .get(NEXT_SEQUENCE_KEY)
            .map_err(map_redb_error)?
        {
            Some(value) => decode_u64(value.value())?,
            None => 1,
        };
        Ok(SequenceNumber(next.saturating_sub(1)))
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        let table_handle = match self.read_txn.open_table(METADATA) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(SequenceNumber(0)),
            Err(error) => return Err(map_redb_error(error)),
        };

        let applied = match table_handle
            .get(APPLIED_SEQUENCE_KEY)
            .map_err(map_redb_error)?
        {
            Some(value) => decode_u64(value.value())?,
            None => 0,
        };
        Ok(SequenceNumber(applied))
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        let total_started = Instant::now();
        let durable_head_started = Instant::now();
        let durable_head = self.latest_sequence()?;
        let durable_head_elapsed = durable_head_started.elapsed();
        let applied_head_started = Instant::now();
        let applied_head = self.applied_sequence()?;
        let applied_head_elapsed = applied_head_started.elapsed();
        maybe_emit_redb_read_profile(format_args!(
            "redb-read-profile op=journal-progress durable_head={:?} applied_head={:?} total={:?}",
            durable_head_elapsed,
            applied_head_elapsed,
            total_started.elapsed(),
        ));
        Ok(JournalProgress {
            durable_head,
            applied_head,
        })
    }
}

fn maybe_emit_redb_read_profile(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEOVEX_REDB_JOURNAL_PROFILE").is_none() {
        return;
    }

    eprintln!("{args}");
}
