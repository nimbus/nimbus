use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::instance::{SandboxHandle, SandboxId};
use crate::spec::SandboxSpec;

pub type SandboxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackendKind {
    Krun,
}

pub trait SandboxBackend: Send + Sync + 'static {
    fn kind(&self) -> SandboxBackendKind;

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle>;

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>>;

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()>;
}
