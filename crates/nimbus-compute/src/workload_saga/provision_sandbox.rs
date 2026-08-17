//! Concrete sandbox-provider substitutions for narrow provision capabilities.
//!
//! Each adapter authenticates the canonical executable and stable execution
//! identity, delegates idempotency to its backend-local attempt journal, and
//! invokes only the capability requested by the compute dispatcher.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use nimbus_network::{
    NetworkLeaseEpoch, PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure,
    PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence, PortLeaseRequest, PortProtocol,
    PortPublicationIntent, PortRequestMode,
};
use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::backends::krun::KrunSandboxBackend;
use nimbus_sandbox::{
    ProviderCommandJournalError, SandboxBackend, SandboxBackendKind, SandboxError, SandboxId,
    SandboxProvisionDependencyListener, SandboxProvisionEndpointIdentity, SandboxProvisionListener,
    SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation, SandboxSpec,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadGeneration, WorkloadNetworkPortRequestMode,
};

use super::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use super::restart_provider_command::ProviderRestartPhaseAdapter;
use super::{
    ConfirmedWorkloadProvisionCommand, NetworkAttachmentCapability, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadPreparationCapability, WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
};
use crate::workload_executable::decode_sandbox_spec;
use crate::workload_projection::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadProviderObservation,
};

const CONTAINER_EXECUTION_PROVIDER_KEY: &str = "nimbus-sandbox.container-execution";
const KRUN_EXECUTION_PROVIDER_KEY: &str = "nimbus-sandbox.krun-execution";

/// Stable execution-provider identity for one concrete sandbox backend.
pub fn sandbox_execution_provider_id(
    backend: SandboxBackendKind,
) -> nimbus_workloads::WorkloadExecutionProviderId {
    let registration_key = match backend {
        SandboxBackendKind::Container => CONTAINER_EXECUTION_PROVIDER_KEY,
        SandboxBackendKind::Krun => KRUN_EXECUTION_PROVIDER_KEY,
    };
    nimbus_workloads::WorkloadExecutionProviderId::for_registration_key(registration_key)
}

/// Exact sandbox-owned inputs authenticated from one confirmed command.
///
/// Provider adapters use this view instead of re-deriving execution,
/// attachment, listener, lease, tenant, plan, or generation identity.
pub struct ValidatedSandboxProvisionCommand {
    spec: SandboxSpec,
    sandbox_id: SandboxId,
    execution_attempt_id: nimbus_sandbox::SandboxExecutionAttemptId,
    network_plan: SandboxProvisionNetworkPlan,
}

