use neovex_core::{DurableMutationRecord, Error, Result, SequenceNumber};
use redb::{ReadableTable, TableError};
use std::time::Instant;

#[cfg(test)]
mod tests;

use super::{
    COMMIT_LOG, MaterializedJournalSnapshot, TenantReadSnapshot, TenantStore, map_redb_error,
};

pub const DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT: usize = 100;
pub const MAX_DURABLE_JOURNAL_STREAM_LIMIT: usize = 1_000;

#[derive(Debug, Clone, PartialEq)]
pub struct DurableJournalPage {
    pub records: Vec<DurableMutationRecord>,
    pub next_cursor: SequenceNumber,
    pub latest_sequence: SequenceNumber,
    pub cursor_floor: SequenceNumber,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableJournalBootstrap {
    pub snapshot: MaterializedJournalSnapshot,
    pub resume_after: SequenceNumber,
    pub bootstrap_cut: SequenceNumber,
    pub cursor_floor: SequenceNumber,
}

impl TenantStore {
    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        self.read_snapshot()?.stream_durable_journal(after, limit)
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        self.read_snapshot()?.export_durable_journal_bootstrap()
    }
}

impl TenantReadSnapshot {
    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let total_started = Instant::now();
        validate_durable_journal_stream_limit(limit)?;

        let latest_sequence_started = Instant::now();
        let latest_sequence = self.latest_sequence()?;
        let latest_sequence_elapsed = latest_sequence_started.elapsed();
        let cursor_floor_started = Instant::now();
        let cursor_floor = self.durable_journal_cursor_floor()?;
        let cursor_floor_elapsed = cursor_floor_started.elapsed();
        if after.0 < cursor_floor.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is behind the retention floor {}",
                after.0, cursor_floor.0
            )));
        }
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }

        let open_table_started = Instant::now();
        let table_handle = match self.read_txn.open_table(COMMIT_LOG) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => {
                return Ok(DurableJournalPage {
                    records: Vec::new(),
                    next_cursor: after,
                    latest_sequence,
                    cursor_floor,
                    has_more: false,
                });
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let open_table_elapsed = open_table_started.elapsed();

        let mut records = Vec::with_capacity(limit);
        let mut has_more = false;
        let from = after.0.saturating_add(1);
        let scan_started = Instant::now();
        for item in table_handle.range(from..).map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            if records.len() == limit {
                has_more = true;
                break;
            }
            records.push(crate::commit_log::deserialize_durable_record(
                value.value(),
            )?);
        }
        let scan_elapsed = scan_started.elapsed();

        let next_cursor = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(after);
        maybe_emit_redb_journal_profile(format_args!(
            "redb-journal-profile op=stream latest_sequence={:?} cursor_floor={:?} open_table={:?} scan={:?} records={} has_more={} total={:?}",
            latest_sequence_elapsed,
            cursor_floor_elapsed,
            open_table_elapsed,
            scan_elapsed,
            records.len(),
            has_more,
            total_started.elapsed(),
        ));
        Ok(DurableJournalPage {
            records,
            next_cursor,
            latest_sequence,
            cursor_floor,
            has_more,
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let total_started = Instant::now();
        let snapshot_started = Instant::now();
        let snapshot = self.export_materialized_journal_snapshot()?;
        let snapshot_elapsed = snapshot_started.elapsed();
        let cursor_floor_started = Instant::now();
        let cursor_floor = self.durable_journal_cursor_floor()?;
        let cursor_floor_elapsed = cursor_floor_started.elapsed();
        maybe_emit_redb_journal_profile(format_args!(
            "redb-journal-profile op=bootstrap snapshot={:?} cursor_floor={:?} documents={} scheduled_execution_ids={} total={:?}",
            snapshot_elapsed,
            cursor_floor_elapsed,
            snapshot.documents.len(),
            snapshot.scheduled_execution_ids.len(),
            total_started.elapsed(),
        ));
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let table_handle = match self.read_txn.open_table(COMMIT_LOG) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(SequenceNumber(0)),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut entries = table_handle.iter().map_err(map_redb_error)?;
        let Some(item) = entries.next() else {
            return Ok(SequenceNumber(0));
        };
        let (key, _) = item.map_err(map_redb_error)?;
        Ok(SequenceNumber(key.value().saturating_sub(1)))
    }
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "journal stream limit {limit} exceeds the maximum {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
}

fn maybe_emit_redb_journal_profile(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("NEOVEX_REDB_JOURNAL_PROFILE").is_none() {
        return;
    }

    eprintln!("{args}");
}
