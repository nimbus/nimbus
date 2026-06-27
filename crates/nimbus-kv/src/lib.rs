//! RESP-native Nimbus KV listener foundation.
//!
//! F1 intentionally owns only the authenticated wire listener and a minimal
//! command surface. Durable storage, cache/tiering, and conformance expansion
//! land in later NKV0 bands.

mod server;

pub use server::{
    CredentialBinding, CredentialRegistry, DevCredential, KvError, NimbusKvConfig, run_listener,
    serve,
};
