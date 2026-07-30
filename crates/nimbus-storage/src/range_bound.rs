use std::ops::Bound;

#[cfg(any(feature = "mysql", feature = "postgres"))]
use nimbus_core::Result;
use serde_json::Value;

pub type IndexRangeBound<'a> = Bound<&'a Value>;

// Owning and re-borrowing a range bound is only needed by the PostgreSQL and
// MySQL backends, which must hold the bound across an `async` statement
// boundary. Every other backend evaluates the bound within one borrow.
#[cfg(any(feature = "mysql", feature = "postgres"))]
#[derive(Clone)]
pub(crate) struct OwnedIndexRangeBounds {
    pub start: Bound<Value>,
    pub end: Bound<Value>,
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn clone_index_range_bound(bound: IndexRangeBound<'_>) -> Bound<Value> {
    match bound {
        Bound::Included(value) => Bound::Included(value.clone()),
        Bound::Excluded(value) => Bound::Excluded(value.clone()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn borrow_index_range_bound(bound: &Bound<Value>) -> IndexRangeBound<'_> {
    match bound {
        Bound::Included(value) => Bound::Included(value),
        Bound::Excluded(value) => Bound::Excluded(value),
        Bound::Unbounded => Bound::Unbounded,
    }
}

pub(crate) fn index_range_bound_value(bound: IndexRangeBound<'_>) -> Option<&Value> {
    match bound {
        Bound::Included(value) | Bound::Excluded(value) => Some(value),
        Bound::Unbounded => None,
    }
}

pub(crate) fn index_range_bound_is_inclusive(bound: IndexRangeBound<'_>) -> bool {
    matches!(bound, Bound::Included(_))
}

pub(crate) fn index_range_bound_presence(bound: IndexRangeBound<'_>) -> Bound<()> {
    match bound {
        Bound::Included(_) => Bound::Included(()),
        Bound::Excluded(_) => Bound::Excluded(()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn map_owned_index_range_bound<T>(
    bound: Bound<Value>,
    convert: impl FnOnce(&Value) -> Result<T>,
) -> Result<Bound<T>> {
    match bound {
        Bound::Included(value) => Ok(Bound::Included(convert(&value)?)),
        Bound::Excluded(value) => Ok(Bound::Excluded(convert(&value)?)),
        Bound::Unbounded => Ok(Bound::Unbounded),
    }
}
