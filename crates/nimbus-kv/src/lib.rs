//! RESP-native Nimbus KV listener foundation.
//!
//! The listener is authenticated and tenant-bound. String storage routes through
//! the tenant-aware storage tiering seam.

mod listener;
mod metrics;
mod server;
mod store;

pub use listener::{NimbusKvListener, NimbusKvListenerConfig};
pub use metrics::{CommandMetricsSnapshot, NimbusKvMetrics, NimbusKvMetricsSnapshot};
pub use server::{
    CredentialBinding, CredentialRegistry, DevCredential, KvError, NimbusKvConfig, adopt_listener,
    bind_listener, run_listener, serve, serve_listener,
};
pub use store::{NimbusKvStore, TieringConfig, TieringMode};
