//! Service registry and sandbox service manager primitives.

mod catalog;
mod manager;
mod registry;

pub use catalog::{
    EmptySandboxCatalog, EmptySandboxServiceCatalog, SandboxCatalog, SandboxServiceCatalog,
    SandboxServiceLaunch,
};
pub use manager::{
    NoopServiceEvidenceWriter, SandboxServiceManager, ServiceEvidenceFuture, ServiceEvidenceWriter,
};
pub use registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, SandboxCatalogRuntimeServiceRegistry,
    service_binding_from_handle,
};
