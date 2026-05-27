use nimbus_core::{Document, Result, TableName};
use redb::ReadTransaction;
use serde_json::Value;

use crate::keys::prefix_end;

use super::super::encoding::encode_index_value;
use super::super::keyspace::index_value_prefix;
use super::read::{
    resolve_queryable_index_in_read_txn, scan_documents_for_index_key_bounds_in_read_txn,
};

pub(super) fn index_scan_eq_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    value: &Value,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let Some((table_id, index)) = resolve_queryable_index_in_read_txn(read_txn, table, index_name)?
    else {
        return Ok(Vec::new());
    };
    let encoded = encode_index_value(value)?;
    let match_prefix = index_value_prefix(&table_id, &index.id, &encoded);
    let end_key = prefix_end(&match_prefix);
    scan_documents_for_index_key_bounds_in_read_txn(
        read_txn,
        &table_id,
        &match_prefix,
        &match_prefix,
        end_key.as_deref(),
        check_cancel,
    )
}
