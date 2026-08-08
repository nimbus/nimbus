//! Transport-free compute surface extracted from `nimbus-server` (CP1/CP2):
//! runtime bundle execution, artifact/provenance admission, machine
//! lifecycle, service manager wiring, and the compute half of `AppState`
//! (`ComputeState`, config composition types, `ComputeError`). This crate
//! carries no HTTP/WebSocket transport framework on its own surface — the
//! server crate wraps `ComputeState` in a thin transport-owning `AppState`
//! and re-exports the pieces its adapters still need.

pub mod artifact_verifier_effects;
pub mod cloudflare_config;
pub mod config;
pub mod deploy;
pub mod execution;
pub mod machine_lifecycle;
pub mod machines;
pub mod node_workloads;
pub mod pagination;
pub mod resource_provision;
pub mod runtime_manager;
pub mod sandbox_spec;
pub mod sandboxes;
pub mod scheduling;
pub mod service_manager;
pub mod services;
pub mod state;
pub mod workload_executable;
pub mod workload_network_plan;
pub mod workload_projection;
pub mod workload_provision_composition;
pub mod workload_provision_source;
pub mod workload_provisioner;
pub mod workload_saga;

pub use resource_provision::{
    ComputeResourceProvisionError, ComputeResourceProvisioner, SandboxServiceProvisionSnapshot,
    SandboxServiceRetirementOutcome,
};
pub use workload_projection::{
    ServiceManagerWorkloadProjectionSink, WorkloadExecutionObservationCapability,
    WorkloadExecutionObservationFuture, WorkloadExecutionObservationRequest,
    WorkloadIngressBindingWitness, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest,
    WorkloadObservedIngressEndpoint, WorkloadObservedProjection, WorkloadProjectionOrchestrator,
    WorkloadProjectionPendingReason, WorkloadProjectionRejectedReason, WorkloadProjectionSink,
    WorkloadProjectionSinkError, WorkloadProjectionSinkFuture, WorkloadProjectionState,
    WorkloadProviderObservation,
};
pub use workload_provision_composition::{
    ComposedWorkloadProvision, WorkloadProvisionCompositionError,
    WorkloadProvisionCompositionInput, WorkloadProvisionSourceSnapshot, compose_workload_provision,
};
pub use workload_provision_source::ServiceManagerWorkloadProvisionSourceAuthority;
pub use workload_provisioner::{
    WorkloadProvisionCancellation, WorkloadProvisionConfigurationError,
    WorkloadProvisionEndpointSemantics, WorkloadProvisionError, WorkloadProvisionOutcome,
    WorkloadProvisionRequest, WorkloadProvisionResult, WorkloadProvisionSource,
    WorkloadProvisioner, embedded_local_node_identity,
};
