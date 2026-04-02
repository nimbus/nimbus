//! Neovex engine crate.

mod evaluator;
mod replica;
pub mod scheduler;
mod service;
mod subscriptions;
mod tenant;

pub use evaluator::{
    evaluate_paginated, evaluate_paginated_with_docs, evaluate_query, evaluate_query_with_docs,
};
pub use neovex_storage::MonthlyActiveUsersSnapshot;
pub use neovex_storage::{
    DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT, DurableJournalBootstrap, DurableJournalPage,
    MaterializedJournalSnapshot, ShadowMaterializer, ShadowMaterializerConfig,
    ShadowMaterializerManifest,
};
pub use replica::EmbeddedReplica;
pub use scheduler::run_scheduler;
pub use service::{MutationExecutionUnit, Service};
pub use subscriptions::{SubscriptionCleanupHandle, SubscriptionRegistration, SubscriptionUpdate};

#[cfg(test)]
mod tests;
