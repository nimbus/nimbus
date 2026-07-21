//! Durable `_nimbus.tables` projection ownership.
//!
//! The public installation seam stays deliberately small. `work` owns bounded
//! scheduling and retry, while `publication` owns the atomic durable ordering
//! contract for visible rows and private deletion tombstones.

mod config;
pub(crate) mod publication;
mod work;

pub use work::install_table_projection_observer;

#[cfg(test)]
#[path = "projection/reconciliation_tests.rs"]
mod reconciliation_tests;
