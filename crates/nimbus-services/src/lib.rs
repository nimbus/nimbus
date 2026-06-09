//! Service registry and service manager primitives.

mod catalog;
mod manager;
mod registry;

pub use catalog::{
    BuiltInServiceImplementation, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalServiceImplementation, SandboxBackedServiceImplementation, ServiceDefinitionCatalog,
    ServiceImplementation, ServiceInstanceCatalog,
};
pub use manager::{
    NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter, ServiceManager,
};
pub use registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, ServiceInstanceRuntimeRegistry,
    service_binding_from_handle,
};
