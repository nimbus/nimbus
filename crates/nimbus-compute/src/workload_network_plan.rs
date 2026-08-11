//! Pure compilation of admitted workload connectivity intent.
//!
//! This module owns composition, not effects. It correlates an already-issued
//! tenant decision with one closed source shape and produces portable desired
//! state. It does not persist, allocate, bind, start, inspect, or reconcile.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU16;

use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkBindRealmKind,
    NetworkCapabilityDimension, NetworkCapabilityRegistry, NetworkCapabilityRequirements,
    NetworkCapabilitySelection, NetworkCapabilitySelectionError, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkForwardingFeature,
    NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkLifecycleRequirements, NetworkManagementMode, NetworkPortAssignmentMode,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyRequirements,
    NetworkTlsBehavior, PortProtocol,
};
use nimbus_sandbox::{SandboxBackendKind, SandboxOwnerSpec, SandboxSpec};
use nimbus_tenant::{TenantIsolationDecision, WorkloadKind};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, TenantWorkloadSpec, WorkloadActivationIntent,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkDependencyListenerBlueprint,
    WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior,
    WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent, WorkloadNetworkPlanError,
    WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode, WorkloadNetworkRouteBlueprint,
    WorkloadPublicationIntent,
};
use thiserror::Error;

const DEFAULT_ATTACHMENT_NAME: &str = "default";
const EGRESS_PEP_LISTENER_NAME: &str = "egress-pep";

/// Closed admitted source shapes accepted by [`WorkloadNetworkPlanCompiler`].
#[derive(Debug, Clone, Copy)]
pub enum AdmittedWorkloadNetworkSource<'source> {
    /// A workload generation with no connectivity resource to realize.
    Empty,
    /// One standalone sandbox resource admitted for this workload generation.
    Sandbox {
        stable_resource_id: &'source str,
        profile: &'source str,
        generation: u64,
        sandbox_spec: &'source SandboxSpec,
    },
    /// One sandbox-backed service admitted for this workload generation.
    SandboxBackedService {
        service_name: &'source str,
        service_generation: u64,
        sandbox_spec: &'source SandboxSpec,
    },
}

impl<'source> AdmittedWorkloadNetworkSource<'source> {
    fn sandbox_spec(self) -> Option<&'source SandboxSpec> {
        match self {
            Self::Empty => None,
            Self::Sandbox { sandbox_spec, .. }
            | Self::SandboxBackedService { sandbox_spec, .. } => Some(sandbox_spec),
        }
    }
}

/// Explicit endpoint semantics correlated by one sandbox listener name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadNetworkEndpointSemanticsInput<'input> {
    listener_name: &'input str,
    forwarding: WorkloadNetworkForwardingBehavior,
    tls: NetworkTlsBehavior,
}

impl<'input> WorkloadNetworkEndpointSemanticsInput<'input> {
    /// Construct exact semantics for one source-owned listener name.
    pub const fn new(
        listener_name: &'input str,
        forwarding: WorkloadNetworkForwardingBehavior,
        tls: NetworkTlsBehavior,
    ) -> Self {
        Self {
            listener_name,
            forwarding,
            tls,
        }
    }

    /// Source-owned listener name.
    pub const fn listener_name(&self) -> &'input str {
        self.listener_name
    }

    /// Canonical portable semantics.
    pub const fn semantics(&self) -> WorkloadNetworkEndpointSemantics {
        WorkloadNetworkEndpointSemantics::new(self.forwarding, self.tls)
    }
}

