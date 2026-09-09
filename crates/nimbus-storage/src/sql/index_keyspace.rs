//! Dialect-neutral planning for index reads over a current-state keyspace.
//!
//! A SQL backend that stores one encoded tuple per maintained index and
//! document answers an index read with one byte-range over that tuple column.
//! The range comes from the same order-preserving encoding that the embedded
//! key-value index uses, so equality, prefix, and range scans share one
//! planner across every backend and the storage dialect only supplies the
//! `>=` / `<` comparison and the row shape. The Rust predicate in
//! [`crate::sql::predicate`] stays the authority on membership; the bounds
//! here only select candidates.

use nimbus_core::Result;
use serde_json::Value;

use crate::IndexRangeBound;
use crate::index::{IndexRangeScanBounds, encode_index_tuple, range_scan_bounds_for_match_prefix};

/// The byte range over a keyspace's `encoded_tuple` column that holds every
/// candidate for one index read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexTupleScanBounds {
    /// The read cannot match any tuple, so the backend skips the query.
    Empty,
    /// Candidates are the rows with `start_key <= encoded_tuple` and, when an
    /// end key exists, `encoded_tuple < end_key`.
    Bounds {
        start_key: Vec<u8>,
        end_key: Option<Vec<u8>>,
    },
}

impl IndexTupleScanBounds {
    /// Plans the tuple range for an index read whose leading fields equal
    /// `exact_prefix` and whose next field, when a bound is given, falls in
    /// `start..end`.
    pub(crate) fn for_scan(
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
    ) -> Result<Self> {
        let match_prefix = encode_index_tuple(exact_prefix)?;
        Ok(
            match range_scan_bounds_for_match_prefix(match_prefix, start, end)? {
                IndexRangeScanBounds::Empty => Self::Empty,
                IndexRangeScanBounds::Bounds {
                    start_key, end_key, ..
                } => Self::Bounds { start_key, end_key },
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use serde_json::json;

    use super::*;
    use crate::index::encode_index_value;

    #[test]
    fn unbounded_scan_without_prefix_covers_the_whole_index() {
        assert_eq!(
            IndexTupleScanBounds::for_scan(&[], Bound::Unbounded, Bound::Unbounded)
                .expect("bounds should plan"),
            IndexTupleScanBounds::Bounds {
                start_key: Vec::new(),
                end_key: None,
            }
        );
    }

    #[test]
    fn exact_prefix_scan_is_the_prefix_and_its_successor() {
        let prefix = [json!("ops")];
        let encoded = encode_index_tuple(&prefix).expect("prefix should encode");
        assert_eq!(
            IndexTupleScanBounds::for_scan(&prefix, Bound::Unbounded, Bound::Unbounded)
                .expect("bounds should plan"),
            IndexTupleScanBounds::Bounds {
                start_key: encoded.clone(),
                end_key: crate::keys::prefix_end(&encoded),
            }
        );
    }

    #[test]
    fn number_range_stays_inside_the_number_type_tag() {
        let low = json!(2);
        let high = json!(9);
        let bounds =
            IndexTupleScanBounds::for_scan(&[], Bound::Included(&low), Bound::Excluded(&high))
                .expect("bounds should plan");
        assert_eq!(
            bounds,
            IndexTupleScanBounds::Bounds {
                start_key: encode_index_value(&low).expect("low should encode"),
                end_key: Some(encode_index_value(&high).expect("high should encode")),
            }
        );
        let text = json!("z");
        assert_eq!(
            IndexTupleScanBounds::for_scan(&[], Bound::Included(&low), Bound::Included(&text))
                .expect("bounds should plan"),
            IndexTupleScanBounds::Empty
        );
    }

    #[test]
    fn inverted_range_is_empty() {
        let low = json!(2);
        let high = json!(9);
        assert_eq!(
            IndexTupleScanBounds::for_scan(&[], Bound::Included(&high), Bound::Included(&low))
                .expect("bounds should plan"),
            IndexTupleScanBounds::Empty
        );
    }
}
