use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use nimbus_core::TenantId;

use crate::error::Result;
use crate::inspection::SandboxInspection;
use crate::instance::SandboxId;
use nimbus_egress::EgressPolicy;

pub type SandboxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackendKind {
    Container,
    Krun,
}

pub trait SandboxBackend: Send + Sync + 'static {
    fn kind(&self) -> SandboxBackendKind;

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>>;

    fn reload_egress_policy(&self, id: &SandboxId, _egress: EgressPolicy) -> SandboxFuture<()> {
        let backend = self.kind();
        let sandbox_id = id.clone();
        Box::pin(async move {
            Err(crate::error::SandboxError::InvalidSpec {
                message: format!(
                    "sandbox backend {backend:?} does not support live egress reload for {sandbox_id}"
                ),
            })
        })
    }

    fn remove_tenant_artifacts(&self, _tenant_id: TenantId) -> SandboxFuture<()> {
        Box::pin(async { Ok(()) })
    }
}
