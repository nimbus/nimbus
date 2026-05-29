//! Access-key → Nimbus tenant resolution.
//!
//! DynamoDB has a flat account-level table namespace; Nimbus is multi-tenant.
//! This adapter binds each configured AWS access-key id to a Nimbus `TenantId`
//! at setup, so every request is scoped to exactly one tenant by its
//! credentials. This is a new pattern for Nimbus adapters (MongoDB/Firebase
//! resolve tenant from a request namespace token, not credentials), so it owns
//! its own resolution + isolation tests.
//!
//! The SigV4 parser (D0.8) extracts the access-key id from the `Authorization`
//! header; [`AccessKeyRegistry::resolve`] maps it to the tenant; strict
//! signature verification (the secret/region per key) lands in D7.
//!
//! Unknown access keys are rejected with `UnrecognizedClientException` — the
//! code real AWS / DynamoDB returns for an unrecognized access key (AWS SDKs
//! special-case it), which is the parity-correct choice over a generic
//! `AccessDeniedException`.

use std::collections::BTreeMap;

use extenddb_core::error::DynamoDbError;
use nimbus_core::TenantId;

/// Configured bindings from AWS access-key id to Nimbus tenant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessKeyRegistry {
    bindings: BTreeMap<String, TenantId>,
}

impl AccessKeyRegistry {
    /// An empty registry (no access keys configured).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind an access-key id to a tenant (builder style).
    #[must_use]
    pub fn bind(mut self, access_key_id: impl Into<String>, tenant: TenantId) -> Self {
        self.bindings.insert(access_key_id.into(), tenant);
        self
    }

    /// Resolve an access-key id to its bound tenant.
    ///
    /// # Errors
    /// `UnrecognizedClientException` if the access-key id has no binding.
    pub fn resolve(&self, access_key_id: &str) -> Result<&TenantId, DynamoDbError> {
        self.bindings.get(access_key_id).ok_or_else(|| {
            DynamoDbError::UnrecognizedClientException(
                "The security token included in the request is invalid.".to_owned(),
            )
        })
    }

    /// Whether any access keys are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Number of configured bindings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("valid tenant id")
    }

    #[test]
    fn resolves_known_access_keys_to_their_tenants() {
        let registry = AccessKeyRegistry::new()
            .bind("AKIAACME", tenant("acme"))
            .bind("AKIAGLOBEX", tenant("globex"));
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.resolve("AKIAACME").unwrap(), &tenant("acme"));
        assert_eq!(registry.resolve("AKIAGLOBEX").unwrap(), &tenant("globex"));
    }

    #[test]
    fn unknown_access_key_is_unrecognized_client() {
        let registry = AccessKeyRegistry::new().bind("AKIAACME", tenant("acme"));
        let err = registry.resolve("AKIANOPE").unwrap_err();
        assert!(matches!(err, DynamoDbError::UnrecognizedClientException(_)));
    }

    #[test]
    fn empty_registry_rejects_everything() {
        let registry = AccessKeyRegistry::new();
        assert!(registry.is_empty());
        assert!(matches!(
            registry.resolve("AKIAACME"),
            Err(DynamoDbError::UnrecognizedClientException(_))
        ));
    }

    #[test]
    fn distinct_keys_isolate_tenants() {
        // Two access keys must never resolve to the same tenant unless bound so;
        // the binding is the only tenant authority (no cross-tenant leakage).
        let registry = AccessKeyRegistry::new()
            .bind("key-a", tenant("tenant-a"))
            .bind("key-b", tenant("tenant-b"));
        assert_ne!(
            registry.resolve("key-a").unwrap(),
            registry.resolve("key-b").unwrap()
        );
    }
}
