use serde::{Deserialize, Serialize};

use neovex_core::TenantId;

use crate::backend::SandboxBackendKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub tenant_id: TenantId,
    pub name: String,
    pub backend: SandboxBackendKind,
}

impl SandboxSpec {
    pub fn new(tenant_id: TenantId, name: impl Into<String>, backend: SandboxBackendKind) -> Self {
        Self {
            tenant_id,
            name: name.into(),
            backend,
        }
    }
}
