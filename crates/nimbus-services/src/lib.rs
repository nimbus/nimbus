//! Service registry and service manager primitives.

mod catalog;
mod manager;
mod registry;

pub use catalog::{
    BuiltInServiceSpec, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalServiceSpec, ServiceBackend, ServiceDefinitionCatalog, ServiceInstanceCatalog,
};
pub use manager::{
    NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter, ServiceManager,
};
pub use registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, ServiceInstanceBindingRegistry,
    service_binding_from_handle,
};
