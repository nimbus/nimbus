use std::collections::BTreeMap;
use std::sync::Arc;

/// Proxy-owned credential material seam (K11P6). The PEP resolves secret
/// material only through this trait, so a future secret-management
/// `SecretProvider` can supply values from outside this crate without secret
/// material ever appearing in policy structs, sandbox adapters, runtime
/// crates, or logs. It must stay public: external providers implement it.
pub trait CredentialSecretProvider: Send + Sync {
    fn resolve_credential_secret(&self, credential_ref: &str) -> Option<String>;
}

pub type CredentialSecretProviderRef = Arc<dyn CredentialSecretProvider>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialSecretStore {
    entries: BTreeMap<String, String>,
}

impl CredentialSecretStore {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_entries(
        entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }

    pub(crate) fn get(&self, credential_ref: &str) -> Option<&str> {
        self.entries.get(credential_ref).map(String::as_str)
    }

    pub(crate) fn into_provider(self) -> CredentialSecretProviderRef {
        Arc::new(self)
    }
}

impl CredentialSecretProvider for CredentialSecretStore {
    fn resolve_credential_secret(&self, credential_ref: &str) -> Option<String> {
        self.get(credential_ref).map(ToOwned::to_owned)
    }
}
