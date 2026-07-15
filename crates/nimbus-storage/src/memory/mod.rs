//! Deterministic, process-local tenant persistence for tests and simulations.
//!
//! This backend deliberately has no filesystem or database configuration
//! surface. Production provider selection remains limited to the retained
//! durable providers; engine tests opt into this store through a dedicated
//! construction path.

mod documents;
mod journal;
mod provider;
mod resources;
mod scans;
mod scheduler;
mod schema;
mod state;
mod store;
mod triggers;

pub use provider::{MemoryTenantProvider, MemoryTenantStorage, OpenedMemoryTenant};
pub use scans::MemoryTenantSnapshot;
pub use store::{MemoryTenantStore, MemoryWriteTransaction};
