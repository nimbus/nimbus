//! Service registry and service manager primitives.

mod catalog;
mod manager;
mod registry;
mod sandbox_templates;
mod workload_control;

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
pub use sandbox_templates::{
    ComposeSandboxTemplateService, DeployMode, EffectiveSandboxTemplatePolicy, LeasedSandbox,
    NimbusAppIntent, NimbusDeployPackage, SandboxTemplate, SandboxTemplateChannelEndpoint,
    SandboxTemplateLeaseController, SandboxTemplateLeaseRequest, SandboxTemplateProvenance,
};
pub use workload_control::{
    DesiredWorkload, DesiredWorkloadKind, DesiredWorkloadSnapshot, DesiredWorkloadState,
    DesiredWorkloadStore, EmbeddedNodeClient, InMemoryDesiredWorkloadStore, NodeAssignment,
    NodeCapacity, PlacementPlan, SchedulingExplanation, WorkloadChannelDescriptor,
    WorkloadController, WorkloadEvaluation, WorkloadEventQueue, WorkloadExecutionPhase,
    WorkloadExecutionStatus, WorkloadExecutor, WorkloadPlacementEngine, WorkloadScheduler,
    WorkloadStatusUpdate,
};
