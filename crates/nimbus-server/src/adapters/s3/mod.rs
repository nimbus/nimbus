//! Server-side composition shim for the S3 adapter.
//!
//! The `nimbus-s3` crate owns protocol behavior and public configuration. This
//! module owns the dedicated listener and the Engine-backed byte/metadata
//! backend that binds that protocol surface into the Nimbus server process.

pub mod listener;

use std::fmt::{Debug, Formatter};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_object_storage::ObjectStorageConfig;
pub use nimbus_s3::{AccessKeyRegistry, DEFAULT_S3_PORT};

use super::wire::WireProtocolAdapter;

/// Server-side S3 listener config.
///
/// `nimbus-s3` owns protocol behavior. This wrapper keeps server-owned
/// listener state and native object-storage placement resolution outside the
/// protocol crate.
#[derive(Clone, PartialEq, Eq)]
pub struct S3Config {
    pub bind_addr: SocketAddr,
    pub access_keys: AccessKeyRegistry,
    pub convex_download_secret: Option<Vec<u8>>,
    pub object_storage: ObjectStorageConfig,
}

impl Debug for S3Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("bind_addr", &self.bind_addr)
            .field("access_key_count", &self.access_keys.len())
            .field(
                "convex_download_secret",
                &self.convex_download_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("object_storage", &self.object_storage)
            .finish()
    }
}

impl S3Config {
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self::localhost(port)
    }

    #[must_use]
    pub fn localhost(port: u16) -> Self {
        Self {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            access_keys: AccessKeyRegistry::new(),
            convex_download_secret: None,
            object_storage: ObjectStorageConfig::default(),
        }
    }

    #[must_use]
    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    #[must_use]
    pub fn with_access_keys(mut self, access_keys: AccessKeyRegistry) -> Self {
        self.access_keys = access_keys;
        self
    }

    #[must_use]
    pub fn with_convex_download_secret(mut self, secret: impl Into<Vec<u8>>) -> Self {
        self.convex_download_secret = Some(secret.into());
        self
    }

    #[must_use]
    pub fn with_object_storage_config(mut self, object_storage: ObjectStorageConfig) -> Self {
        self.object_storage = object_storage;
        self
    }

    #[must_use]
    pub fn with_signed_access_key(
        mut self,
        access_key_id: impl Into<String>,
        tenant: nimbus_core::TenantId,
        secret: impl Into<String>,
    ) -> Self {
        self.access_keys = self.access_keys.bind_signed(access_key_id, tenant, secret);
        self
    }
}

impl Default for S3Config {
    fn default() -> Self {
        Self::new(DEFAULT_S3_PORT)
    }
}

impl WireProtocolAdapter for S3Config {
    fn name(&self) -> &'static str {
        "s3"
    }

    fn protocol(&self) -> &'static str {
        "http"
    }

    fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    fn guard(&self, _addr: SocketAddr) -> std::io::Result<()> {
        listener::guard_config(self)
    }

    fn spawn(
        self: Box<Self>,
        listener: tokio::net::TcpListener,
        engine: Arc<Engine>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let config = *self;
        vec![tokio::spawn(async move {
            listener::run_listener(listener, engine, config).await;
        })]
    }
}
