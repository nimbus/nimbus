//! Pure composition of one admitted workload-provision intent.
//!
//! This module accepts only immutable decision and source snapshots. It owns
//! validation and portable desired-state construction, never persistence or a
//! provider effect.

use nimbus_core::WorkloadId;
use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilitySelection, NetworkSovereigntyRequirements,
};
use nimbus_sandbox::SandboxSpec;
use nimbus_tenant::TenantIsolationDecision;
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, TenantWorkloadSpec,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutionProviderId,
    WorkloadGeneration, WorkloadNetworkIntent, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaIntent,
    WorkloadSagaKey,
};
use thiserror::Error;

use crate::workload_executable::{WorkloadExecutableCodecError, encode_sandbox_spec};
use crate::workload_network_plan::{
    AdmittedWorkloadNetworkSource, WorkloadNetworkEndpointSemanticsInput,
    WorkloadNetworkPlanCompileError, WorkloadNetworkPlanCompiler,
};

/// Closed source snapshots accepted for workload provisioning.
#[derive(Debug, Clone, Copy)]
pub enum WorkloadProvisionSourceSnapshot<'source> {
    /// One standalone sandbox resource and its complete immutable spec.
    StandaloneSandbox {
        stable_resource_id: &'source str,
        profile: &'source str,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: &'source WorkloadProvisionSourceResourceVersion,
        sandbox_spec: &'source SandboxSpec,
    },
    /// One service definition whose executable source is a sandbox.
    SandboxBackedService {
        service_name: &'source str,
        source_generation: WorkloadProvisionSourceGeneration,
        resource_version: &'source WorkloadProvisionSourceResourceVersion,
        sandbox_spec: &'source SandboxSpec,
    },
}

impl<'source> WorkloadProvisionSourceSnapshot<'source> {
    fn sandbox_spec(self) -> &'source SandboxSpec {
        match self {
            Self::StandaloneSandbox { sandbox_spec, .. }
            | Self::SandboxBackedService { sandbox_spec, .. } => sandbox_spec,
        }
    }

    fn stable_name(self) -> &'source str {
        match self {
            Self::StandaloneSandbox {
                stable_resource_id, ..
            } => stable_resource_id,
            Self::SandboxBackedService { service_name, .. } => service_name,
        }
    }

    fn desired_kind(self) -> DesiredWorkloadKind {
        match self {
            Self::StandaloneSandbox { .. } => DesiredWorkloadKind::Sandbox,
            Self::SandboxBackedService { .. } => DesiredWorkloadKind::Service,
        }
    }
}

/// Complete immutable inputs to the sole pure provision constructor.
pub struct WorkloadProvisionCompositionInput<'input> {
    pub decision: &'input TenantIsolationDecision,
    pub local_node: &'input NodeIdentity,
    pub source: WorkloadProvisionSourceSnapshot<'input>,
    pub execution_provider_id: &'input WorkloadExecutionProviderId,
    pub capability_selection: &'input NetworkCapabilitySelection,
    pub capability_registry: &'input NetworkCapabilityRegistry,
    pub sovereignty: NetworkSovereigntyRequirements,
    pub endpoint_semantics: &'input [WorkloadNetworkEndpointSemanticsInput<'input>],
    pub activation: WorkloadActivationIntent,
    pub publication: WorkloadPublicationIntent,
}

/// Exact tenant-qualified key and complete intent produced together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedWorkloadProvision {
    key: WorkloadSagaKey,
    intent: WorkloadSagaIntent,
}

impl ComposedWorkloadProvision {
    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn intent(&self) -> &WorkloadSagaIntent {
        &self.intent
    }

    pub fn into_parts(self) -> (WorkloadSagaKey, WorkloadSagaIntent) {
        (self.key, self.intent)
    }
}

