use std::collections::BTreeMap;

use nimbus_core::{Error, Result, TenantId};
use s3s::auth::{S3Auth, SecretKey};
use s3s::{S3Result, s3_error};

pub const S3_ACCESS_KEY_SPEC: &str = "ACCESS_KEY_ID:SECRET:TENANT";
const RESERVED_TENANT_PREFIX: &str = "_nimbus";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub tenant: TenantId,
    pub secret: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessKeyRegistry {
    bindings: BTreeMap<String, KeyBinding>,
}

impl AccessKeyRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn bind_signed(
        mut self,
        access_key_id: impl Into<String>,
        tenant: TenantId,
        secret: impl Into<String>,
    ) -> Self {
        self.bindings.insert(
            access_key_id.into(),
            KeyBinding {
                tenant,
                secret: secret.into(),
            },
        );
        self
    }

    pub fn from_operator_spec(raw: &str) -> Result<Self> {
        let mut registry = Self::new();
        for binding in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let (access_key_id, secret, tenant) = parse_binding(binding)?;
            registry = registry.bind_signed(access_key_id, tenant, secret);
        }
        Ok(registry)
    }

    pub fn binding(&self, access_key_id: &str) -> S3Result<&KeyBinding> {
        self.bindings
            .get(access_key_id)
            .filter(|binding| !is_reserved_tenant(&binding.tenant))
            .ok_or_else(|| s3_error!(InvalidAccessKeyId))
    }

    pub fn tenant(&self, access_key_id: &str) -> S3Result<TenantId> {
        self.binding(access_key_id)
            .map(|binding| binding.tenant.clone())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
}

#[async_trait::async_trait]
impl S3Auth for AccessKeyRegistry {
    async fn get_secret_key(&self, access_key: &str) -> S3Result<SecretKey> {
        Ok(SecretKey::from(self.binding(access_key)?.secret.clone()))
    }
}

fn parse_binding(binding: &str) -> Result<(String, String, TenantId)> {
    let mut parts = binding.splitn(3, ':');
    let (Some(access_key_id), Some(secret), Some(tenant)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::InvalidInput(format!(
            "invalid S3 access-key binding `{binding}`: expected {S3_ACCESS_KEY_SPEC}"
        )));
    };
    if access_key_id.is_empty() || secret.is_empty() || tenant.is_empty() {
        return Err(Error::InvalidInput(format!(
            "invalid S3 access-key binding `{binding}`: every segment must be non-empty"
        )));
    }
    let tenant = TenantId::new(tenant)?;
    if is_reserved_tenant(&tenant) {
        return Err(Error::InvalidInput(format!(
            "invalid S3 access-key binding `{binding}`: tenant `{tenant}` is reserved"
        )));
    }
    Ok((access_key_id.to_string(), secret.to_string(), tenant))
}

fn is_reserved_tenant(tenant: &TenantId) -> bool {
    tenant.as_str().starts_with(RESERVED_TENANT_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(value: &str) -> TenantId {
        TenantId::new(value).expect("tenant id")
    }

    #[test]
    fn operator_spec_builds_s3_only_registry() {
        let registry = AccessKeyRegistry::from_operator_spec("AKIAONE:s1:alpha, AKIATWO:s2:beta")
            .expect("spec should parse");
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.tenant("AKIAONE").unwrap(), tenant("alpha"));
        assert_eq!(registry.binding("AKIATWO").unwrap().secret, "s2");
    }

    #[test]
    fn malformed_operator_spec_is_a_boot_error() {
        let error = AccessKeyRegistry::from_operator_spec("only-two:parts")
            .expect_err("malformed binding should fail");
        assert!(error.to_string().contains(S3_ACCESS_KEY_SPEC));
    }

    #[test]
    fn reserved_tenant_is_rejected() {
        let error = AccessKeyRegistry::from_operator_spec("AKIA:secret:_nimbus_internal")
            .expect_err("reserved tenant should fail");
        assert!(error.to_string().contains("reserved"));
    }
}