impl ValidatedSandboxProvisionCommand {
    pub fn spec(&self) -> &SandboxSpec {
        &self.spec
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub fn execution_attempt_id(&self) -> &nimbus_sandbox::SandboxExecutionAttemptId {
        &self.execution_attempt_id
    }

    pub fn network_plan(&self) -> &SandboxProvisionNetworkPlan {
        &self.network_plan
    }
}

/// Authenticate one confirmed command for an exact sandbox backend.
pub fn validate_sandbox_provision_command(
    command: &ConfirmedWorkloadProvisionCommand,
    backend: SandboxBackendKind,
) -> Result<ValidatedSandboxProvisionCommand, ProviderProvisionEffectObservation> {
    let spec = decode_sandbox_spec(command.executable())
        .map_err(|error| definite_failure("invalid_executable", error.to_string()))?;
    if spec.backend != backend {
        return Err(definite_failure(
            "execution_backend_mismatch",
            format!(
                "command selected {backend:?}, but executable requests {:?}",
                spec.backend
            ),
        ));
    }
    if &spec.tenant_id != command.key().tenant_id() {
        return Err(definite_failure(
            "execution_tenant_mismatch",
            format!(
                "command tenant {} does not match executable tenant {}",
                command.key().tenant_id(),
                spec.tenant_id
            ),
        ));
    }
    let network_plan = sandbox_network_plan(command, &spec)?;
    Ok(ValidatedSandboxProvisionCommand {
        spec,
        sandbox_id: SandboxId::new(command.execution().execution_id().as_str()),
        execution_attempt_id: nimbus_sandbox::SandboxExecutionAttemptId::new(
            command.execution().attempt_id().to_string(),
        )
        .map_err(|error| definite_failure("invalid_execution_attempt", error.to_string()))?,
        network_plan,
    })
}

fn sandbox_network_plan(
    command: &ConfirmedWorkloadProvisionCommand,
    spec: &SandboxSpec,
) -> Result<SandboxProvisionNetworkPlan, ProviderProvisionEffectObservation> {
    sandbox_network_plan_for(command.generation(), command.compiled_network_plan(), spec)
}

pub(crate) fn sandbox_network_plan_for(
    generation: WorkloadGeneration,
    compiled_network_plan: &CompiledWorkloadNetworkPlan,
    spec: &SandboxSpec,
) -> Result<SandboxProvisionNetworkPlan, ProviderProvisionEffectObservation> {
    let content = compiled_network_plan.content();
    if content.identity().tenant_id() != &spec.tenant_id
        || content.identity().generation().as_u64() != generation.as_u64()
    {
        return Err(definite_failure(
            "network_plan_identity_mismatch",
            "compiled network-plan tenant or generation does not match the provision command",
        ));
    }
    let attachment = content.attachment().ok_or_else(|| {
        definite_failure(
            "missing_network_attachment",
            "sandbox provision requires one compiled attachment",
        )
    })?;
    let plan_id = content.identity().plan_id();
    let mut listeners = Vec::with_capacity(content.listeners().len());
    for blueprint in content.listeners() {
        let binding = spec
            .port_bindings
            .iter()
            .find(|binding| binding.name == blueprint.name())
            .ok_or_else(|| {
                definite_failure(
                    "network_listener_missing_from_executable",
                    format!(
                        "compiled listener {:?} is absent from the executable sandbox spec",
                        blueprint.name()
                    ),
                )
            })?
            .clone();
        if binding.protocol != blueprint.protocol()
            || binding.host_address != blueprint.desired_host_address()
            || Some(binding.guest_port) != blueprint.guest_port()
            || !port_request_matches(binding.host_port, blueprint.port_request())
        {
            return Err(definite_failure(
                "network_listener_executable_mismatch",
                format!(
                    "compiled listener {:?} diverges from the executable sandbox binding",
                    blueprint.name()
                ),
            ));
        }
        let request = PortLeaseRequest::new(
            blueprint.port_lease_id().clone(),
            blueprint.listener_id().clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(content.identity().generation(), NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(blueprint.desired_host_address()),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                port_bind_target(blueprint.desired_host_address())?,
                port_exposure(blueprint.desired_host_address()),
                match blueprint.port_request() {
                    WorkloadNetworkPortRequestMode::Exact { port } => PortRequestMode::Exact(port),
                    WorkloadNetworkPortRequestMode::ProviderAssigned => {
                        PortRequestMode::ProviderAssigned
                    }
                },
            ),
        )
        .with_plan_id(plan_id.clone());
        listeners.push(SandboxProvisionListener::new(
            blueprint.endpoint_id().clone(),
            blueprint.listener_id().clone(),
            binding,
            request,
        ));
    }
    if spec.port_bindings.len() != listeners.len() {
        return Err(definite_failure(
            "network_listener_set_mismatch",
            "executable sandbox bindings contain a listener absent from the compiled plan",
        ));
    }
    let endpoint_identities = content.listeners().iter().map(|listener| {
        SandboxProvisionEndpointIdentity::new(
            listener.listener_id().clone(),
            listener.endpoint_id().clone(),
        )
    });
    let dependency_listeners = content.dependency_listeners().iter().map(|dependency| {
        SandboxProvisionDependencyListener::new(
            dependency.listener_id().clone(),
            dependency.name(),
            dependency.provider_id().clone(),
        )
    });
    SandboxProvisionNetworkPlan::new(
        compiled_network_plan.plan().clone(),
        spec.tenant_id.clone(),
        content.identity().generation(),
        attachment.attachment_id().clone(),
        endpoint_identities,
        listeners,
        dependency_listeners,
    )
    .map_err(|error| definite_failure("invalid_sandbox_network_plan", error.to_string()))
}

fn port_request_matches(host_port: u16, request: WorkloadNetworkPortRequestMode) -> bool {
    match request {
        WorkloadNetworkPortRequestMode::Exact { port } => host_port == port.get(),
        WorkloadNetworkPortRequestMode::ProviderAssigned => host_port == 0,
    }
}

fn port_bind_target(address: IpAddr) -> Result<PortBindTarget, ProviderProvisionEffectObservation> {
    match address {
        IpAddr::V4(address) if address == Ipv4Addr::UNSPECIFIED => {
            Ok(PortBindTarget::ipv4_wildcard())
        }
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) if address == Ipv6Addr::UNSPECIFIED => {
            Ok(PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown))
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(|error| definite_failure("invalid_listener_address", error.to_string())),
    }
}

