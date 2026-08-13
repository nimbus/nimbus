//! Native resource provisioning through the sole compute-owned saga facade.
//!
//! Services owns desired sources and observed projections. This module owns
//! only the product choreography that admits an exact local generation,
//! linearizes standalone source reservation with tracked submission, and
//! delegates every provider phase to [`WorkloadProvisioner`].

use std::collections::BTreeMap;
use std::sync::Arc;

use nimbus_core::{Error, TenantId};
use nimbus_network::{EndpointProtocol, NetworkTlsBehavior};
use nimbus_sandbox::SandboxSpec;
use nimbus_services::{
    SandboxResourceSnapshot, ServiceDefinition, ServiceDefinitionObservation, ServiceManager,
};
use nimbus_tenant::{TenantIsolationContext, WorkloadLocation};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadNetworkForwardingBehavior, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaKey,
};
use thiserror::Error;

use crate::state::{ComputeError, ComputeState};
use crate::workload_projection::WorkloadProjectionState;
use crate::workload_provisioner::{
    WorkloadProvisionCancellation, WorkloadProvisionEndpointSemantics, WorkloadProvisionError,
    WorkloadProvisionRequest, WorkloadProvisionSource, WorkloadProvisioner,
};
use crate::workload_saga::sandbox_execution_provider_id;

/// Exact service definition plus its optional services-owned observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxServiceProvisionSnapshot {
    pub definition: ServiceDefinition,
    pub observation: Option<ServiceDefinitionObservation>,
}

/// Product-facade failure before a truthful native response can be formed.
#[derive(Debug, Error)]
pub enum ComputeResourceProvisionError {
    #[error("resource source or tenant admission failed: {0}")]
    Source(#[from] Error),
    #[error("workload provision failed: {0}")]
    Provision(Arc<WorkloadProvisionError>),
    #[error("workload provision was rejected before an observed projection: {reason}")]
    Rejected { reason: &'static str },
    #[error(
        "projected workload `{name}` for tenant `{tenant_id}` has no exact services observation"
    )]
    MissingProjection { tenant_id: TenantId, name: String },
}

impl ComputeResourceProvisionError {
    pub fn into_compute_error(self) -> ComputeError {
        match self {
            Self::Source(error) => ComputeError::from(error),
            other => ComputeError::from(Error::Internal(other.to_string())),
        }
    }
}

/// One exact services source owner paired with the sole compute provisioner.
#[derive(Clone)]
pub struct ComputeResourceProvisioner {
    services: Arc<ServiceManager>,
    provisioner: Arc<WorkloadProvisioner>,
}

impl ComputeResourceProvisioner {
    pub fn new(services: Arc<ServiceManager>, provisioner: Arc<WorkloadProvisioner>) -> Self {
        Self {
            services,
            provisioner,
        }
    }

