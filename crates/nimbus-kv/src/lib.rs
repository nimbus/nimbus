//! RESP-native Nimbus KV listener foundation.
//!
//! The listener is authenticated and tenant-bound. String storage routes through
//! the tenant-aware storage tiering seam.

mod metrics;
mod server;
mod store;

pub use metrics::{CommandMetricsSnapshot, NimbusKvMetrics, NimbusKvMetricsSnapshot};
pub use server::{
    CredentialBinding, CredentialRegistry, DevCredential, KvError, NimbusKvConfig, run_listener,
    serve,
};
pub use store::{NimbusKvStore, TieringConfig, TieringMode};
