use nimbus_core::{Document, Result, TableName};
use serde_json::Value;

use crate::store::{TenantReadSnapshot, TenantStore, map_redb_error};

use super::exact::index_scan_eq_in_read_txn;
use super::prefix::index_scan_prefix_in_read_txn;
use super::range::{index_scan_composite_range_in_read_txn, index_scan_range_in_read_txn};

impl TenantStore {
    /// Returns documents whose indexed field equals the provided value.
    pub fn index_scan_eq(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
    ) -> Result<Vec<Document>> {
        self.index_scan_eq_cancellable(table, index_name, value, &mut || Ok(()))
    }

    /// Returns documents whose indexed field equals the provided value, checking for cancellation between rows.
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_eq_in_read_txn(&read_txn, table, index_name, value, check_cancel)
    }

    /// Returns documents whose indexed field falls within the provided range.
    pub fn index_scan_range(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_range_cancellable(
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose indexed field falls within the provided range, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_range_in_read_txn(
            &read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix.
    pub fn index_scan_prefix(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(table, index_name, prefix_values, &mut || Ok(()))
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix, checking for cancellation between rows.
    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_prefix_in_read_txn(&read_txn, table, index_name, prefix_values, check_cancel)
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_composite_range_cancellable(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
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
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_composite_range_in_read_txn(
            &read_txn,
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

impl TenantReadSnapshot {
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_eq_in_read_txn(&self.read_txn, table, index_name, value, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_range_in_read_txn(
            &self.read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
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
        index_scan_prefix_in_read_txn(
            &self.read_txn,
            table,
            index_name,
            prefix_values,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
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
        index_scan_composite_range_in_read_txn(
            &self.read_txn,
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