fn port_exposure(address: IpAddr) -> PortExposure {
    match address {
        address if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        _ => PortExposure::Public,
    }
}

fn phase_result(
    result: Result<SandboxProvisionPhaseObservation, SandboxError>,
) -> ProviderProvisionEffectObservation {
    match result {
        Ok(SandboxProvisionPhaseObservation::Succeeded { evidence }) => {
            ProviderProvisionEffectObservation::Succeeded { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::Absent { evidence }) => {
            ProviderProvisionEffectObservation::Absent { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::InProgress { evidence }) => {
            ProviderProvisionEffectObservation::InProgress { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::Ambiguous { evidence }) => {
            ProviderProvisionEffectObservation::Ambiguous { evidence }
        }
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure("sandbox_phase_rejected", error.to_string())
        }
        Err(error) => ProviderProvisionEffectObservation::Ambiguous {
            evidence: error.to_string().into_bytes(),
        },
    }
}

fn handle_result(
    result: Result<nimbus_sandbox::SandboxHandle, SandboxError>,
) -> ProviderProvisionEffectObservation {
    match result {
        Ok(handle) => match serde_json::to_vec(&handle) {
            Ok(evidence) => ProviderProvisionEffectObservation::Succeeded { evidence },
            Err(error) => ProviderProvisionEffectObservation::Ambiguous {
                evidence: error.to_string().into_bytes(),
            },
        },
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure("sandbox_phase_rejected", error.to_string())
        }
        Err(error) => ProviderProvisionEffectObservation::Ambiguous {
            evidence: error.to_string().into_bytes(),
        },
    }
}

fn optional_handle_result(
    result: Result<Option<nimbus_sandbox::SandboxHandle>, SandboxError>,
) -> ProviderProvisionEffectObservation {
    match result {
        Ok(Some(handle)) => handle_result(Ok(handle)),
        Ok(None) => ProviderProvisionEffectObservation::Absent {
            evidence: b"sandbox phase is absent".to_vec(),
        },
        Err(error) => ProviderProvisionEffectObservation::Ambiguous {
            evidence: error.to_string().into_bytes(),
        },
    }
}

fn definite_failure(
    code: &str,
    evidence: impl Into<Vec<u8>>,
) -> ProviderProvisionEffectObservation {
    ProviderProvisionEffectObservation::DefiniteFailure {
        code: code.to_owned(),
        evidence: evidence.into(),
    }
}

fn validate_execution_observation_request(
    request: &WorkloadExecutionObservationRequest,
    backend: SandboxBackendKind,
) -> Option<(SandboxId, nimbus_sandbox::SandboxExecutionAttemptId)> {
    let spec = decode_sandbox_spec(request.executable()).ok()?;
    if spec.backend != backend
        || spec.tenant_id != *request.key().tenant_id()
        || request.source().execution_provider_id() != &sandbox_execution_provider_id(backend)
    {
        return None;
    }
    Some((
        SandboxId::new(request.execution().execution_id().as_str()),
        nimbus_sandbox::SandboxExecutionAttemptId::new(
            request.execution().attempt_id().to_string(),
        )
        .ok()?,
    ))
}

trait ExactNetworkReservationInspection {
    fn inspect_exact_network_reservation(
        &self,
        sandbox_id: &SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<Option<nimbus_sandbox::SandboxHandle>, SandboxError>;
}

impl ExactNetworkReservationInspection for ContainerSandboxBackend {
    fn inspect_exact_network_reservation(
        &self,
        sandbox_id: &SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<Option<nimbus_sandbox::SandboxHandle>, SandboxError> {
        self.inspect_provision_network_reservation(sandbox_id, execution_attempt_id, network_plan)
    }
}

impl ExactNetworkReservationInspection for KrunSandboxBackend {
    fn inspect_exact_network_reservation(
        &self,
        sandbox_id: &SandboxId,
        execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<Option<nimbus_sandbox::SandboxHandle>, SandboxError> {
        self.inspect_provision_network_reservation(sandbox_id, execution_attempt_id, network_plan)
    }
}

/// Real Container substitution for attachment and execution capabilities.
pub struct ContainerProvisionAdapter {
    pub(super) backend: Arc<ContainerSandboxBackend>,
    phases: ProviderProvisionPhaseAdapter,
    pub(super) restart_phases: ProviderRestartPhaseAdapter,
}

impl ContainerProvisionAdapter {
    pub fn new(backend: Arc<ContainerSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        let phases = ProviderProvisionPhaseAdapter::new(journal.clone());
        let restart_phases = ProviderRestartPhaseAdapter::new(journal);
        Ok(Self {
            backend,
            phases,
            restart_phases,
        })
    }

    fn validated(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> Result<ValidatedSandboxProvisionCommand, ProviderProvisionEffectObservation> {
        validate_sandbox_provision_command(command, SandboxBackendKind::Container)
    }
}

/// Real Krun substitution for attachment and execution capabilities.
pub struct KrunProvisionAdapter {
    pub(super) backend: Arc<KrunSandboxBackend>,
    phases: ProviderProvisionPhaseAdapter,
    pub(super) restart_phases: ProviderRestartPhaseAdapter,
}

impl KrunProvisionAdapter {
    pub fn new(backend: Arc<KrunSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        let phases = ProviderProvisionPhaseAdapter::new(journal.clone());
        let restart_phases = ProviderRestartPhaseAdapter::new(journal);
        Ok(Self {
            backend,
            phases,
            restart_phases,
        })
    }

    fn validated(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> Result<ValidatedSandboxProvisionCommand, ProviderProvisionEffectObservation> {
        validate_sandbox_provision_command(command, SandboxBackendKind::Krun)
    }
}

macro_rules! impl_sandbox_capabilities {
    ($adapter:ty) => {
        impl NetworkReservationCapability for $adapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.execute(command, || match validated {
                        Ok(validated) => handle_result(self.backend.reserve_provision_network(
                            validated.spec,
                            validated.sandbox_id,
                            validated.execution_attempt_id,
                            validated.network_plan,
                        )),
                        Err(error) => error,
                    })
                })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => {
                            optional_handle_result(self.backend.inspect_exact_network_reservation(
                                &validated.sandbox_id,
                                &validated.execution_attempt_id,
                                &validated.network_plan,
                            ))
                        }
                        Err(error) => error,
                    })
                })
            }
        }

        impl WorkloadPreparationCapability for $adapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.execute(command, || match validated {
                        Ok(validated) => handle_result(
                            self.backend
                                .prepare_provision_workload(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => optional_handle_result(
                            self.backend
                                .inspect_provision_preparation(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }
        }

        impl NetworkAttachmentCapability for $adapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.execute(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend.attach_provision_network(
                                &validated.sandbox_id,
                                &validated.execution_attempt_id,
                            ),
                        ),
                        Err(error) => error,
                    })
                })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend
                                .inspect_provision_network_attachment(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }
        }

        impl WorkloadActivationPrerequisiteCapability for $adapter {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend
                                .inspect_provision_activation_prerequisites(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }
        }

        impl WorkloadActivationCapability for $adapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.execute(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend
                                .activate_provision_workload(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend
                                .inspect_provision_workload_activation(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }
        }

        impl WorkloadReadinessCapability for $adapter {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move {
                    let validated = self.validated(command);
                    self.phases.inspect(command, || match validated {
                        Ok(validated) => phase_result(
                            self.backend
                                .inspect_provision_workload_readiness(
                                    &validated.sandbox_id,
                                    &validated.execution_attempt_id,
                                ),
                        ),
                        Err(error) => error,
                    })
                })
            }
        }

        impl WorkloadExecutionObservationCapability for $adapter {
            fn observe<'a>(
                &'a self,
                request: &'a WorkloadExecutionObservationRequest,
            ) -> WorkloadExecutionObservationFuture<'a> {
                Box::pin(async move {
                    let Some((sandbox_id, expected_attempt_id)) =
                        validate_execution_observation_request(request, self.backend.kind())
                    else {
                        return WorkloadProviderObservation::Ambiguous;
                    };
                    match self.backend.inspect(&sandbox_id).await {
                        Ok(Some(inspection))
                            if matches!(
                                &inspection.execution_attempt,
                                nimbus_sandbox::SandboxExecutionAttemptObservation::Exact(
                                    observed
                                ) if observed == &expected_attempt_id
                            ) =>
                        {
                            WorkloadProviderObservation::Present(inspection)
                        }
                        Ok(Some(_)) => WorkloadProviderObservation::Ambiguous,
                        Ok(None) | Err(SandboxError::NotFound { .. }) => {
                            WorkloadProviderObservation::Absent
                        }
                        Err(_) => WorkloadProviderObservation::Ambiguous,
                    }
                })
            }
        }
    };
}

