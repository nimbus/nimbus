//! Shared runtime read-tracking primitives that are provider-neutral.
//!
//! This module owns the canonical read-set model, intersection logic, and
//! subscription-base-query synthesis. Adapter-specific runtime shims should
//! record reads through these primitives instead of reimplementing commit or
//! intersection semantics.

mod intersection;
mod read_set;
mod subscriptions;
#[cfg(test)]
mod tests;

pub(crate) use intersection::commit_intersects_runtime_read_set;
pub(crate) use read_set::{RuntimeIndexRangeRead, RuntimeReadSet};
pub(crate) use subscriptions::synthesize_runtime_subscription_base_queries;
