//! Async execution boundary for tenant and usage storage.
//!
//! This is the storage migration seam derived from the live engine call sites,
//! not a generic CRUD facade. The engine still depends on:
//!
//! - cancellable read execution over `Arc<TenantStore>` for planner-driven
//!   queries, journal reads, and subscription bootstrap work
//! - cancellable write execution over `TenantWriteTransaction` for schema
//!   changes, scheduler transitions, and other write-side helpers that must
//!   preserve the current pre-commit versus committed-write split
//! - direct access to the underlying `TenantStore` APIs for validated direct
//!   writes, execution-unit batch apply, journal replay, and
//!   snapshot/bootstrap flows
//!
//! During the SQLite migration this module should stay focused on that real
//! contract instead of growing a permanent generalized backend abstraction.
#![allow(async_fn_in_trait)]

mod control;
mod engine;
mod helpers;
mod read;
mod sqlite;
mod traits;
mod write;

pub use self::control::EmbeddedRedbControlPlaneProvider;
pub use self::engine::{EmbeddedProviderKind, EmbeddedRedbProvider, OpenedEmbeddedRedbTenant};
pub use self::read::{RedbTenantStorage, RedbUsageStorage};
pub use self::sqlite::{EmbeddedSqliteProvider, OpenedEmbeddedSqliteTenant, SqliteTenantStorage};
pub use self::traits::{
    EmbeddedPersistenceProvider, TenantReadStorage, TenantWriteOutcome, TenantWriteStorage,
    UsageStorage,
};
