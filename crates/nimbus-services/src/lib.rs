//! Service registry and service manager primitives.

mod catalog;
mod manager;
mod registry;

pub use catalog::{
    BuiltInServiceSpec, EmptyServiceDefinitionCatalog, EmptyServiceInstanceCatalog,
    ExternalAuthPolicy, ExternalServiceSpec, HealthCheckPolicy, SandboxResource, ServiceBackend,
    ServiceDefinition, ServiceDefinitionCatalog, ServiceDefinitionSource, ServiceInstanceCatalog,
    SessionLifecycleState, SessionResource, SessionTarget, SessionTargetSnapshot,
};
pub use manager::{
    LocalBuildAdmission, NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter,
    ServiceManager,
};
pub use registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, RuntimeServiceTeardownFuture,
    ServiceInstanceBindingRegistry, service_binding_from_handle,
};
