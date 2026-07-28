//! Batch-scoped cache of apply invariants.
//!
//! One durable apply transaction re-validates the same invariants for every
//! record: the document-version storage format, the index-version storage
//! format, the table schema used for index planning, and the
//! `(table, table_id)` catalog identity. Those answers cannot change between
//! records unless an event in the same batch changes them, so the batch apply
//! context answers each distinct question once per transaction and re-asks
//! only after a schema, index, or table-lifecycle event invalidates it at
//! that record's sequence boundary.
//!
//! The context deliberately caches nothing per document: every write still
//! performs its own preimage read, integrity comparison, version rows, index
//! effects, and resource-binding effects.

use std::collections::{HashMap, HashSet};

use super::backend::ensure_table_id_in_conn;
#[cfg(test)]
use super::config::{
    SqliteWriteStatementConcept, observe_sqlite_cached_statement, observe_sqlite_format_check,
    observe_sqlite_schema_check, observe_sqlite_table_identity_check,
};
use super::document_versions::ensure_document_version_storage_format_in_conn;
use super::index_versions::ensure_index_version_storage_format_in_conn;
use super::schema::load_table_schema_from_conn;
use super::*;

/// Caches apply-invariant answers for one write transaction.
///
/// Owned by exactly one `BEGIN IMMEDIATE`..`COMMIT` scope: the queued
/// apply-batch transaction, or one direct/execution-unit prepared-write
/// transaction. Never reuse a context across transactions or connections.
#[derive(Default)]
pub(super) struct SqliteBatchApplyContext {
    document_format_checked: bool,
    index_format_checked: bool,
    schema_plans: HashMap<TableName, Option<TableSchema>>,
    verified_identities: HashSet<(TableName, TableId)>,
}

impl SqliteBatchApplyContext {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Drops every cached schema plan and verified identity.
    ///
    /// Called after applying any schema, index, or table-lifecycle event so a
    /// later record in the same batch re-reads state as of its own sequence.
    /// Storage-format checks are not invalidated: no tenant event mutates the
    /// version storage formats inside a batch.
    pub(super) fn invalidate_table_invariants(&mut self) {
        self.schema_plans.clear();
        self.verified_identities.clear();
    }

    pub(super) fn ensure_document_format(
        &mut self,
        conn: &Connection,
        #[cfg(test)] observation_path: &Path,
    ) -> Result<()> {
        if self.document_format_checked {
            return Ok(());
        }
        #[cfg(test)]
        observe_sqlite_format_check(observation_path);
        ensure_document_version_storage_format_in_conn(
            conn,
            #[cfg(test)]
            observation_path,
        )?;
        self.document_format_checked = true;
        Ok(())
    }

    pub(super) fn ensure_index_format(
        &mut self,
        conn: &Connection,
        #[cfg(test)] observation_path: &Path,
    ) -> Result<()> {
        if self.index_format_checked {
            return Ok(());
        }
        ensure_index_version_storage_format_in_conn(
            conn,
            #[cfg(test)]
            observation_path,
        )?;
        self.index_format_checked = true;
        Ok(())
    }

    /// Returns the table's schema plan, loading it once per distinct table
    /// since the last invalidation.
    pub(super) fn table_schema(
        &mut self,
        conn: &Connection,
        table: &TableName,
        #[cfg(test)] observation_path: &Path,
    ) -> Result<Option<&TableSchema>> {
        if !self.schema_plans.contains_key(table) {
            #[cfg(test)]
            {
                observe_sqlite_schema_check(observation_path);
                observe_sqlite_cached_statement(
                    observation_path,
                    SqliteWriteStatementConcept::IndexSchemaRead,
                );
            }
            let schema = load_table_schema_from_conn(conn, table)?;
            self.schema_plans.insert(table.clone(), schema);
        }
        Ok(self
            .schema_plans
            .get(table)
            .expect("schema plan inserted above")
            .as_ref())
    }

    /// Validates the `(table, table_id)` catalog identity once per distinct
    /// key since the last invalidation.
    pub(super) fn ensure_table_identity(
        &mut self,
        conn: &Connection,
        table: &TableName,
        table_id: &TableId,
        #[cfg(test)] observation_path: &Path,
    ) -> Result<()> {
        let key = (table.clone(), table_id.clone());
        if self.verified_identities.contains(&key) {
            return Ok(());
        }
        #[cfg(test)]
        {
            observe_sqlite_table_identity_check(observation_path);
            observe_sqlite_cached_statement(
                observation_path,
                SqliteWriteStatementConcept::TableIdentityCheck,
            );
        }
        ensure_table_id_in_conn(conn, table, table_id)?;
        self.verified_identities.insert(key);
        Ok(())
    }
}
