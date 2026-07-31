//! SQLite operations that exist only because libsql keeps a derivative SQLite
//! replica cache.
//!
//! The embedded SQLite provider is the system of record for its own data: it
//! never reconciles against a remote journal head and never rebuilds its
//! indexes from a re-synced schema. Both operations here belong to the replica
//! lifecycle in `crate::libsql`, and live under `sqlite/` only because they run
//! against a `rusqlite` connection and need this module tree's internals. They
//! are gathered here so the sqlite modules proper carry no libsql-conditional
//! code.

#[cfg(any(test, feature = "test-hooks"))]
use super::config::observe_sqlite_foreground_commit;
#[cfg(test)]
use super::config::{SqliteWriteStatementConcept, observe_sqlite_cached_statement};
use super::journal::{latest_sequence_in_conn, put_metadata_in_conn};
use super::*;

impl SqliteTenantStore {
    /// Reconciles a remotely fetched journal range into a derivative SQLite
    /// replica cache in one write transaction.
    ///
    /// Concurrent refreshers may fetch from the same former local head. By the
    /// time either owns the cache transaction, another refresher can already
    /// have appended an identical prefix. Existing records must therefore
    /// match byte-independent durable content exactly; only the contiguous
    /// missing suffix is appended. This deliberately does not weaken
    /// [`Self::append_durable_records_batch`], which remains the strict primary
    /// append interface.
    pub(crate) fn reconcile_replica_durable_records_batch(
        &self,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let conn = self.acquire_writer_connection()?;
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_sqlite_error)?;
        #[cfg(test)]
        observe_sqlite_cached_statement(
            &self.path,
            SqliteWriteStatementConcept::JournalNextSequenceRead,
        );
        let mut next = latest_sequence_in_conn(&conn)?.0.saturating_add(1);
        let mut appended = false;
        for record in records {
            if record.sequence.0 < next {
                #[cfg(test)]
                observe_sqlite_cached_statement(
                    &self.path,
                    SqliteWriteStatementConcept::DurableRecordRead,
                );
                let payload = conn
                    .prepare_cached("SELECT record_blob FROM commit_log WHERE sequence = ?1")
                    .map_err(map_sqlite_error)?
                    .query_row(params![record.sequence.0], |row| row.get::<_, Vec<u8>>(0))
                    .optional()
                    .map_err(map_sqlite_error)?;
                let durable = payload
                    .as_deref()
                    .map(deserialize_tenant_event_record)
                    .transpose()?;
                crate::commit_log::ensure_applied_record_matches(record, durable.as_ref())?;
                continue;
            }
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "replica journal reconciliation expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            #[cfg(test)]
            observe_sqlite_cached_statement(&self.path, SqliteWriteStatementConcept::JournalInsert);
            cached_execute(
                &conn,
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                params![record.sequence.0, serialize_tenant_event_record(record)?],
            )?;
            next = next.saturating_add(1);
            appended = true;
        }
        if appended {
            #[cfg(test)]
            observe_sqlite_cached_statement(
                &self.path,
                SqliteWriteStatementConcept::NextSequenceWrite,
            );
            put_metadata_in_conn(&conn, NEXT_SEQUENCE_KEY, &encode_u64(next))?;
            self.fault_injector
                .check_durable_records(FaultPoint::JournalAppendBeforeDurableFlush, records)?;
        }
        #[cfg(any(test, feature = "test-hooks"))]
        let commit_started = std::time::Instant::now();
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        #[cfg(any(test, feature = "test-hooks"))]
        observe_sqlite_foreground_commit(&self.path, &conn, commit_started.elapsed());
        self.release_writer_connection(conn);
        if appended {
            self.fault_injector
                .check_durable_records(FaultPoint::JournalFlushBeforeVisibility, records)?;
        }
        Ok(())
    }
}

/// Rebuilds every index from the schema already stored in `conn`. Runs after a
/// libsql replica re-sync; no embedded SQLite path needs it, so unlike the
/// reconciliation above it has no test-build consumer to keep it alive.
#[cfg(feature = "libsql")]
pub(crate) fn rebuild_sqlite_indexes_from_loaded_schema(conn: &Connection) -> Result<()> {
    let schema = load_schema_from_conn(conn)?;
    for table_schema in schema.tables.values() {
        create_sqlite_indexes_for_table_schema(conn, table_schema)?;
    }
    Ok(())
}
