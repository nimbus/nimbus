//! Generic sandbox and isolation contracts for Nimbus.
//!
//! This crate intentionally owns only stable, backend-agnostic lifecycle nouns.
//! Concrete implementations such as a krun-backed sandbox or future
//! Firecracker support should live behind backend-owned module paths in this
//! crate rather than leaking their implementation vocabulary into the rest of
//! the workspace.

pub mod backends;

mod artifact_paths;
mod backend;
mod egress;
mod endpoint;
mod error;
mod instance;
mod process;
mod spec;

pub use backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
pub use egress::{
    SandboxEgressAuthorization, SandboxEgressPolicy, SandboxEgressRequest, SandboxEgressRule,
};
pub use endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
pub use error::{Result, SandboxError};
pub use instance::{SandboxHandle, SandboxId, SandboxStatus};
pub use spec::{
    SandboxBuildLaunchSpec, SandboxFilesystemSpec, SandboxImageLaunchSpec,
    SandboxImageProcessOverrides, SandboxLifecycleSpec, SandboxMountSource, SandboxMountSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceCharge, SandboxResourceLimits,
    SandboxResourceQuotaPolicy, SandboxRestartPolicy, SandboxSpec, validate_sandbox_mounts,
    validate_tenant_volume_name,
};
