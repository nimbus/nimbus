use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeGrants;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeEnvPolicy {
    allowed_read_names: BTreeSet<String>,
    allowed_write_names: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub(crate) enum RuntimeEnvLookupDescriptor {
    Allowed { value: String },
    Missing,
    Denied { message: String },
}

impl RuntimeEnvPolicy {
    pub(crate) fn for_grants(grants: &RuntimeGrants) -> Self {
        let allowed_read_names = grants.env_read.iter().cloned().collect();
        let allowed_write_names = grants.env_write.iter().cloned().collect();
        Self {
            allowed_read_names,
            allowed_write_names,
        }
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<String, String> {
        self.allowed_read_names
            .iter()
            .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
            .collect()
    }

    pub(crate) fn lookup(&self, name: &str) -> RuntimeEnvLookupDescriptor {
        if let Err(error) = self.ensure_read_name(name) {
            return RuntimeEnvLookupDescriptor::Denied {
                message: error.to_string(),
            };
        }
        match std::env::var(name) {
            Ok(value) => RuntimeEnvLookupDescriptor::Allowed { value },
            Err(std::env::VarError::NotPresent) => RuntimeEnvLookupDescriptor::Missing,
            Err(std::env::VarError::NotUnicode(_)) => RuntimeEnvLookupDescriptor::Denied {
                message: format!(
                    "runtime env capability denied for `{name}`; value is not valid UTF-8"
                ),
            },
        }
    }

    pub(crate) fn allowed_names(&self) -> Vec<String> {
        self.allowed_read_names.iter().cloned().collect()
    }

    pub(super) fn has_allowed_read_names(&self) -> bool {
        !self.allowed_read_names.is_empty()
    }

    pub(crate) fn ensure_read_name(&self, name: &str) -> Result<()> {
        ensure_env_name_allowed(name, &self.allowed_read_names, "read")
    }

    pub(crate) fn ensure_write_name(&self, name: &str) -> Result<()> {
        ensure_env_name_allowed(name, &self.allowed_write_names, "write")
    }

    pub(crate) fn filter_readable_snapshot(
        &self,
        snapshot: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        snapshot
            .into_iter()
            .filter(|(name, _)| self.allowed_read_names.contains(name))
            .collect()
    }
}

fn ensure_env_name_allowed(
    name: &str,
    allowed_names: &BTreeSet<String>,
    access: &str,
) -> Result<()> {
    if !is_valid_env_name(name) {
        return Err(NimbusRuntimeError::CapabilityDenied(format!(
            "runtime env {access} capability denied for invalid variable name `{name}`"
        )));
    }
    if !allowed_names.contains(name) {
        return Err(NimbusRuntimeError::CapabilityDenied(format!(
            "runtime env {access} capability denied for `{name}`; env {access} access is allowlist-only"
        )));
    }
    Ok(())
}

fn is_valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
