use nimbus_core::{Document, Result, TableName};
use redb::ReadTransaction;
use serde_json::Value;

use crate::IndexRangeBound;

use super::super::bounds::{
    IndexRangeScanBounds, composite_range_scan_bounds, single_field_range_scan_bounds,
};
use super::read::{
    resolve_queryable_index_in_read_txn, scan_documents_for_index_key_bounds_in_read_txn,
};

pub(super) fn index_scan_range_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let Some((table_id, index)) = resolve_queryable_index_in_read_txn(read_txn, table, index_name)?
    else {
        return Ok(Vec::new());
    };
    let IndexRangeScanBounds::Bounds {
        match_prefix,
        start_key,
        end_key,
    } = single_field_range_scan_bounds(&table_id, &index.id, start, end)?
    else {
        return Ok(Vec::new());
    };

    scan_documents_for_index_key_bounds_in_read_txn(
        read_txn,
        &table_id,
        &match_prefix,
        &start_key,
        end_key.as_deref(),
        check_cancel,
    )
}

pub(super) fn index_scan_composite_range_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    exact_prefix: &[Value],
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let Some((table_id, index)) = resolve_queryable_index_in_read_txn(read_txn, table, index_name)?
    else {
        return Ok(Vec::new());
    };
    let IndexRangeScanBounds::Bounds {
        match_prefix,
        start_key,
        end_key,
    } = composite_range_scan_bounds(&table_id, &index.id, exact_prefix, start, end)?
    else {
        return Ok(Vec::new());
    };

    scan_documents_for_index_key_bounds_in_read_txn(
        read_txn,
        &table_id,
        &match_prefix,
        &start_key,
        end_key.as_deref(),
        check_cancel,
    )
}