    /// Provision one caller-stable standalone resource through the saga.
    pub async fn provision_standalone_sandbox(
        &self,
        context: &TenantIsolationContext,
        stable_resource_id: &str,
        profile: &str,
        spec: SandboxSpec,
        labels: BTreeMap<String, String>,
        cancellation: &WorkloadProvisionCancellation,
    ) -> Result<SandboxResourceSnapshot, ComputeResourceProvisionError> {
        let prepared = self.services.prepare_standalone_sandbox_provision_source(
            context.tenant_id(),
            stable_resource_id,
            profile,
            spec,
            labels,
        )?;
        let source = prepared.source().clone();
        let resource_version = source_version(&source.resource_version)?;
        let key = WorkloadSagaKey::new(
            context.tenant_id().clone(),
            nimbus_core::WorkloadId::new(stable_resource_id)?,
        );
        let lifecycle_generation = self
            .provisioner
            .lifecycle_generation_for_start(
                &key,
                WorkloadProvisionSourceGeneration::new(source.generation),
                &resource_version,
            )
            .await?;
        let decision = admit_local_generation(
            context,
            lifecycle_generation,
            self.provisioner.local_node(),
            prepared.policy_input().clone(),
        )?;
        self.services
            .validate_standalone_sandbox_provision_decision(&decision, &prepared)?;
        let request = provision_request(
            decision.clone(),
            WorkloadProvisionSource::StandaloneSandbox {
                stable_resource_id: source.id.clone(),
                profile: source.profile.clone(),
                source_generation: WorkloadProvisionSourceGeneration::new(source.generation),
                resource_version,
                sandbox_spec: source.spec.clone(),
            },
            &source.spec,
        );
        let services = self.services.clone();
        let outcome = self
            .provisioner
            .provision_with_source_reservation(request, cancellation, move || {
                services
                    .reserve_standalone_sandbox_provision_source(&decision, prepared)
                    .map(|_| ())
            })
            .await
            .map_err(ComputeResourceProvisionError::Provision)?;
        require_accepted_projection(outcome.projection())?;
        let snapshot = self
            .services
            .sandbox_resource_snapshot_for_tenant(context.tenant_id(), stable_resource_id)?
            .ok_or_else(|| ComputeResourceProvisionError::MissingProjection {
                tenant_id: context.tenant_id().clone(),
                name: stable_resource_id.to_owned(),
            })?;
        if outcome.projection() == WorkloadProjectionState::Projected
            && snapshot.observation.as_ref().is_none_or(|observation| {
                observation.source_generation != snapshot.source.generation
                    || observation.execution != outcome.record().current_execution_reference()
            })
        {
            return Err(ComputeResourceProvisionError::MissingProjection {
                tenant_id: context.tenant_id().clone(),
                name: stable_resource_id.to_owned(),
            });
        }
        Ok(snapshot)
    }

    /// Provision one declared sandbox-backed service through the same saga.
    pub async fn provision_sandbox_service(
        &self,
        context: &TenantIsolationContext,
        service_name: &str,
        cancellation: &WorkloadProvisionCancellation,
    ) -> Result<SandboxServiceProvisionSnapshot, ComputeResourceProvisionError> {
        let prepared = self
            .services
            .prepare_sandbox_service_provision_source(context.tenant_id(), service_name)?;
        let definition = prepared.definition().clone();
        let resource_version = source_version(&definition.resource_version)?;
        let key = WorkloadSagaKey::new(
            context.tenant_id().clone(),
            nimbus_core::WorkloadId::new(service_name)?,
        );
        let lifecycle_generation = self
            .provisioner
            .lifecycle_generation_for_start(
                &key,
                WorkloadProvisionSourceGeneration::new(definition.generation),
                &resource_version,
            )
            .await?;
        let decision = admit_local_generation(
            context,
            lifecycle_generation,
            self.provisioner.local_node(),
            prepared.policy_input().clone(),
        )?;
        self.services
            .validate_sandbox_service_provision_decision(&decision, &prepared)?;
        let request = provision_request(
            decision.clone(),
            WorkloadProvisionSource::SandboxBackedService {
                service_name: definition.name.clone(),
                source_generation: WorkloadProvisionSourceGeneration::new(definition.generation),
                resource_version,
                sandbox_spec: prepared.sandbox_spec().clone(),
            },
            prepared.sandbox_spec(),
        );
        let services = Arc::clone(&self.services);
        let outcome = self
            .provisioner
            .provision_with_source_reservation(request, cancellation, move || {
                services.reserve_sandbox_service_provision_source(&decision, prepared)
            })
            .await
            .map_err(ComputeResourceProvisionError::Provision)?;
        require_accepted_projection(outcome.projection())?;
        let observation = self
            .services
            .service_definition_observation_for_tenant(context.tenant_id(), service_name);
        if outcome.projection() == WorkloadProjectionState::Projected
            && observation.as_ref().is_none_or(|observation| {
                observation.source_generation != definition.generation
                    || observation.execution != outcome.record().current_execution_reference()
            })
        {
            return Err(ComputeResourceProvisionError::MissingProjection {
                tenant_id: context.tenant_id().clone(),
                name: service_name.to_owned(),
            });
        }
        Ok(SandboxServiceProvisionSnapshot {
            definition,
            observation,
        })
    }
}

