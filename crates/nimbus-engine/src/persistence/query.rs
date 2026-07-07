use nimbus_core::{Document, DocumentId, Result, TableName};
use nimbus_storage::{IndexRangeBound, TenantPointRead, TenantRangeScan};

use super::{TenantPersistence, TenantPersistenceSnapshot};

// `QueryReadStore` (and, transitively, `ReadCapabilities`) are blanket-impl'd
// for any type implementing `TenantPointRead` + `TenantRangeScan` (see
// `nimbus_storage::query_read`), so implementing those two capability traits
// here is sufficient — no separate `QueryReadStore` impl is needed.

impl TenantPointRead for TenantPersistence {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistence::get(self, table, id)
    }
}

impl TenantRangeScan for TenantPersistence {
    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[nimbus_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match_tenant_persistence!(self, |store| {
            store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
        })
    }

    fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)
        })
    }

    fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.scan_table_id_starting_at_cancellable(table, start_id, limit, check_cancel)
        })
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
        })
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
        })
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.index_scan_range_cancellable(table, index_name, start, end, check_cancel)
        })
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence!(self, |store| {
            store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                check_cancel,
            )
        })
    }
}

impl TenantPointRead for TenantPersistenceSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistenceSnapshot::get(self, table, id)
    }
}

impl TenantRangeScan for TenantPersistenceSnapshot {
    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[nimbus_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
        })
    }

    fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)
        })
    }

    fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.scan_table_id_starting_at_cancellable(table, start_id, limit, check_cancel)
        })
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
        })
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
        })
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.index_scan_range_cancellable(table, index_name, start, end, check_cancel)
        })
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match_tenant_persistence_snapshot!(self, |snapshot| {
            snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                check_cancel,
            )
        })
    }
}
