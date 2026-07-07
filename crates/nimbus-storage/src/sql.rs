//! Dialect-shared SQL backend helpers.
//!
//! The MySQL and PostgreSQL providers are near-identical apart from SQL dialect.
//! This module owns the pieces that are genuinely dialect-independent so they
//! live once instead of being copied per backend: row serialization ([`row`])
//! and the transaction-orchestration methods shared through the
//! `write_core::SqlWriteBackend` seam. Dialect-load-bearing logic (placeholder
//! style, lock/retry order, notifications) stays in each backend's own
//! `write.rs`.

pub(crate) mod row;
pub(crate) mod write_core;