fn admit_local_generation(
    context: &TenantIsolationContext,
    generation: u64,
    local_node: &nimbus_workloads::NodeIdentity,
    policy_input: nimbus_tenant::TenantIsolationPolicyInput,
) -> Result<nimbus_tenant::TenantIsolationDecision, Error> {
    context
        .clone()
        .with_deployment_generation(generation)
        .with_workload_location(WorkloadLocation::new().with_node_id(local_node.as_str()))
        .admit_decision(policy_input)
}

fn provision_request(
    decision: nimbus_tenant::TenantIsolationDecision,
    source: WorkloadProvisionSource,
    spec: &SandboxSpec,
) -> WorkloadProvisionRequest {
    let endpoint_semantics = spec
        .port_bindings
        .iter()
        .map(|binding| {
            WorkloadProvisionEndpointSemantics::new(
                binding.name.clone(),
                WorkloadNetworkForwardingBehavior::PortForwarded,
                match binding.protocol {
                    EndpointProtocol::Https => NetworkTlsBehavior::Passthrough,
                    EndpointProtocol::Tcp | EndpointProtocol::Http => NetworkTlsBehavior::Disabled,
                },
            )
        })
        .collect();
    WorkloadProvisionRequest {
        decision,
        source,
        execution_provider_id: sandbox_execution_provider_id(spec.backend),
        endpoint_semantics,
        activation: WorkloadActivationIntent::ActivateWhenAttached,
        publication: if spec.port_bindings.is_empty() {
            WorkloadPublicationIntent::Withheld
        } else {
            WorkloadPublicationIntent::PublishWhenReady
        },
    }
}

fn source_version(value: &str) -> Result<WorkloadProvisionSourceResourceVersion, Error> {
    WorkloadProvisionSourceResourceVersion::new(value.to_owned()).map_err(|error| {
        Error::InvalidInput(format!("invalid services-owned resource version: {error}"))
    })
}

fn require_accepted_projection(
    projection: WorkloadProjectionState,
) -> Result<(), ComputeResourceProvisionError> {
    match projection {
        WorkloadProjectionState::Projected | WorkloadProjectionState::Pending(_) => Ok(()),
        WorkloadProjectionState::Rejected(reason) => Err(ComputeResourceProvisionError::Rejected {
            reason: projection_rejection_reason(reason),
        }),
    }
}

fn projection_rejection_reason(
    reason: crate::workload_projection::WorkloadProjectionRejectedReason,
) -> &'static str {
    use crate::workload_projection::WorkloadProjectionRejectedReason as Reason;
    match reason {
        Reason::ProvisionDefiniteFailure => "provision_definite_failure",
        Reason::DurableRecordNotObserved => "durable_record_not_observed",
        Reason::MissingExecutionObservationCapability => "missing_execution_observation_capability",
        Reason::MissingIngressObservationCapability => "missing_ingress_observation_capability",
        Reason::InvalidExecutionEvidence => "invalid_execution_evidence",
        Reason::InvalidPublicationReference => "invalid_publication_reference",
        Reason::InvalidIngressEvidence => "invalid_ingress_evidence",
        Reason::WithheldPublicationCarriedEndpoints => "withheld_publication_carried_endpoints",
        Reason::ProjectionSinkRejected => "projection_sink_rejected",
    }
}

impl ComputeState {
    /// Resolve the complete managed native resource facade.
    pub fn resource_provisioner(&self) -> Result<ComputeResourceProvisioner, ComputeError> {
        if self.workload_teardown_runtime().is_none() {
            return Err(ComputeError::not_found(
                "native workload provisioning requires exact teardown composition",
            ));
        }
        let services = self.service_manager().ok_or_else(|| {
            ComputeError::not_found("native workload provisioning requires a services source owner")
        })?;
        let provisioner = self.workload_provisioner().ok_or_else(|| {
            ComputeError::not_found("native workload provisioning requires managed compute")
        })?;
        Ok(ComputeResourceProvisioner::new(services, provisioner))
    }
}

#[cfg(test)]
#[path = "resource_provision/tests.rs"]
mod tests;
