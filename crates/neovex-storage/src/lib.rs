//! Storage layer backed by redb.

pub mod async_storage;
pub mod commit_log;
pub mod index;
pub mod keys;
pub mod materializer;
pub mod scheduler;
pub mod schema_store;
pub mod simulation;
pub mod store;
pub mod usage_store;

pub use async_storage::{
    OpenedRedbTenant, RedbStorageEngine, RedbTenantStorage, RedbUsageStorage, StorageEngine,
    TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, UsageStorage,
};
pub use materializer::{ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest};
pub use simulation::{
    Clock, FaultInjector, FaultOccurrence, FaultPoint, ManualClock, NoopFaultInjector,
    ScriptedFaultInjector, SeededFaultInjector, SystemClock,
};
pub use store::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    JournalProgress, MAX_DURABLE_JOURNAL_STREAM_LIMIT, MaterializedJournalSnapshot,
    ResolvedScheduleOp, ResolvedWrite, TenantReadSnapshot, TenantStore, TenantWriteCommit,
    TenantWriteTransaction,
};
pub use usage_store::{MonthlyActiveUsersSnapshot, UsageStore};

#[cfg(test)]
mod tests;
