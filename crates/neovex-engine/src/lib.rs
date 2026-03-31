//! Neovex engine crate.

mod evaluator;
pub mod scheduler;
mod service;
mod subscriptions;
mod tenant;

pub use evaluator::{
    evaluate_paginated, evaluate_paginated_with_docs, evaluate_query, evaluate_query_with_docs,
};
pub use neovex_storage::MonthlyActiveUsersSnapshot;
pub use scheduler::run_scheduler;
pub use service::Service;
pub use subscriptions::SubscriptionUpdate;

#[cfg(test)]
mod tests;
