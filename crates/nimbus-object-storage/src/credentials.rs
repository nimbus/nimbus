use nimbus_core::{Error, Result};

/// Resolved secret material for an object-store placement target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectStoreSecret {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl ObjectStoreSecret {
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        session_token: Option<String>,
    ) -> Result<Self> {
        let access_key_id = access_key_id.into();
        let secret_access_key = secret_access_key.into();
        if access_key_id.trim().is_empty() {
            return Err(Error::InvalidInput(
                "object-store access key id is required".to_string(),
            ));
        }
        if secret_access_key.trim().is_empty() {
            return Err(Error::InvalidInput(
                "object-store secret access key is required".to_string(),
            ));
        }
        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

/// Secret lookup seam for provider `SecretRef` credentials.
pub trait ObjectStoreCredentialResolver: Send + Sync {
    fn resolve_object_store_secret(&self, id: &str) -> Result<ObjectStoreSecret>;
}

#[derive(Debug, Default)]
pub(crate) struct NoObjectStoreCredentialResolver;

impl ObjectStoreCredentialResolver for NoObjectStoreCredentialResolver {
    fn resolve_object_store_secret(&self, id: &str) -> Result<ObjectStoreSecret> {
        Err(Error::InvalidInput(format!(
            "object-store credential secret ref {id} cannot be resolved: no credential resolver configured"
        )))
    }
}
