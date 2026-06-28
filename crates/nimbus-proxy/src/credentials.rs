use std::collections::BTreeMap;

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
}
