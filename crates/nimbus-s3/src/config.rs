use std::fmt::{Debug, Formatter};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use nimbus_core::TenantId;

use crate::AccessKeyRegistry;

pub const DEFAULT_S3_PORT: u16 = 9000;

#[derive(Clone, PartialEq, Eq)]
pub struct S3Config {
    pub bind_addr: SocketAddr,
    pub access_keys: AccessKeyRegistry,
    pub convex_download_secret: Option<Vec<u8>>,
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
    pub fn with_signed_access_key(
        mut self,
        access_key_id: impl Into<String>,
        tenant: TenantId,
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
