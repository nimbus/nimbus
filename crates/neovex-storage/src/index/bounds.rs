use neovex_core::{Result, TableName};
use serde_json::Value;

use crate::keys::prefix_end;

use super::encoding::{encode_index_tuple, encode_index_value};
use super::keyspace::index_value_prefix;

pub(super) type CompositeRangeScanBounds = (Vec<u8>, Vec<u8>, Option<Vec<u8>>);

pub(super) fn composite_range_scan_bounds(
    table: &TableName,
    index_name: &str,
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<CompositeRangeScanBounds> {
    let encoded_prefix = encode_index_tuple(exact_prefix)?;
    let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
    let start_key = if let Some(start) = start {
        let mut start_key = match_prefix.clone();
        start_key.extend_from_slice(&encode_index_value(start)?);
        if start_inclusive {
            start_key
        } else {
            let Some(next_key) = prefix_end(&start_key) else {
                return Ok((match_prefix, Vec::new(), Some(Vec::new())));
            };
            next_key
        }
    } else {
        match_prefix.clone()
    };
    let end_key = if let Some(end) = end {
        let mut end_key = match_prefix.clone();
        end_key.extend_from_slice(&encode_index_value(end)?);
        if end_inclusive {
            prefix_end(&end_key)
        } else {
            Some(end_key)
        }
    } else {
        prefix_end(&match_prefix)
    };

    Ok((match_prefix, start_key, end_key))
}
