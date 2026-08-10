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
mod error;
mod execution_attempt;
mod inspection;
mod instance;
mod process;
mod provider_command;
mod provision;
mod spec;
mod teardown;
pub mod volume;

pub use crate::backends::oci::network::{MachinePortForwardOutcome, MachinePortForwardReceipt};
pub use crate::backends::oci::network::{OciNetworkProcess, OciNetworkProcessError};
pub use crate::backends::{SandboxNetworkPlanRequirements, sandbox_network_plan_requirements};
pub use backend::{SandboxBackend, SandboxBackendKind, SandboxFuture};
pub use error::{Result, SandboxError};
pub use execution_attempt::{
    SandboxExecutionAttemptId, SandboxExecutionAttemptIdError, SandboxRestartAttemptFence,
};
pub use inspection::{
    SandboxCleanupObservation, SandboxExecutionAttemptObservation, SandboxExecutionObservation,
    SandboxInspection, SandboxInspectionVersion, SandboxObservationUnknownReason,
    SandboxRestartAssessment, SandboxRestartBlocker, SandboxRestartIneligibility,
};
pub use instance::{SandboxHandle, SandboxId, SandboxStatus};
pub use provider_command::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandCurrentExecution, ProviderCommandCurrentInspection,
    ProviderCommandExecutionClaim, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandOperation,
};
pub use provision::{
    SandboxProvisionDependencyListener, SandboxProvisionIngressRoute,
    SandboxProvisionIngressTargetObservation, SandboxProvisionIngressTargets,
    SandboxProvisionListener, SandboxProvisionNetworkPlan, SandboxProvisionNetworkPlanError,
    SandboxProvisionPhaseObservation,
};
pub use spec::{
    SandboxLifecycleSpec, SandboxMountSource, SandboxMountSpec, SandboxOciBuildSpec,
    SandboxOciImageReferenceSpec, SandboxOciImageSource, SandboxOciImageSpec, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceCharge, SandboxResourceLimits,
    SandboxResourceQuotaPolicy, SandboxRestartPolicy, SandboxRootSpec, SandboxRootfsSpec,
    SandboxSpec, validate_sandbox_mounts, validate_tenant_volume_name,
};
pub use teardown::{
    SandboxExecutionTeardownCommand, SandboxExecutionTeardownCommandError,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation,
    SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandError,
    SandboxNetworkTeardownCommandInput, SandboxNetworkTeardownIdentity,
    SandboxNetworkTeardownIdentityInput, SandboxNetworkTeardownObservation,
    SandboxNetworkTeardownOperation,
};
