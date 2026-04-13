use serde::{Deserialize, Serialize};

use crate::backend::SandboxBackendKind;
use crate::endpoint::PublishedEndpoint;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(String);

impl SandboxId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SandboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStatus {
    Starting,
    Ready,
    NotReady,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxHandle {
    pub id: SandboxId,
    pub name: String,
    pub backend: SandboxBackendKind,
    pub status: SandboxStatus,
    pub published_endpoints: Vec<PublishedEndpoint>,
}

impl SandboxHandle {
    pub fn new(
        id: SandboxId,
        name: impl Into<String>,
        backend: SandboxBackendKind,
        status: SandboxStatus,
        published_endpoints: Vec<PublishedEndpoint>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            backend,
            status,
            published_endpoints,
        }
    }
}
