//! Service registry and service manager primitives.

mod catalog;
mod manager;
mod registry;
mod sandbox_templates;

pub use catalog::{
    BuiltInServiceSpec, DurableObjectActivationLease, DurableObjectId, DurableObjectIdError,
    DurableObjectInstance, DurableObjectInstanceKey, DurableObjectNamespace,
    DurableObjectNamespaceError, DurableObjectStorageHandle, EmptyServiceDefinitionCatalog,
    EmptyServiceInstanceCatalog, ExternalAuthPolicy, ExternalServiceSpec, HealthCheckPolicy,
    SandboxResource, ServiceBackend, ServiceDefinition, ServiceDefinitionCatalog,
    ServiceDefinitionSource, ServiceInstanceCatalog, SessionLifecycleState, SessionResource,
    SessionTarget, SessionTargetSnapshot,
};
pub use manager::{
    LocalBuildAdmission, NoopServiceEvidenceWriter, ServiceEvidenceFuture, ServiceEvidenceWriter,
    ServiceManager,
};
pub use registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, RuntimeServiceTeardownFuture,
    ServiceInstanceBindingRegistry, service_binding_from_handle,
};
pub use sandbox_templates::{
    ComposeSandboxTemplateService, DeployMode, EffectiveSandboxTemplatePolicy, LeasedSandbox,
    NimbusAppIntent, NimbusDeployPackage, SandboxTemplate, SandboxTemplateChannelEndpoint,
    SandboxTemplateLeaseController, SandboxTemplateLeaseRequest, SandboxTemplateProvenance,
};
