//! Cloudflare-compatible adapter configuration.
//!
//! This module owns the transport-free binding/config surface (CP2). The
//! transport-facing `Router` construction and per-request handlers stay in
//! `nimbus-server`'s `adapters::cloudflare` module, which re-exports these
//! types.

use nimbus_core::TenantId;
use nimbus_dynamodb::AccessKeyRegistry;

mod wrangler;

pub use wrangler::{
    CloudflareBindingRegistry, D1DatabaseBinding, DurableObjectBinding, KvNamespaceBinding,
    R2BucketBinding, WranglerConfigError,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudflareConfig {
    bindings: CloudflareBindingRegistry,
    access_keys: AccessKeyRegistry,
}

impl CloudflareConfig {
    pub fn new(bindings: CloudflareBindingRegistry) -> Self {
        Self {
            bindings,
            access_keys: AccessKeyRegistry::new(),
        }
    }

    pub fn bindings(&self) -> &CloudflareBindingRegistry {
        &self.bindings
    }

    pub fn access_keys(&self) -> &AccessKeyRegistry {
        &self.access_keys
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
        self.access_keys =
            self.access_keys
                .bind_signed(access_key_id.into(), tenant, secret.into());
        self
    }
}
