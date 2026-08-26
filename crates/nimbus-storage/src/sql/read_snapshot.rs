//! The point-in-time read snapshot shared by the SQL backends.
//!
//! Both SQL backends materialize a whole-tenant snapshot up front — schema,
//! journal progress, table identities, documents, resource-path bindings, and
//! scheduled execution ids — inside one consistent read transaction, then serve
//! every read from that in-memory image. Once the rows are loaded there is no
//! dialect left: the accessors here are pure Rust over `Vec<Document>` and
//! friends, which is why PostgreSQL and MySQL had byte-identical copies of them.
//!
//! `PostgresReadSnapshot` and `MySqlReadSnapshot` are aliases for this type. The
//! backends still differ in *how* they fill it (PostgreSQL uses a `read_only`
//! transaction, MySQL a `REPEATABLE READ` one), which stays in each backend's
//! `read.rs`.
//!
//! # Journal cursor floor
//!
//! MySQL's snapshot used to carry a seventh field, `journal_cursor_floor`,
//! captured inside the same `REPEATABLE READ` transaction. It fed exactly two
//! snapshot methods: `export_durable_journal_bootstrap`, and a
//! `stream_durable_journal` that had no callers anywhere in the workspace (the
//! live one is `MySqlTenantStore::stream_durable_journal`, which reads the floor
//! itself). The floor now stays with the MySQL store, which still captures it in
//! the same transaction as the snapshot via
//! `MySqlTenantStore::read_snapshot_with_journal_floor`, so MySQL keeps its
//! atomic snapshot/floor pair and PostgreSQL keeps reading the floor separately.

use nimbus_core::{
    CollectionName, Document, DocumentId, DocumentLocator, DocumentPath, Error, Filter,
    ResourcePathBinding, Result, Schema, SequenceNumber, TableId, TableName, TriggerDeliveryCursor,
};
use serde_json::Value;

use crate::IndexRangeBound;
use crate::keys::document_path_key;
use crate::sql::predicate::{
    document_matches_exact_prefix, document_matches_range_bounds, matches_filters,
    validate_index_prefix_len,
};
use crate::store::{JournalProgress, MATERIALIZED_JOURNAL_SNAPSHOT_VERSION};
use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry};

/// A fully materialized, immutable view of one tenant at a single sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlReadSnapshot {
    pub(crate) schema: Schema,
    pub(crate) progress: JournalProgress,
    pub(crate) table_identities: Vec<TableIdentitySnapshotEntry>,
    pub(crate) documents: Vec<Document>,
    pub(crate) resource_path_bindings: Vec<ResourcePathBinding>,
    pub(crate) scheduled_execution_ids: Vec<String>,
    pub(crate) trigger_delivery_cursor: TriggerDeliveryCursor,
}

impl SqlReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.schema.clone())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(self.progress)
    }

    pub fn table_identities(&self) -> Result<Vec<TableIdentitySnapshotEntry>> {
        Ok(self.table_identities.clone())
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        Ok(MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: self.progress.applied_head,
            durable_head: self.progress.durable_head,
            table_identities: self.table_identities.clone(),
            schema: self.schema.clone(),
            documents: self.documents.clone(),
            resource_path_bindings: self.resource_path_bindings.clone(),
            scheduled_execution_ids: self.scheduled_execution_ids.clone(),
            trigger_delivery_cursor: self.trigger_delivery_cursor,
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        Ok(self
            .documents
            .iter()
            .find(|document| &document.table == table && &document.id == id)
            .cloned())
    }

    pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        Ok(self
            .table_identities
            .iter()
            .find(|identity| {
                identity.namespace == crate::table_identity::DEFAULT_TABLE_NAMESPACE
                    && &identity.table == table
            })
            .map(|identity| identity.table_id.clone()))
    }

    pub fn table_identity_diagnostics(
        &self,
        backend_layout: crate::TableBackendLayout,
    ) -> Result<Vec<crate::TableIdentityDiagnostic>> {
        Ok(self
            .table_identities
            .iter()
            .map(|identity| {
                let document_count = (identity.namespace
                    == crate::table_identity::DEFAULT_TABLE_NAMESPACE)
                    .then(|| {
                        self.documents
                            .iter()
                            .filter(|document| document.table == identity.table)
                            .count() as u64
                    });
                crate::TableIdentityDiagnostic::from_snapshot_entry(
                    identity,
                    backend_layout,
                    document_count,
                )
            })
            .collect())
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
        self.scan_table_matching_with_filters_cancellable(
            table,
            &[],
            check_cancel,
            include_document,
        )
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
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if matches_filters(document, filters)? && include_document(document)? {
                documents.push(document.clone());
            }
        }
        Ok(documents)
    }

    pub fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if document.id.as_str().starts_with(id_prefix) {
                documents.push(document.clone());
            }
        }
        Ok(documents)
    }

    pub fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
            .filter(|document| document.id.as_str() >= start_id)
            .take(limit)
        {
            check_cancel()?;
            documents.push(document.clone());
        }
        Ok(documents)
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(
            table,
            index_name,
            std::slice::from_ref(value),
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        validate_index_prefix_len(index_name, prefix_values.len(), index_fields.len())?;
        self.filter_index_documents(
            table,
            &index_fields,
            prefix_values,
            std::ops::Bound::Unbounded,
            std::ops::Bound::Unbounded,
            check_cancel,
        )
    }

    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        self.filter_index_documents(table, &index_fields, &[], start, end, check_cancel)
    }

    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        if exact_prefix.len() >= index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} leaves no range field for index '{}'",
                exact_prefix.len(),
                index_name
            )));
        }
        self.filter_index_documents(table, &index_fields, exact_prefix, start, end, check_cancel)
    }

    pub fn scan_resource_path_bindings(&self) -> Result<Vec<ResourcePathBinding>> {
        let mut bindings = self.resource_path_bindings.clone();
        bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
        Ok(bindings)
    }

    pub fn resource_path_binding(
        &self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        Ok(self
            .resource_path_bindings
            .iter()
            .find(|binding| &binding.locator == locator)
            .cloned())
    }

    pub fn locator_for_document_path(
        &self,
        document_path: &DocumentPath,
    ) -> Result<Option<DocumentLocator>> {
        Ok(self
            .resource_path_bindings
            .iter()
            .find(|binding| &binding.document_path == document_path)
            .map(|binding| binding.locator.clone()))
    }

    pub fn scan_collection_group_bindings(
        &self,
        collection_group: &CollectionName,
    ) -> Result<Vec<ResourcePathBinding>> {
        let mut bindings = self
            .resource_path_bindings
            .iter()
            .filter(|binding| binding.collection_group() == collection_group)
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| document_path_key(&binding.document_path));
        Ok(bindings)
    }

    fn index_fields(&self, table: &TableName, index_name: &str) -> Result<Vec<String>> {
        let table_schema = self
            .schema
            .get_table(table)
            .ok_or_else(|| Error::SchemaNotFound(table.clone()))?;
        let index = table_schema
            .queryable_indexes()
            .find(|index| index.name == index_name)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "index '{}' not found for table '{}'",
                    index_name,
                    table.as_str()
                ))
            })?;
        Ok(index.fields.clone())
    }

    fn filter_index_documents(
        &self,
        table: &TableName,
        index_fields: &[String],
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let range_field = index_fields.get(exact_prefix.len());
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if !document_matches_exact_prefix(document, index_fields, exact_prefix) {
                continue;
            }
            if let Some(range_field) = range_field
                && !document_matches_range_bounds(document, range_field, start, end)?
            {
                continue;
            }
            documents.push(document.clone());
        }
        Ok(documents)
    }
}