impl_sandbox_capabilities!(ContainerProvisionAdapter);
impl_sandbox_capabilities!(KrunProvisionAdapter);

#[cfg(test)]
pub(crate) mod tests {
    use nimbus_core::TenantId;
    use nimbus_network::{
        NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
        NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
        NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
        NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
        NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
        NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
        NetworkSovereigntyRequirements, PortProtocol,
    };
    use nimbus_sandbox::backends::container::ContainerSandboxBackendConfig;
    use nimbus_sandbox::backends::krun::KrunSandboxBackendConfig;
    use nimbus_sandbox::{
        SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, sandbox_network_plan_requirements,
    };
    use nimbus_tenant::{
        TenantIsolationContext, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
        WorkloadAttributes, WorkloadLocation,
    };
    use nimbus_workloads::{
        NodeIdentity, WorkloadActivationIntent, WorkloadProvisionInspectionResult,
        WorkloadProvisionSourceGeneration, WorkloadProvisionSourceResourceVersion,
        WorkloadPublicationIntent, WorkloadSagaPhase, WorkloadSagaRecord,
    };

    use super::*;
    use crate::workload_provision_composition::{
        WorkloadProvisionCompositionInput, WorkloadProvisionSourceSnapshot,
        compose_workload_provision,
    };
    use crate::workload_saga::{
        NetworkAttachmentProvisionCapabilities, WorkloadExecutionProvisionCapabilities,
        WorkloadProvisionCapabilityRegistry,
    };

