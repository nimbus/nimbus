use crate::traits::{TenantPointRead, TenantRangeScan};

/// Query planner and evaluator read surface: point document reads plus the
/// table/index range scans the query planner needs.
///
/// This is a supertrait alias over `TenantPointRead` + `TenantRangeScan` (see
/// `traits/core.rs` for the canonical method definitions and per-backend
/// impls) rather than a standalone method list, so a type only has to satisfy
/// the two core capability traits once to get `QueryReadStore` for free.
pub trait QueryReadStore: TenantPointRead + TenantRangeScan {}

impl<T> QueryReadStore for T where T: TenantPointRead + TenantRangeScan + ?Sized {}
