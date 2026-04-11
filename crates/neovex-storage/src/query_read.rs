use neovex_core::{Document, DocumentId, Filter, Result, TableName};
use serde_json::Value;

use crate::{
    MySqlReadSnapshot, MySqlTenantStore, PostgresReadSnapshot, PostgresTenantStore,
    SqliteReadSnapshot, SqliteTenantStore, TenantReadSnapshot, TenantStore,
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

    #[allow(clippy::too_many_arguments)]
    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;

    #[allow(clippy::too_many_arguments)]
    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>>;
}

impl QueryReadStore for TenantStore {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantStore::get(self, table, id)
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
        TenantStore::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantStore::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantStore::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantStore::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantStore::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for TenantReadSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantReadSnapshot::get(self, table, id)
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
        TenantReadSnapshot::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantReadSnapshot::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantReadSnapshot::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantReadSnapshot::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        TenantReadSnapshot::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for SqliteTenantStore {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        SqliteTenantStore::get(self, table, id)
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
        SqliteTenantStore::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteTenantStore::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteTenantStore::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteTenantStore::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteTenantStore::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for SqliteReadSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        SqliteReadSnapshot::get(self, table, id)
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
        SqliteReadSnapshot::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteReadSnapshot::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteReadSnapshot::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteReadSnapshot::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        SqliteReadSnapshot::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for PostgresTenantStore {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        PostgresTenantStore::get(self, table, id)
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
        PostgresTenantStore::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresTenantStore::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresTenantStore::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresTenantStore::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresTenantStore::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for PostgresReadSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        PostgresReadSnapshot::get(self, table, id)
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
        PostgresReadSnapshot::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresReadSnapshot::index_scan_eq_cancellable(
            self,
            table,
            index_name,
            value,
            check_cancel,
        )
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresReadSnapshot::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresReadSnapshot::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        PostgresReadSnapshot::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for MySqlTenantStore {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        MySqlTenantStore::get(self, table, id)
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
        MySqlTenantStore::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlTenantStore::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlTenantStore::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlTenantStore::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlTenantStore::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}

impl QueryReadStore for MySqlReadSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        MySqlReadSnapshot::get(self, table, id)
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
        MySqlReadSnapshot::scan_table_matching_with_filters_cancellable(
            self,
            table,
            filters,
            check_cancel,
            include_document,
        )
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlReadSnapshot::index_scan_eq_cancellable(self, table, index_name, value, check_cancel)
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlReadSnapshot::index_scan_prefix_cancellable(
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
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlReadSnapshot::index_scan_range_cancellable(
            self,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        MySqlReadSnapshot::index_scan_composite_range_cancellable(
            self,
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }
}