/// A fail-before composition rejection.
#[derive(Debug, Error)]
pub enum WorkloadProvisionCompositionError {
    #[error("the admitted workload projection is invalid: {message}")]
    InvalidWorkloadProjection { message: String },
    #[error("the tenant decision does not carry an assigned node")]
    MissingNodeAssignment,
    #[error("local node {local} does not match admitted assignment {admitted}")]
    NodeMismatch { admitted: String, local: String },
    #[error("the admitted workload generation is missing")]
    MissingDeploymentGeneration,
    #[error("the logical workload key is invalid: {message}")]
    InvalidWorkloadKey { message: String },
    #[error("the executable source is invalid: {0}")]
    Executable(#[from] WorkloadExecutableCodecError),
    #[error("the network plan is invalid: {0}")]
    Network(#[from] WorkloadNetworkPlanCompileError),
    #[error("the provision source or desired intent is invalid: {0}")]
    Intent(#[from] nimbus_workloads::WorkloadSagaError),
}

/// Compose one exact provision submission without persistence or effects.
pub fn compose_workload_provision(
    input: WorkloadProvisionCompositionInput<'_>,
) -> Result<ComposedWorkloadProvision, WorkloadProvisionCompositionError> {
    let workload = TenantWorkloadSpec::from_decision(input.decision).map_err(|error| {
        WorkloadProvisionCompositionError::InvalidWorkloadProjection {
            message: error.to_string(),
        }
    })?;
    let admitted_node = workload
        .assigned_node_id()
        .ok_or(WorkloadProvisionCompositionError::MissingNodeAssignment)?;
    if admitted_node != input.local_node {
        return Err(WorkloadProvisionCompositionError::NodeMismatch {
            admitted: admitted_node.as_str().to_owned(),
            local: input.local_node.as_str().to_owned(),
        });
    }
    let deployment_generation = input
        .decision
        .workload_identity()
        .deployment_generation()
        .ok_or(WorkloadProvisionCompositionError::MissingDeploymentGeneration)?;
    debug_assert_eq!(
        workload.generation(),
        WorkloadGeneration::new(deployment_generation)
    );

    let executable = encode_sandbox_spec(input.source.sandbox_spec())?;
    let admitted_source = match input.source {
        WorkloadProvisionSourceSnapshot::StandaloneSandbox {
            stable_resource_id,
            profile,
            sandbox_spec,
            ..
        } => AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id,
            profile,
            generation: deployment_generation,
            sandbox_spec,
        },
        WorkloadProvisionSourceSnapshot::SandboxBackedService {
            service_name,
            sandbox_spec,
            ..
        } => AdmittedWorkloadNetworkSource::SandboxBackedService {
            service_name,
            service_generation: deployment_generation,
            sandbox_spec,
        },
    };
    let compiled_network = WorkloadNetworkPlanCompiler.compile(
        input.decision,
        admitted_source,
        Some(input.capability_selection),
        input.capability_registry,
        input.sovereignty,
        input.endpoint_semantics,
        input.activation,
        input.publication,
    )?;

    let source = match input.source {
        WorkloadProvisionSourceSnapshot::StandaloneSandbox {
            stable_resource_id,
            profile,
            source_generation,
            resource_version,
            ..
        } => WorkloadProvisionSourceEvidence::standalone_sandbox(
            WorkloadProvisionSourceIdentity::standalone_sandbox(stable_resource_id, profile)?,
            source_generation,
            resource_version.clone(),
            executable.content_digest(),
            input.capability_selection.attachment_provider_id().clone(),
            input.execution_provider_id.clone(),
        )?,
        WorkloadProvisionSourceSnapshot::SandboxBackedService {
            service_name,
            source_generation,
            resource_version,
            ..
        } => WorkloadProvisionSourceEvidence::sandbox_backed_service(
            WorkloadProvisionSourceIdentity::sandbox_backed_service(service_name)?,
            source_generation,
            resource_version.clone(),
            executable.content_digest(),
            input.capability_selection.attachment_provider_id().clone(),
            input.execution_provider_id.clone(),
        )?,
    };

    let workload_id = WorkloadId::new(input.source.stable_name()).map_err(|error| {
        WorkloadProvisionCompositionError::InvalidWorkloadKey {
            message: error.to_string(),
        }
    })?;
    let key = WorkloadSagaKey::new(workload.tenant_id().clone(), workload_id);
    let intent = WorkloadSagaIntent::new(
        input.source.desired_kind(),
        DesiredWorkloadState::Running,
        workload.generation(),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_network),
        input.activation,
        input.publication,
        WorkloadAdmissionEvidence::new(
            workload.decision_id().clone(),
            workload.workload_uid().clone(),
            input.local_node.clone(),
        ),
    )?;

    Ok(ComposedWorkloadProvision { key, intent })
}

#[cfg(test)]
#[path = "workload_provision_composition/tests.rs"]
mod tests;
