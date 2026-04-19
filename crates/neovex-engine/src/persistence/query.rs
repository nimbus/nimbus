use neovex_core::{Document, DocumentId, Result, TableName};
use neovex_storage::QueryReadStore;

use super::{TenantPersistence, TenantPersistenceSnapshot};

impl QueryReadStore for TenantPersistence {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistence::get(self, table, id)
    }

    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Sqlite(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::LibsqlReplica(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Postgres(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::MySql(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
        }
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Sqlite(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::LibsqlReplica(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Postgres(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::MySql(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
        }
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::Sqlite(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::LibsqlReplica(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::Postgres(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::MySql(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
        }
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::LibsqlReplica(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Postgres(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::LibsqlReplica(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Postgres(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }
}

impl QueryReadStore for TenantPersistenceSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistenceSnapshot::get(self, table, id)
    }

    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    filters,
                    check_cancel,
                    include_document,
                ),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    filters,
                    check_cancel,
                    include_document,
                ),
            Self::Postgres(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::MySql(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
        }
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_eq_cancellable(table, index_name, value, check_cancel),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_eq_cancellable(table, index_name, value, check_cancel),
            Self::Postgres(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::MySql(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
        }
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel),
            Self::Postgres(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
        }
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_range_cancellable(
                    table,
                    index_name,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_range_cancellable(
                    table,
                    index_name,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::Postgres(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_composite_range_cancellable(
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::LibsqlReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_composite_range_cancellable(
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::Postgres(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }
}
