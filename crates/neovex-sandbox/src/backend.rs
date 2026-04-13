use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::instance::{SandboxHandle, SandboxId};
use crate::spec::{SandboxBuildLaunchSpec, SandboxImageLaunchSpec, SandboxSpec};

pub type SandboxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackendKind {
    Krun,
}

pub trait SandboxBackend: Send + Sync + 'static {
    fn kind(&self) -> SandboxBackendKind;

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle>;

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.kind();
        Box::pin(async move {
            Err(crate::error::SandboxError::InvalidSpec {
                message: format!(
                    "sandbox backend {:?} does not support image-backed launches for {}",
                    backend, launch.spec.name
                ),
            })
        })
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        let backend = self.kind();
        Box::pin(async move {
            Err(crate::error::SandboxError::InvalidSpec {
                message: format!(
                    "sandbox backend {:?} does not support build-backed launches for {}",
                    backend, launch.spec.name
                ),
            })
        })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>>;

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()>;
}
