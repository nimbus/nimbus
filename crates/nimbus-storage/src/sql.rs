//! Dialect-shared SQL backend helpers.
//!
//! The MySQL and PostgreSQL providers are near-identical apart from SQL dialect.
//! This module owns the pieces that are genuinely dialect-independent so they
//! live once instead of being copied per backend: row serialization ([`row`]),
//! document predicates ([`predicate`]), the materialized read snapshot
//! ([`read_snapshot`]), and the transaction-orchestration methods shared through
//! the `write_core::SqlWriteBackend` seam, plus the store-level wrapper layer
//! above it ([`store_core`]). Dialect-load-bearing logic (placeholder style,
//! lock/retry order, notifications) stays in each backend's own `write.rs`.

pub(crate) mod commit_effects;
pub(crate) mod index_history;
pub(crate) mod predicate;
pub(crate) mod read_snapshot;
pub(crate) mod row;
pub(crate) mod scheduler_core;
pub(crate) mod schema_events;
pub(crate) mod store_core;
pub(crate) mod write_core;
pub(crate) mod write_pipeline;
