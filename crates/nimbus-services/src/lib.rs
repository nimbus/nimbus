//! Service registry and service manager primitives.

pub mod appws;
pub mod broker;
mod catalog;
pub mod frame;
pub mod hibernation;
pub mod ingress;
mod manager;
pub mod meter;
mod registry;
mod sandbox_templates;

pub use catalog::{
    BuiltInServiceSpec, DurableObjectActivationLease, DurableObjectId, DurableObjectIdError,
    DurableObjectInstance, DurableObjectInstanceKey, DurableObjectNamespace,
    DurableObjectNamespaceError, DurableObjectStorageHandle, EmptyServiceDefinitionCatalog,
    EmptyServiceInstanceCatalog, ExternalAuthPolicy, ExternalServiceSpec, HealthCheckPolicy,
    SandboxResource, SandboxResourceObservation, SandboxResourceSnapshot, SandboxResourceSource,
    ServiceBackend, ServiceDefinition, ServiceDefinitionCatalog, ServiceDefinitionObservation,
    ServiceDefinitionSource, ServiceInstanceCatalog, SessionLifecycleState, SessionResource,
    SessionTarget, SessionTargetSnapshot,
};
pub use manager::{
    LocalBuildAdmission, SandboxServiceProvisionSource, ServiceManager,
    StandaloneSandboxProvisionSource, TenantSourceRetirementClaim, TenantSourceRetirementSnapshot,
    TenantWorkloadSourceSnapshot, WorkloadSourceRetirementClaim, WorkloadSourceRetirementIdentity,
    WorkloadSourceRetirementOperation,
};
pub use registry::{
    RuntimeServiceRegistry, ServiceInstanceBindingRegistry, service_binding_from_handle,
};
pub use sandbox_templates::{
    ComposeSandboxTemplateService, DeployMode, EffectiveSandboxTemplatePolicy, LeasedSandbox,
    NimbusAppIntent, NimbusDeployPackage, SandboxTemplate, SandboxTemplateChannelEndpoint,
    SandboxTemplateLeaseController, SandboxTemplateLeaseRequest, SandboxTemplateProvenance,
};
