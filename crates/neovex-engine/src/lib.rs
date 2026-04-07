//! Neovex engine crate.

mod evaluator;
mod replica;
pub mod scheduler;
mod service;
mod subscriptions;
mod tenant;
mod verification;

pub use evaluator::{
    encode_cursor, evaluate_paginated, evaluate_paginated_with_docs, evaluate_query,
    evaluate_query_with_docs,
};
pub use neovex_storage::MonthlyActiveUsersSnapshot;
pub use neovex_storage::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    MaterializedJournalSnapshot, ShadowMaterializer, ShadowMaterializerConfig,
    ShadowMaterializerManifest,
};
pub use replica::EmbeddedReplica;
pub use scheduler::run_scheduler;
pub use service::{MutationExecutionUnit, Service, SubscriptionBootstrapCancellation};
pub use subscriptions::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionCleanupHandle, SubscriptionRegistration,
    SubscriptionUpdate,
};
pub use tenant::{
    MaterializedReadSurfaceStats, MutationAdmissionPhase, MutationAdmissionStats,
    MutationJournalStats, QueryPlanningStats, ServingSnapshotManagerStats,
    SubscriptionDeliveryStats, TenantEngineDiagnosticsSnapshot,
};
pub use verification::{
    BootstrapFingerprint, ConsistencyMismatch, ConsistencyScope, ConsistencyVerificationReport,
    SnapshotFingerprint,
};

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