/// A pure admitted-intent compilation failure.
#[derive(Debug, Error)]
pub enum WorkloadNetworkPlanCompileError {
    #[error("the tenant decision does not carry a deployment generation")]
    MissingDeploymentGeneration,
    #[error("the tenant decision does not carry an assigned node")]
    MissingNodeAssignment,
    #[error("the admitted workload projection is invalid: {message}")]
    InvalidWorkloadProjection { message: String },
    #[error("network source `{candidate}` does not match admitted workload kind `{admitted}`")]
    WorkloadKindMismatch {
        admitted: &'static str,
        candidate: &'static str,
    },
    #[error("network source tenant {candidate} does not match admitted tenant {admitted}")]
    TenantMismatch { admitted: String, candidate: String },
    #[error("network source name `{candidate}` does not match admitted workload name `{admitted}`")]
    WorkloadNameMismatch { admitted: String, candidate: String },
    #[error("network source generation {candidate} does not match admitted generation {admitted}")]
    GenerationMismatch { admitted: u64, candidate: u64 },
    #[error("network source backend {candidate:?} does not match admitted backend {admitted:?}")]
    SandboxBackendMismatch {
        admitted: Option<SandboxBackendKind>,
        candidate: SandboxBackendKind,
    },
    #[error("standalone sandbox source is owned by service `{service_name}`")]
    StandaloneSandboxOwnedByService { service_name: String },
    #[error("service `{service_name}` sandbox owner is {owner}")]
    ServiceSandboxOwnerMismatch { service_name: String, owner: String },
    #[error("sandbox source ID `{candidate}` does not match admitted ID `{admitted}`")]
    SandboxResourceIdMismatch { admitted: String, candidate: String },
    #[error("sandbox source `{candidate}` is not a concrete non-empty name")]
    InvalidSourceName { candidate: String },
    #[error("tenant admission did not authorize the sandbox egress source: {message}")]
    SandboxEgressMismatch { message: String },
    #[error("an explicit empty network source still carries admitted routes")]
    EmptySourceHasRoutes,
    #[error("an explicit empty network source cannot select provider capabilities")]
    EmptySourceHasCapabilitySelection,
    #[error("an explicit empty network source cannot carry endpoint semantics")]
    EmptySourceHasEndpointSemantics,
    #[error("an attachment-bearing network source requires an exact capability selection")]
    MissingCapabilitySelection,
    #[error(
        "selected attachment provider {selected} does not match source-owned provider {required}"
    )]
    AttachmentProviderMismatch {
        required: NetworkProviderId,
        selected: NetworkProviderId,
    },
    #[error("admitted sovereignty relaxes source-owned dimensions {dimensions:?}")]
    SourceSovereigntyRelaxation {
        dimensions: Vec<NetworkCapabilityDimension>,
    },
    #[error("listener `{listener_name}` uses reserved internal resource name `{reserved}`")]
    ReservedListenerName {
        listener_name: String,
        reserved: &'static str,
    },
    #[error("duplicate endpoint semantics for listener `{listener_name}`")]
    DuplicateEndpointSemantics { listener_name: String },
    #[error("missing endpoint semantics for listener `{listener_name}`")]
    MissingEndpointSemantics { listener_name: String },
    #[error("unexpected endpoint semantics for listener `{listener_name}`")]
    UnexpectedEndpointSemantics { listener_name: String },
    #[error("forwarding behavior must match guest port shape")]
    ForwardingBehaviorMismatch,
    #[error("TLS behavior must match listener protocol")]
    TlsBehaviorMismatch,
    #[error("publish-when-ready requires at least one admitted listener")]
    PublicationRequiresListener,
    #[error("network capability selection failed: {0}")]
    CapabilitySelection(#[from] NetworkCapabilitySelectionError),
    #[error("portable workload network plan is invalid: {0}")]
    PortablePlan(#[from] WorkloadNetworkPlanError),
}

/// The single pure composition authority for admitted workload network plans.
#[derive(Debug, Default, Clone, Copy)]
pub struct WorkloadNetworkPlanCompiler;

impl WorkloadNetworkPlanCompiler {
    /// Compile one admitted workload generation into exact portable desired
    /// state without invoking any store, allocator, provider, or workload
    /// lifecycle effect.
    #[allow(clippy::too_many_arguments)]
    pub fn compile(
        &self,
        decision: &TenantIsolationDecision,
        source: AdmittedWorkloadNetworkSource<'_>,
        capability_selection: Option<&NetworkCapabilitySelection>,
        capability_registry: &NetworkCapabilityRegistry,
        sovereignty_requirements: NetworkSovereigntyRequirements,
        endpoint_semantics: &[WorkloadNetworkEndpointSemanticsInput<'_>],
        activation: WorkloadActivationIntent,
        publication: WorkloadPublicationIntent,
    ) -> Result<CompiledWorkloadNetworkPlan, WorkloadNetworkPlanCompileError> {
        let identity = decision.workload_identity();
        let admitted_generation = identity
            .deployment_generation()
            .ok_or(WorkloadNetworkPlanCompileError::MissingDeploymentGeneration)?;
        if identity.node_id().is_none() {
            return Err(WorkloadNetworkPlanCompileError::MissingNodeAssignment);
        }

        let workload = TenantWorkloadSpec::from_decision(decision).map_err(|error| {
            WorkloadNetworkPlanCompileError::InvalidWorkloadProjection {
                message: error.to_string(),
            }
        })?;
        debug_assert_eq!(workload.generation().as_u64(), admitted_generation);
        debug_assert!(workload.assigned_node_id().is_some());

        validate_source(decision, source, admitted_generation)?;

        let workload_key = network_workload_incarnation_key(decision, source);
        let plan_identity = WorkloadNetworkPlanIdentity::new(
            decision.tenant_id().clone(),
            workload_key,
            NetworkResourceGeneration::new(workload.generation().as_u64()),
        )?;
        let routes = compile_routes(decision, &plan_identity)?;
        let (attachment, listeners, source_requirements) = match source.sandbox_spec() {
            Some(spec) => {
                let projection = nimbus_sandbox::sandbox_network_plan_requirements(spec.backend);
                let attachment = WorkloadNetworkAttachmentBlueprint::new(
                    &plan_identity,
                    DEFAULT_ATTACHMENT_NAME,
                )?;
                let listeners = compile_listeners(&plan_identity, spec, endpoint_semantics)?;
                (Some(attachment), listeners, Some(projection))
            }
            None => {
                if !endpoint_semantics.is_empty() {
                    return Err(WorkloadNetworkPlanCompileError::EmptySourceHasEndpointSemantics);
                }
                (None, Vec::new(), None)
            }
        };
        if publication == WorkloadPublicationIntent::PublishWhenReady && listeners.is_empty() {
            return Err(WorkloadNetworkPlanCompileError::PublicationRequiresListener);
        }

        let (selection, selection_evidence, requirements, dependency_listeners) =
            match source_requirements {
                None => {
                    if !routes.is_empty() {
                        return Err(WorkloadNetworkPlanCompileError::EmptySourceHasRoutes);
                    }
                    if capability_selection.is_some() {
                        return Err(
                            WorkloadNetworkPlanCompileError::EmptySourceHasCapabilitySelection,
                        );
                    }
                    (
                        None,
                        None,
                        empty_requirements(sovereignty_requirements.clone()),
                        Vec::new(),
                    )
                }
                Some(source_requirements) => {
                    let selection = capability_selection
                        .ok_or(WorkloadNetworkPlanCompileError::MissingCapabilitySelection)?;
                    if selection.attachment_provider_id()
                        != source_requirements.required_attachment_provider_id()
                    {
                        return Err(
                            WorkloadNetworkPlanCompileError::AttachmentProviderMismatch {
                                required: source_requirements
                                    .required_attachment_provider_id()
                                    .clone(),
                                selected: selection.attachment_provider_id().clone(),
                            },
                        );
                    }
                    let requirements = aggregate_requirements(
                        source_requirements.capability_requirements(),
                        &listeners,
                        sovereignty_requirements.clone(),
                    )?;
                    let selected_bundle =
                        capability_registry.select_exact(selection, &requirements)?;
                    let selection_evidence = selected_bundle.selection_evidence();
                    let dependency_listeners = source_requirements
                        .requires_pep_readiness()
                        .then(|| {
                            WorkloadNetworkDependencyListenerBlueprint::new(
                                &plan_identity,
                                source_requirements.pep_listener_name(),
                                source_requirements.pep_provider_id().clone(),
                            )
                        })
                        .transpose()?
                        .into_iter()
                        .collect();
                    (
                        Some(selection.clone()),
                        Some(selection_evidence),
                        requirements,
                        dependency_listeners,
                    )
                }
            };

        let content = WorkloadNetworkPlanContent::new(
            plan_identity,
            requirements,
            selection,
            selection_evidence,
            attachment,
            routes,
            listeners,
            dependency_listeners,
            activation,
            publication,
        )?;
        CompiledWorkloadNetworkPlan::from_content(content).map_err(Into::into)
    }

    /// Compile the resource-free terminal successor of one admitted plan.
    ///
    /// Retirement advances only the desired network generation. The stable
    /// workload-incarnation identity and the complete admitted sovereignty
    /// baseline remain unchanged while all provider selections and desired
    /// connectivity resources are withdrawn.
    pub(crate) fn compile_terminal_empty_successor(
        &self,
        base: &CompiledWorkloadNetworkPlan,
        generation: NetworkResourceGeneration,
    ) -> Result<CompiledWorkloadNetworkPlan, WorkloadNetworkPlanCompileError> {
        let base = base.content();
        let identity = WorkloadNetworkPlanIdentity::new(
            base.identity().tenant_id().clone(),
            base.identity().workload_incarnation_key(),
            generation,
        )?;
        let content = WorkloadNetworkPlanContent::new(
            identity,
            empty_requirements(base.sovereignty_requirements().clone()),
            None,
            None,
            None,
            [],
            [],
            [],
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )?;
        CompiledWorkloadNetworkPlan::from_content(content).map_err(Into::into)
    }
}

fn network_workload_incarnation_key(
    decision: &TenantIsolationDecision,
    source: AdmittedWorkloadNetworkSource<'_>,
) -> String {
    let admitted_subject = decision.workload_identity().subject();
    match source {
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id, ..
        } => format!(
            "nimbus.network.workload-incarnation.v1:{}:{}:{}:{}",
            admitted_subject.len(),
            admitted_subject,
            stable_resource_id.len(),
            stable_resource_id
        ),
        AdmittedWorkloadNetworkSource::Empty
        | AdmittedWorkloadNetworkSource::SandboxBackedService { .. } => format!(
            "nimbus.network.workload-incarnation.v1:{}:{}",
            admitted_subject.len(),
            admitted_subject
        ),
    }
}

fn validate_source(
    decision: &TenantIsolationDecision,
    source: AdmittedWorkloadNetworkSource<'_>,
    admitted_generation: u64,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    let admitted_kind = decision.workload().kind();
    match source {
        AdmittedWorkloadNetworkSource::Empty => {
            if matches!(admitted_kind, WorkloadKind::Sandbox | WorkloadKind::Service) {
                return Err(WorkloadNetworkPlanCompileError::WorkloadKindMismatch {
                    admitted: admitted_kind.label(),
                    candidate: "empty",
                });
            }
        }
        AdmittedWorkloadNetworkSource::Sandbox {
            stable_resource_id,
            profile,
            generation,
            sandbox_spec,
        } => {
            require_kind(admitted_kind, WorkloadKind::Sandbox, "sandbox")?;
            validate_source_name(profile)?;
            require_name(decision, profile)?;
            require_generation(admitted_generation, generation)?;
            require_tenant(decision, sandbox_spec)?;
            require_backend(decision, sandbox_spec)?;
            match &sandbox_spec.owner {
                SandboxOwnerSpec::Standalone { .. } => {}
                SandboxOwnerSpec::Service { name } => {
                    return Err(
                        WorkloadNetworkPlanCompileError::StandaloneSandboxOwnedByService {
                            service_name: name.clone(),
                        },
                    );
                }
            }
            validate_source_name(stable_resource_id)?;
            let admitted_id = decision.workload().sandbox_id().unwrap_or_default();
            if admitted_id != stable_resource_id {
                return Err(WorkloadNetworkPlanCompileError::SandboxResourceIdMismatch {
                    admitted: admitted_id.to_owned(),
                    candidate: stable_resource_id.to_owned(),
                });
            }
            require_egress(decision, sandbox_spec)?;
        }
        AdmittedWorkloadNetworkSource::SandboxBackedService {
            service_name,
            service_generation,
            sandbox_spec,
        } => {
            require_kind(
                admitted_kind,
                WorkloadKind::Service,
                "sandbox_backed_service",
            )?;
            validate_source_name(service_name)?;
            require_name(decision, service_name)?;
            require_generation(admitted_generation, service_generation)?;
            require_tenant(decision, sandbox_spec)?;
            require_backend(decision, sandbox_spec)?;
            if sandbox_spec.service_name() != Some(service_name) {
                let owner = match &sandbox_spec.owner {
                    SandboxOwnerSpec::Service { name } => format!("service `{name}`"),
                    SandboxOwnerSpec::Standalone { .. } => "standalone".to_owned(),
                };
                return Err(
                    WorkloadNetworkPlanCompileError::ServiceSandboxOwnerMismatch {
                        service_name: service_name.to_owned(),
                        owner,
                    },
                );
            }
            require_egress(decision, sandbox_spec)?;
        }
    }
    Ok(())
}

fn require_kind(
    admitted: WorkloadKind,
    expected: WorkloadKind,
    source: &'static str,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    if admitted == expected {
        Ok(())
    } else {
        Err(WorkloadNetworkPlanCompileError::WorkloadKindMismatch {
            admitted: admitted.label(),
            candidate: source,
        })
    }
}

