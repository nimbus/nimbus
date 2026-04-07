use neovex_core::{Document, Result, TableName};
use redb::ReadTransaction;
use serde_json::Value;

use crate::keys::prefix_end;

use super::super::encoding::encode_index_tuple;
use super::super::keyspace::index_value_prefix;
use super::read::scan_documents_for_index_key_bounds_in_read_txn;

pub(super) fn index_scan_prefix_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    prefix_values: &[Value],
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let encoded_prefix = encode_index_tuple(prefix_values)?;
    let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
    let end_key = prefix_end(&match_prefix);
    scan_documents_for_index_key_bounds_in_read_txn(
        read_txn,
        table,
        &match_prefix,
        &match_prefix,
        end_key.as_deref(),
        check_cancel,
    )
}
