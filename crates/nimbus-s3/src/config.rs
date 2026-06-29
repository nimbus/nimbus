use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use nimbus_core::TenantId;

use crate::AccessKeyRegistry;

pub const DEFAULT_S3_PORT: u16 = 9000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct S3Config {
    pub bind_addr: SocketAddr,
    pub access_keys: AccessKeyRegistry,
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