fn validate_source_name(value: &str) -> Result<(), WorkloadNetworkPlanCompileError> {
    if value.trim().is_empty() || value != value.trim() || value.contains(char::is_whitespace) {
        Err(WorkloadNetworkPlanCompileError::InvalidSourceName {
            candidate: value.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn require_name(
    decision: &TenantIsolationDecision,
    source: &str,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    let admitted = decision.workload().name();
    if admitted == source {
        Ok(())
    } else {
        Err(WorkloadNetworkPlanCompileError::WorkloadNameMismatch {
            admitted: admitted.to_owned(),
            candidate: source.to_owned(),
        })
    }
}

fn require_generation(admitted: u64, source: u64) -> Result<(), WorkloadNetworkPlanCompileError> {
    if admitted == source {
        Ok(())
    } else {
        Err(WorkloadNetworkPlanCompileError::GenerationMismatch {
            admitted,
            candidate: source,
        })
    }
}

fn require_tenant(
    decision: &TenantIsolationDecision,
    sandbox_spec: &SandboxSpec,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    if decision.tenant_id() == &sandbox_spec.tenant_id {
        Ok(())
    } else {
        Err(WorkloadNetworkPlanCompileError::TenantMismatch {
            admitted: decision.tenant_id().as_str().to_owned(),
            candidate: sandbox_spec.tenant_id.as_str().to_owned(),
        })
    }
}

fn require_backend(
    decision: &TenantIsolationDecision,
    sandbox_spec: &SandboxSpec,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    let admitted = decision.workload().sandbox_backend();
    if admitted == Some(sandbox_spec.backend) {
        Ok(())
    } else {
        Err(WorkloadNetworkPlanCompileError::SandboxBackendMismatch {
            admitted,
            candidate: sandbox_spec.backend,
        })
    }
}

fn require_egress(
    decision: &TenantIsolationDecision,
    sandbox_spec: &SandboxSpec,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    decision
        .network()
        .ensure_sandbox_egress_matches(sandbox_spec, "network plan compilation")
        .map_err(
            |error| WorkloadNetworkPlanCompileError::SandboxEgressMismatch {
                message: error.to_string(),
            },
        )
}

fn compile_routes(
    decision: &TenantIsolationDecision,
    identity: &WorkloadNetworkPlanIdentity,
) -> Result<Vec<WorkloadNetworkRouteBlueprint>, WorkloadNetworkPlanCompileError> {
    decision
        .network()
        .endpoints()
        .iter()
        .map(|endpoint| {
            WorkloadNetworkRouteBlueprint::new(
                identity,
                endpoint.service_name(),
                endpoint.endpoint_name(),
                endpoint.protocol(),
                endpoint.host(),
                endpoint.host_port(),
                endpoint.guest_port(),
            )
            .map_err(Into::into)
        })
        .collect()
}

fn compile_listeners(
    identity: &WorkloadNetworkPlanIdentity,
    sandbox_spec: &SandboxSpec,
    endpoint_semantics: &[WorkloadNetworkEndpointSemanticsInput<'_>],
) -> Result<Vec<WorkloadNetworkListenerBlueprint>, WorkloadNetworkPlanCompileError> {
    let mut semantics_by_name = BTreeMap::new();
    for semantics in endpoint_semantics {
        validate_source_name(semantics.listener_name())?;
        if semantics_by_name
            .insert(semantics.listener_name(), semantics.semantics())
            .is_some()
        {
            return Err(
                WorkloadNetworkPlanCompileError::DuplicateEndpointSemantics {
                    listener_name: semantics.listener_name().to_owned(),
                },
            );
        }
    }

    let listeners = sandbox_spec
        .port_bindings
        .iter()
        .map(|binding| {
            if binding.name == EGRESS_PEP_LISTENER_NAME {
                return Err(WorkloadNetworkPlanCompileError::ReservedListenerName {
                    listener_name: binding.name.clone(),
                    reserved: EGRESS_PEP_LISTENER_NAME,
                });
            }
            let port_request = NonZeroU16::new(binding.host_port).map_or(
                WorkloadNetworkPortRequestMode::ProviderAssigned,
                WorkloadNetworkPortRequestMode::exact,
            );
            let semantics = semantics_by_name
                .remove(binding.name.as_str())
                .ok_or_else(
                    || WorkloadNetworkPlanCompileError::MissingEndpointSemantics {
                        listener_name: binding.name.clone(),
                    },
                )?;
            let forwarding_matches = matches!(
                (semantics.forwarding(), Some(binding.guest_port)),
                (WorkloadNetworkForwardingBehavior::PortForwarded, Some(_))
            );
            if !forwarding_matches {
                return Err(WorkloadNetworkPlanCompileError::ForwardingBehaviorMismatch);
            }
            let tls_matches = matches!(
                (binding.protocol, semantics.tls()),
                (
                    EndpointProtocol::Tcp | EndpointProtocol::Http,
                    NetworkTlsBehavior::Disabled
                ) | (
                    EndpointProtocol::Https,
                    NetworkTlsBehavior::Passthrough | NetworkTlsBehavior::TerminateAtIngress
                )
            );
            if !tls_matches {
                return Err(WorkloadNetworkPlanCompileError::TlsBehaviorMismatch);
            }
            WorkloadNetworkListenerBlueprint::new(
                identity,
                &binding.name,
                binding.protocol,
                binding.host_address,
                port_request,
                semantics,
                Some(binding.guest_port),
            )
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(listener_name) = semantics_by_name.keys().next() {
        return Err(
            WorkloadNetworkPlanCompileError::UnexpectedEndpointSemantics {
                listener_name: (*listener_name).to_owned(),
            },
        );
    }
    Ok(listeners)
}

fn aggregate_requirements(
    source: &NetworkCapabilityRequirements,
    listeners: &[WorkloadNetworkListenerBlueprint],
    sovereignty: NetworkSovereigntyRequirements,
) -> Result<NetworkCapabilityRequirements, WorkloadNetworkPlanCompileError> {
    require_sovereignty_refinement(source.sovereignty(), &sovereignty)?;
    let mut address_families = source.endpoint().address_families().clone();
    let mut bind_realms = source.endpoint().bind_realms().clone();
    let mut exposures = source.endpoint().exposures().clone();
    let mut protocols = source.endpoint().protocols().clone();
    let mut port_assignment_modes = source.endpoint().port_assignment_modes().clone();
    let ingress_features = source.ingress().features().clone();
    let mut tls_behaviors = source.ingress().tls_behaviors().clone();
    let mut forwarding_features = source.forwarding().features().clone();
    let mut attachment_lifecycle_features = source.lifecycle().attachment().features().clone();
    let mut ingress_lifecycle_features = source.lifecycle().ingress().features().clone();

    for listener in listeners {
        address_families.insert(address_family(listener.desired_host_address()));
        bind_realms.insert(NetworkBindRealmKind::Host);
        exposures.insert(exposure(listener.desired_host_address()));
        protocols.insert(PortProtocol::Tcp);
        port_assignment_modes.insert(match listener.port_request() {
            WorkloadNetworkPortRequestMode::Exact { .. } => NetworkPortAssignmentMode::Exact,
            WorkloadNetworkPortRequestMode::ProviderAssigned => {
                NetworkPortAssignmentMode::ProviderAssigned
            }
        });
        if listener.endpoint_semantics().forwarding()
            == WorkloadNetworkForwardingBehavior::PortForwarded
        {
            forwarding_features.insert(NetworkForwardingFeature::PortForwarding);
        }
        tls_behaviors.insert(listener.endpoint_semantics().tls());
    }
    attachment_lifecycle_features.extend([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    ingress_lifecycle_features.extend([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
    ]);

    Ok(NetworkCapabilityRequirements::new(
        source.attachment().clone(),
        NetworkEndpointCapabilitySet::new(
            address_families,
            bind_realms,
            exposures,
            protocols,
            port_assignment_modes,
        ),
        NetworkIngressCapabilitySet::new(ingress_features).with_tls_behaviors(tls_behaviors),
        NetworkForwardingCapabilitySet::new(forwarding_features),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new(attachment_lifecycle_features),
            NetworkLifecycleCapabilitySet::new(ingress_lifecycle_features),
        ),
        sovereignty,
    ))
}

fn require_sovereignty_refinement(
    source: &NetworkSovereigntyRequirements,
    admitted: &NetworkSovereigntyRequirements,
) -> Result<(), WorkloadNetworkPlanCompileError> {
    let mut dimensions = Vec::with_capacity(3);
    if admitted.maximum_control_plane_locality() > source.maximum_control_plane_locality() {
        dimensions.push(NetworkCapabilityDimension::ControlPlaneLocality);
    }
    if !admitted
        .allowed_external_dependencies()
        .is_subset(source.allowed_external_dependencies())
    {
        dimensions.push(NetworkCapabilityDimension::ExternalDependency);
    }
    if source.offline_restart_required() && !admitted.offline_restart_required() {
        dimensions.push(NetworkCapabilityDimension::OfflineRestart);
    }
    if !dimensions.is_empty() {
        return Err(WorkloadNetworkPlanCompileError::SourceSovereigntyRelaxation { dimensions });
    }
    Ok(())
}

fn empty_requirements(
    sovereignty: NetworkSovereigntyRequirements,
) -> NetworkCapabilityRequirements {
    NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        sovereignty,
    )
}

fn address_family(address: IpAddr) -> NetworkAddressFamily {
    match address {
        IpAddr::V4(_) => NetworkAddressFamily::Ipv4,
        IpAddr::V6(_) => NetworkAddressFamily::Ipv6,
    }
}

fn exposure(address: IpAddr) -> NetworkExposure {
    if address.is_loopback() {
        return NetworkExposure::Loopback;
    }
    match address {
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            NetworkExposure::Private
        }
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            NetworkExposure::Private
        }
        IpAddr::V4(_) | IpAddr::V6(_) => NetworkExposure::Public,
    }
}

#[cfg(test)]
#[path = "workload_network_plan/tests.rs"]
mod tests;
