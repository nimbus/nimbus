use nimbus_core::{Document, DocumentId, Filter, Result, TableName};
use serde_json::Value;

use crate::{
    IndexRangeBound, MySqlReadSnapshot, MySqlTenantStore, PostgresReadSnapshot,
    PostgresTenantStore, SqliteReadSnapshot, SqliteTenantStore, TenantReadSnapshot, TenantStore,
};

/// Query planner and evaluator read surface derived from live engine call sites.
///
/// This is intentionally narrow and read-only. It captures the concrete
/// operations the engine still needs while SQLite replaces redb's physical read
/// path, without projecting a permanent generalized storage abstraction.
pub trait QueryReadStore {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>>;

    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>;

    fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;
}

macro_rules! impl_query_read_store {
    ($type:ty) => {
        impl QueryReadStore for $type {
            fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
                <$type>::get(self, table, id)
            }

            fn scan_table_matching_with_filters_cancellable<F>(
                &self,
                table: &TableName,
                filters: &[Filter],
                check_cancel: &mut dyn FnMut() -> Result<()>,
                include_document: F,
            ) -> Result<Vec<Document>>
            where
                F: FnMut(&Document) -> Result<bool>,
            {
                <$type>::scan_table_matching_with_filters_cancellable(
                    self,
                    table,
                    filters,
                    check_cancel,
                    include_document,
                )
            }

            fn scan_table_id_prefix_cancellable(
                &self,
                table: &TableName,
                id_prefix: &str,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::scan_table_id_prefix_cancellable(self, table, id_prefix, check_cancel)
            }

            fn scan_table_id_starting_at_cancellable(
                &self,
                table: &TableName,
                start_id: &str,
                limit: usize,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::scan_table_id_starting_at_cancellable(
                    self,
                    table,
                    start_id,
                    limit,
                    check_cancel,
                )
            }

            fn index_scan_eq_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                value: &Value,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
            }

            fn index_scan_prefix_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                prefix_values: &[Value],
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::index_scan_prefix_cancellable(
                    self,
                    table,
                    index_name,
                    prefix_values,
                    check_cancel,
                )
            }

            fn index_scan_range_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                start: IndexRangeBound<'_>,
                end: IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::index_scan_range_cancellable(
                    self,
                    table,
                    index_name,
                    start,
                    end,
                    check_cancel,
                )
            }

            fn index_scan_composite_range_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                exact_prefix: &[Value],
                start: IndexRangeBound<'_>,
                end: IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                <$type>::index_scan_composite_range_cancellable(
                    self,
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    check_cancel,
                )
            }
        }
    };
}

impl_query_read_store!(TenantStore);
impl_query_read_store!(TenantReadSnapshot);
impl_query_read_store!(SqliteTenantStore);
impl_query_read_store!(SqliteReadSnapshot);
impl_query_read_store!(PostgresTenantStore);
impl_query_read_store!(PostgresReadSnapshot);
impl_query_read_store!(MySqlTenantStore);
impl_query_read_store!(MySqlReadSnapshot);
