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
//!
//! # Feature gating
//!
//! The whole module compiles only when at least one remote provider feature is
//! on (see the `cfg` on `mod sql` in `lib.rs`). Within it the members fall into
//! two tiers. The store/transaction seam -- `commit_effects`, `store_core`,
//! `write_core` -- is shared by all three remote providers including libsql, so
//! it needs no further gate at module level. Everything else is consumed only
//! by PostgreSQL and MySQL: libsql's replica reads go through its local SQLite
//! cache, and its writes go to the remote primary rather than through the
//! journal write pipeline. Those members carry an explicit
//! `postgres`-or-`mysql` gate so a libsql-only build does not compile them as
//! dead code, which `-D warnings` would reject.
//!
//! `store_core` and `write_core` straddle the split: libsql uses the
//! transaction seam and the store facade, but not the durable-journal or
//! blocking-read halves. Those items carry the same `postgres`-or-`mysql` gate
//! inline.

pub(crate) mod commit_effects;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod index_history;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod predicate;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod read_snapshot;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod row;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod scheduler_core;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod schema_events;
pub(crate) mod store_core;
pub(crate) mod write_core;
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) mod write_pipeline;
