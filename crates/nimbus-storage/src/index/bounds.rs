use nimbus_core::{IndexId, Result, TableId};
use serde_json::Value;

use crate::IndexRangeBound;
use crate::keys::prefix_end;

use super::encoding::{encode_index_tuple, encode_index_value};
use super::keyspace::index_value_prefix;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexRangeScanBounds {
    Empty,
    Bounds {
        match_prefix: Vec<u8>,
        start_key: Vec<u8>,
        end_key: Option<Vec<u8>>,
    },
}

fn empty_range_bounds() -> IndexRangeScanBounds {
    IndexRangeScanBounds::Empty
}

fn range_scan_bounds_for_match_prefix(
    match_prefix: Vec<u8>,
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<IndexRangeScanBounds> {
    let start_inclusive = matches!(&start, std::ops::Bound::Included(_));
    let end_inclusive = matches!(&end, std::ops::Bound::Included(_));
    let encoded_start = match start {
        std::ops::Bound::Included(value) | std::ops::Bound::Excluded(value) => {
            Some(encode_index_value(value)?)
        }
        std::ops::Bound::Unbounded => None,
    };
    let encoded_end = match end {
        std::ops::Bound::Included(value) | std::ops::Bound::Excluded(value) => {
            Some(encode_index_value(value)?)
        }
        std::ops::Bound::Unbounded => None,
    };

    let range_type_tag = match (encoded_start.as_deref(), encoded_end.as_deref()) {
        (Some(start), Some(end)) if start[0] != end[0] => {
            return Ok(empty_range_bounds());
        }
        (Some(start), _) => Some(start[0]),
        (_, Some(end)) => Some(end[0]),
        (None, None) => None,
    };

    let start_key = match encoded_start.as_deref() {
        Some(start) => {
            let mut start_key = match_prefix.clone();
            start_key.extend_from_slice(start);
            if start_inclusive {
                start_key
            } else {
                let Some(next_key) = prefix_end(&start_key) else {
                    return Ok(empty_range_bounds());
                };
                next_key
            }
        }
        None => match range_type_tag {
            Some(tag) => {
                let mut start_key = match_prefix.clone();
                start_key.push(tag);
                start_key
            }
            None => match_prefix.clone(),
        },
    };

    let end_key = match encoded_end.as_deref() {
        Some(end) => {
            let mut end_key = match_prefix.clone();
            end_key.extend_from_slice(end);
            if end_inclusive {
                prefix_end(&end_key)
            } else {
                Some(end_key)
            }
        }
        None => match range_type_tag {
            Some(tag) => {
                let mut type_prefix_end = match_prefix.clone();
                type_prefix_end.push(tag);
                prefix_end(&type_prefix_end)
            }
            None => prefix_end(&match_prefix),
        },
    };

    if let Some(end_key) = end_key.as_ref()
        && start_key >= *end_key
    {
        return Ok(empty_range_bounds());
    }

    Ok(IndexRangeScanBounds::Bounds {
        match_prefix,
        start_key,
        end_key,
    })
}

pub(crate) fn single_field_range_scan_bounds(
    table_id: &TableId,
    index_id: &IndexId,
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<IndexRangeScanBounds> {
    range_scan_bounds_for_match_prefix(index_value_prefix(table_id, index_id, &[]), start, end)
}

pub(crate) fn composite_range_scan_bounds(
    table_id: &TableId,
    index_id: &IndexId,
    exact_prefix: &[Value],
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<IndexRangeScanBounds> {
    let encoded_prefix = encode_index_tuple(exact_prefix)?;
    let match_prefix = index_value_prefix(table_id, index_id, &encoded_prefix);
    range_scan_bounds_for_match_prefix(match_prefix, start, end)
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use nimbus_core::{IndexId, TableId};
    use serde_json::json;

    use super::*;

    #[test]
    fn composite_range_bounds_reports_empty_as_enum() {
        let table_id = TableId::new();
        let index_id = IndexId::new();
        let lower = json!("open");
        let upper = json!(2);

        assert_eq!(
            composite_range_scan_bounds(
                &table_id,
                &index_id,
                &[],
                Bound::Included(&lower),
                Bound::Included(&upper),
            )
            .expect("bounds should compute"),
            IndexRangeScanBounds::Empty
        );
    }
}
