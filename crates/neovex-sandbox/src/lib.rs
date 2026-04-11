//! Generic sandbox and isolation contracts for Neovex.
//!
//! This crate intentionally owns only stable, backend-agnostic lifecycle nouns.
//! Concrete implementations such as a krun-backed sandbox or future
//! Firecracker support should live behind backend-owned module paths in this
//! crate rather than leaking their implementation vocabulary into the rest of
//! the workspace.

mod backend;
mod endpoint;
mod error;
mod instance;
mod spec;

pub use backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
pub use endpoint::{PublishedEndpoint, PublishedEndpointProtocol};
pub use error::{Result, SandboxError};
pub use instance::{SandboxHandle, SandboxId, SandboxStatus};
pub use spec::SandboxSpec;