    fn lifecycle() -> NetworkLifecycleCapabilitySet {
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ])
    }

    fn provider_realm(
        backend: SandboxBackendKind,
    ) -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
        let requirements = sandbox_network_plan_requirements(backend);
        let ingress_key = format!("fixture-{backend:?}-ingress");
        let ingress_provider = NetworkProviderId::for_registration_key(&ingress_key);
        let attachment = NetworkAttachmentProviderRegistration::new(
            requirements.required_attachment_provider_id().clone(),
            requirements.capability_requirements().attachment().clone(),
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            lifecycle(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        );
        let ingress = NetworkIngressProviderRegistration::new(
            ingress_provider.clone(),
            NetworkEndpointCapabilitySet::new(
                [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
                [NetworkBindRealmKind::Host],
                [NetworkExposure::Loopback, NetworkExposure::Private],
                [PortProtocol::Tcp],
                [
                    NetworkPortAssignmentMode::Exact,
                    NetworkPortAssignmentMode::ProviderAssigned,
                ],
            ),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            lifecycle(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        );
        let selection = NetworkCapabilitySelection::new(
            requirements.required_attachment_provider_id().clone(),
            ingress_provider,
        );
        (
            NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
                .expect("fixture provider realm should validate"),
            selection,
        )
    }

    pub(crate) fn composed_record_with_rootfs(
        backend: SandboxBackendKind,
        rootfs: &std::path::Path,
    ) -> WorkloadSagaRecord {
        let label = match backend {
            SandboxBackendKind::Container => "container",
            SandboxBackendKind::Krun => "krun",
        };
        let tenant_id =
            TenantId::new(format!("tenant-{label}")).expect("fixture tenant should validate");
        let local_node =
            NodeIdentity::new(format!("node-{label}")).expect("fixture node should validate");
        let decision = TenantIsolationContext::system(
            tenant_id.clone(),
            "sandbox-adapter-behavioral-substitution",
        )
        .with_deployment_generation(7)
        .with_workload_location(WorkloadLocation::new().with_node_id(local_node.as_str()))
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox(label)
                    .with_sandbox_id(format!("sandbox-{label}"))
                    .with_sandbox_backend(backend),
            )
            .with_services(TenantServiceGrantPolicyDecision::new(std::iter::empty::<
                String,
            >())),
        )
        .expect("fixture decision should admit");
        let spec = SandboxSpec::new(
            tenant_id,
            SandboxOwnerSpec::standalone_named(label),
            backend,
            SandboxRootSpec::rootfs(rootfs),
            SandboxProcessSpec::new(["/bin/true"]),
        );
        let (registry, selection) = provider_realm(backend);
        let source_version = WorkloadProvisionSourceResourceVersion::new("source-v1")
            .expect("fixture source version should validate");
        let execution_provider_id = sandbox_execution_provider_id(backend);
        let stable_resource_id = format!("sandbox-{label}");
        let composed = compose_workload_provision(WorkloadProvisionCompositionInput {
            decision: &decision,
            local_node: &local_node,
            source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
                stable_resource_id: &stable_resource_id,
                profile: label,
                source_generation: WorkloadProvisionSourceGeneration::new(1),
                resource_version: &source_version,
                sandbox_spec: &spec,
            },
            execution_provider_id: &execution_provider_id,
            capability_selection: &selection,
            capability_registry: &registry,
            sovereignty: NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
            endpoint_semantics: &[],
            activation: WorkloadActivationIntent::ActivateWhenAttached,
            publication: WorkloadPublicationIntent::Withheld,
        })
        .expect("valid backend-specific source should compose");
        let (key, intent) = composed.into_parts();
        WorkloadSagaRecord::new(key, intent).expect("fixture workload saga should validate")
    }

    fn composed_record(backend: SandboxBackendKind) -> WorkloadSagaRecord {
        composed_record_with_rootfs(backend, std::path::Path::new("/fixture/rootfs"))
    }

    fn record_at_phase(
        backend: SandboxBackendKind,
        target: WorkloadSagaPhase,
        rootfs: &std::path::Path,
    ) -> WorkloadSagaRecord {
        let mut record = composed_record_with_rootfs(backend, rootfs);
        if target == WorkloadSagaPhase::IntentCommitted {
            return record;
        }
        for phase in [
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated,
        ] {
            record = crate::workload_saga::test_support::confirmed_provision(&record);
            assert_eq!(record.phase(), phase, "fixture should reach {phase:?}");
            if phase == target {
                return record;
            }
        }
        panic!("unsupported sandbox adapter fixture phase {target:?}");
    }

    async fn commands_for_capability_substitution(
        backend: SandboxBackendKind,
        rootfs: &std::path::Path,
    ) -> [ConfirmedWorkloadProvisionCommand; 6] {
        let reserve = crate::workload_saga::provision_provider::tests::command_for_record(
            record_at_phase(backend, WorkloadSagaPhase::IntentCommitted, rootfs),
        )
        .await;
        let prepare = crate::workload_saga::provision_provider::tests::command_for_record(
            record_at_phase(backend, WorkloadSagaPhase::NetworkReserved, rootfs),
        )
        .await;
        let attach = crate::workload_saga::provision_provider::tests::command_for_record(
            record_at_phase(backend, WorkloadSagaPhase::WorkloadPrepared, rootfs),
        )
        .await;
        let prerequisites = crate::workload_saga::provision_provider::tests::command_for_record(
            record_at_phase(backend, WorkloadSagaPhase::NetworkAttached, rootfs),
        )
        .await;
        let activate =
            crate::workload_saga::provision_provider::tests::activation_command_for_record(
                record_at_phase(backend, WorkloadSagaPhase::NetworkAttached, rootfs),
            )
            .await;
        let readiness = crate::workload_saga::provision_provider::tests::command_for_record(
            record_at_phase(backend, WorkloadSagaPhase::WorkloadActivated, rootfs),
        )
        .await;
        [reserve, prepare, attach, prerequisites, activate, readiness]
    }

    async fn assert_real_adapter_capabilities<Adapter>(
        adapter: &Adapter,
        backend: SandboxBackendKind,
        rootfs: &std::path::Path,
    ) where
        Adapter: NetworkReservationCapability
            + WorkloadPreparationCapability
            + NetworkAttachmentCapability
            + WorkloadActivationPrerequisiteCapability
            + WorkloadActivationCapability
            + WorkloadReadinessCapability,
    {
        let [reserve, prepare, attach, prerequisites, activate, readiness] =
            commands_for_capability_substitution(backend, rootfs).await;
        assert!(
            validate_sandbox_provision_command(&reserve, backend).is_ok(),
            "{backend:?} reserve fixture must authenticate"
        );

        for (label, result) in [
            (
                "prepare",
                WorkloadPreparationCapability::execute(adapter, &prepare).await,
            ),
            (
                "attach",
                NetworkAttachmentCapability::execute(adapter, &attach).await,
            ),
        ] {
            assert!(
                matches!(
                    result,
                    WorkloadProvisionInspectionResult::DefiniteFailure { .. }
                ),
                "{backend:?} {label} must reach the real backend and reject a missing prerequisite: {result:?}"
            );
        }
        assert!(matches!(
            WorkloadActivationPrerequisiteCapability::inspect(adapter, &prerequisites).await,
            WorkloadProvisionInspectionResult::Absent { .. }
        ));
        assert!(matches!(
            WorkloadReadinessCapability::inspect(adapter, &readiness).await,
            WorkloadProvisionInspectionResult::Absent { .. }
        ));
        let activation = WorkloadActivationCapability::execute(adapter, &activate).await;
        assert!(
            matches!(
                activation,
                WorkloadProvisionInspectionResult::DefiniteFailure { .. }
                    | WorkloadProvisionInspectionResult::Ambiguous { .. }
            ),
            "{backend:?} activation must reach the real backend without treating missing runtime state as success or retryable absence: {activation:?}"
        );

        let reserved = NetworkReservationCapability::execute(adapter, &reserve).await;
        assert!(
            matches!(
                reserved,
                WorkloadProvisionInspectionResult::Succeeded { .. }
            ),
            "{backend:?} reservation execute must cross the real provider seam: {reserved:?}"
        );
        assert_eq!(
            NetworkReservationCapability::inspect(adapter, &reserve).await,
            reserved,
            "{backend:?} reservation inspection must adopt the exact provider result"
        );
    }

    #[tokio::test]
    async fn real_container_adapter_substitutes_behaviorally_for_narrow_capabilities() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let rootfs = root.path().join("rootfs");
        std::fs::create_dir(&rootfs).expect("fixture rootfs should exist");
        let adapter = Arc::new(
            ContainerProvisionAdapter::new(Arc::new(ContainerSandboxBackend::new(
                ContainerSandboxBackendConfig::under_root(root.path()),
            )))
            .expect("container provider journal should open"),
        );
        let _registry = WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                sandbox_network_plan_requirements(SandboxBackendKind::Container)
                    .required_attachment_provider_id()
                    .clone(),
                adapter.clone(),
            )],
            [WorkloadExecutionProvisionCapabilities::new(
                sandbox_execution_provider_id(SandboxBackendKind::Container),
                adapter.clone(),
            )],
            [],
        )
        .expect("real container adapter should earn every registered narrow capability");
        assert_real_adapter_capabilities(adapter.as_ref(), SandboxBackendKind::Container, &rootfs)
            .await;
    }

    #[tokio::test]
    async fn real_krun_adapter_substitutes_behaviorally_for_narrow_capabilities() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let rootfs = root.path().join("rootfs");
        std::fs::create_dir(&rootfs).expect("fixture rootfs should exist");
        let adapter = Arc::new(
            KrunProvisionAdapter::new(Arc::new(KrunSandboxBackend::new(
                KrunSandboxBackendConfig::under_root(root.path()),
            )))
            .expect("krun provider journal should open"),
        );
        let _registry = WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                sandbox_network_plan_requirements(SandboxBackendKind::Krun)
                    .required_attachment_provider_id()
                    .clone(),
                adapter.clone(),
            )],
            [WorkloadExecutionProvisionCapabilities::new(
                sandbox_execution_provider_id(SandboxBackendKind::Krun),
                adapter.clone(),
            )],
            [],
        )
        .expect("real krun adapter should earn every registered narrow capability");
        assert_real_adapter_capabilities(adapter.as_ref(), SandboxBackendKind::Krun, &rootfs).await;
    }

    #[tokio::test]
    async fn concrete_sandbox_adapters_reject_crossed_backend_commands_before_inspection() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let container = ContainerProvisionAdapter::new(Arc::new(ContainerSandboxBackend::new(
            ContainerSandboxBackendConfig::under_root(root.path().join("container")),
        )))
        .expect("container provider journal should open");
        let krun = KrunProvisionAdapter::new(Arc::new(KrunSandboxBackend::new(
            KrunSandboxBackendConfig::under_root(root.path().join("krun")),
        )))
        .expect("krun provider journal should open");
        let container_command =
            crate::workload_saga::provision_provider::tests::command_for_record(composed_record(
                SandboxBackendKind::Container,
            ))
            .await;
        let krun_command = crate::workload_saga::provision_provider::tests::command_for_record(
            composed_record(SandboxBackendKind::Krun),
        )
        .await;

        let krun_error = match krun.validated(&container_command) {
            Err(error) => error,
            Ok(_) => panic!("Krun must reject a Container executable"),
        };
        let container_error = match container.validated(&krun_command) {
            Err(error) => error,
            Ok(_) => panic!("Container must reject a Krun executable"),
        };
        for error in [krun_error, container_error] {
            assert!(matches!(
                error,
                ProviderProvisionEffectObservation::DefiniteFailure { ref code, .. }
                    if code == "execution_backend_mismatch"
            ));
        }
        assert!(matches!(
            NetworkReservationCapability::inspect(&krun, &container_command).await,
            WorkloadProvisionInspectionResult::DefiniteFailure { .. }
        ));
        assert!(matches!(
            NetworkReservationCapability::inspect(&container, &krun_command).await,
            WorkloadProvisionInspectionResult::DefiniteFailure { .. }
        ));
    }
}
