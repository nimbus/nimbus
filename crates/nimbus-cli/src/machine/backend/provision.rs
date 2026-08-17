//! Parent-host exact-phase adapter for compute-confirmed machine workloads.
//!
//! Compute owns lifecycle order. The guest owns workload and gvproxy effects.
//! This adapter transports one already-confirmed phase at a time. Its only
//! parent-side effect authority is ingress publication: exact host leases and
//! inspection-led reconciliation remain separate from guest provider effects.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use nimbus::{Error, SandboxBackendKind};
use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::restart_provider_command::ProviderRestartPhaseAdapter;
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
    validate_sandbox_provision_command,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressBindingWitness,
    WorkloadIngressObservationCapability, WorkloadIngressObservationFuture,
    WorkloadIngressObservationRequest, WorkloadObservedIngressEndpoint,
    WorkloadProviderObservation, workload_executable::decode_sandbox_spec,
};
use nimbus_machine::{
    MachineConnectivityCapabilities, MachineForwarderAuthority, MachineProvider,
    api::{MachineApiWorkloadProvisionCommandEnvelope, MachineApiWorkloadProvisionObservation},
};
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRequirements,
    NetworkCapabilityRole, NetworkCapabilitySelection, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkForwardingFeature, NetworkIngressCapabilitySet,
    NetworkIngressFeature, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkLifecycleRequirements, NetworkManagementMode, NetworkPlanId,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkResourceGeneration, NetworkResourceId,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortBindClaim, PortBindRealm,
    PortLeaseAccounting, PortLeaseBinding, PortLeaseLifetimeGuard, PortLeasePhase, PortProtocol,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, SandboxId, backends::container::OciMachinePortForwarderConfig,
};
use nimbus_workloads::{
    NodeIdentity, WorkloadExecutableIntent, WorkloadExecutionProviderId,
    WorkloadExecutionReference, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionCommandMode, WorkloadProvisionInspectionResult,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceEvidence, WorkloadProvisionStep,
    WorkloadPublicationIntent, WorkloadSagaKey,
};

use super::super::network_composition::HostMachineNetworkAuthority;
use super::super::publication_authority::{
    ConfirmedMachineDesireAdmissionGuard, ConfirmedMachinePublicationJournal,
    ConfirmedMachinePublicationMember, ConfirmedMachinePublicationObservation,
    ConfirmedMachinePublicationRetirement, authenticate_exact_durable_plan,
    canonical_machine_publication_members, canonical_machine_restart_publication_members,
    port_authority_error, recover_dead_batch,
};
use super::super::{DEFAULT_MACHINE_NAME, client::MachineApiClient};

mod teardown_authority;

const PROVIDER_JOURNAL_NAMESPACE: &str = "forwarded-machine-provision";
const RESTART_PROVIDER_JOURNAL_NAMESPACE: &str = "forwarded-machine-restart";
const FORWARDED_ATTACHMENT_PROVIDER_KEY: &str = "nimbus-machine.forwarded-container-attachment";
const FORWARDED_EXECUTION_PROVIDER_KEY: &str = "nimbus-machine.forwarded-container-execution";

pub(crate) fn forwarded_machine_attachment_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key(FORWARDED_ATTACHMENT_PROVIDER_KEY)
}

pub(crate) fn forwarded_machine_execution_provider_id() -> WorkloadExecutionProviderId {
    WorkloadExecutionProviderId::for_registration_key(FORWARDED_EXECUTION_PROVIDER_KEY)
}

/// Effect-free provider facts frozen before any machine or listener effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardedMachineProvisionSourcePlan {
    provider: MachineProvider,
    forwarder_authority: MachineForwarderAuthority,
    node_identity: NodeIdentity,
    connectivity: MachineConnectivityCapabilities,
    forwarder_config: OciMachinePortForwarderConfig,
    bundle: NetworkCapabilityBundle,
    selection: NetworkCapabilitySelection,
    requirements: NetworkCapabilityRequirements,
    sovereignty: NetworkSovereigntyRequirements,
    execution_provider_id: WorkloadExecutionProviderId,
    digest: WorkloadOwnerEvidenceDigest,
}

impl ForwardedMachineProvisionSourcePlan {
    pub(crate) fn new(
        provider: MachineProvider,
        forwarder_authority: MachineForwarderAuthority,
        node_identity: NodeIdentity,
        connectivity: MachineConnectivityCapabilities,
        forwarder_config: OciMachinePortForwarderConfig,
    ) -> Result<Self, Error> {
        if provider.network_management_mode() != NetworkManagementMode::NimbusHostManaged
            || connectivity.attachment().management_mode()
                != NetworkManagementMode::NimbusHostManaged
        {
            return Err(Error::InvalidInput(format!(
                "the {} machine provider does not offer Nimbus host-managed forwarded networking",
                provider.as_str()
            )));
        }
        if forwarder_config.provider_instance() != forwarder_authority.provider_instance()
            || forwarder_config.provider_generation() != forwarder_authority.generation()
        {
            return Err(Error::InvalidInput(
                "forwarded machine control endpoint is crossed with its provider instance or generation"
                    .to_owned(),
            ));
        }

        let lifecycle = NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]);
        let endpoint = NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            connectivity.exposures().iter().copied(),
            [PortProtocol::Tcp],
            [NetworkPortAssignmentMode::Exact],
        );
        let ingress = NetworkIngressCapabilitySet::new([NetworkIngressFeature::Streaming])
            .with_tls_behaviors([NetworkTlsBehavior::Disabled]);
        let forwarding =
            NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::PortForwarding]);
        let sovereignty = NetworkSovereigntyRequirements::new(
            connectivity.sovereignty().control_plane_locality(),
            connectivity
                .sovereignty()
                .required_external_dependencies()
                .iter()
                .copied(),
            connectivity.sovereignty().offline_restart_supported(),
        );
        let requirements = NetworkCapabilityRequirements::new(
            connectivity.attachment().clone(),
            endpoint.clone(),
            ingress.clone(),
            forwarding.clone(),
            NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle.clone()),
            sovereignty.clone(),
        );
        let bundle = NetworkCapabilityBundle::new(
            NetworkAttachmentProviderRegistration::new(
                forwarded_machine_attachment_provider_id(),
                connectivity.attachment().clone(),
                [NetworkAddressFamily::Ipv4],
                lifecycle.clone(),
                connectivity.sovereignty().clone(),
            ),
            NetworkIngressProviderRegistration::new(
                forwarder_authority
                    .provider_instance()
                    .provider_id()
                    .clone(),
                endpoint,
                ingress,
                forwarding,
                lifecycle,
                connectivity.sovereignty().clone(),
            ),
        );
        let selection = bundle.selection();
        let execution_provider_id = forwarded_machine_execution_provider_id();
        let digest = source_plan_digest(
            provider,
            &forwarder_authority,
            &node_identity,
            &connectivity,
            &forwarder_config,
            &bundle,
            &selection,
            &requirements,
            &sovereignty,
            &execution_provider_id,
        )?;
        Ok(Self {
            provider,
            forwarder_authority,
            node_identity,
            connectivity,
            forwarder_config,
            bundle,
            selection,
            requirements,
            sovereignty,
            execution_provider_id,
            digest,
        })
    }

    pub(crate) fn bundle(&self) -> &NetworkCapabilityBundle {
        &self.bundle
    }

    pub(crate) fn selection(&self) -> &NetworkCapabilitySelection {
        &self.selection
    }

    pub(crate) fn requirements(&self) -> &NetworkCapabilityRequirements {
        &self.requirements
    }

    pub(crate) fn sovereignty(&self) -> &NetworkSovereigntyRequirements {
        &self.sovereignty
    }

    pub(crate) fn node_identity(&self) -> &NodeIdentity {
        &self.node_identity
    }

    pub(crate) fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub(in crate::machine) fn forwarder_config(&self) -> &OciMachinePortForwarderConfig {
        &self.forwarder_config
    }

    #[cfg(test)]
    pub(crate) const fn machine_provider_generation(
        &self,
    ) -> nimbus_network::NetworkResourceGeneration {
        self.forwarder_authority.generation()
    }

    #[cfg(test)]
    pub(crate) const fn digest(&self) -> WorkloadOwnerEvidenceDigest {
        self.digest
    }

    pub(crate) fn activate(
        &self,
        client: MachineApiClient,
        network: &HostMachineNetworkAuthority,
    ) -> Result<std::sync::Arc<ForwardedMachineProvisionAdapter>, Error> {
        self.authenticate_for_activation(&client)?;
        ForwardedMachineProvisionAdapter::open(
            client,
            Some(network.clone()),
            network.port_leases(),
            self.clone(),
        )
        .map(std::sync::Arc::new)
    }

    pub(super) fn authenticate_for_activation(
        &self,
        client: &MachineApiClient,
    ) -> Result<(), Error> {
        self.forwarder_authority
            .authenticate(client.forwarder_authority()?)
            .map_err(|error| Error::PreconditionFailed(error.to_string()))?;
        let rebuilt = Self::new(
            self.provider,
            self.forwarder_authority.clone(),
            self.node_identity.clone(),
            self.connectivity.clone(),
            self.forwarder_config.clone(),
        )?;
        if &rebuilt != self {
            return Err(Error::PreconditionFailed(
                "forwarded machine provision source plan failed intrinsic authentication"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(serde::Serialize)]
struct ForwardedMachineProvisionSourcePlanDigest<'a> {
    provider: MachineProvider,
    forwarder_authority: &'a MachineForwarderAuthority,
    node_identity: &'a NodeIdentity,
    connectivity: &'a MachineConnectivityCapabilities,
    forwarder_config: &'a OciMachinePortForwarderConfig,
    bundle: &'a NetworkCapabilityBundle,
    selection: &'a NetworkCapabilitySelection,
    requirements: &'a NetworkCapabilityRequirements,
    sovereignty: &'a NetworkSovereigntyRequirements,
    execution_provider_id: &'a WorkloadExecutionProviderId,
}

#[allow(clippy::too_many_arguments)]
fn source_plan_digest(
    provider: MachineProvider,
    forwarder_authority: &MachineForwarderAuthority,
    node_identity: &NodeIdentity,
    connectivity: &MachineConnectivityCapabilities,
    forwarder_config: &OciMachinePortForwarderConfig,
    bundle: &NetworkCapabilityBundle,
    selection: &NetworkCapabilitySelection,
    requirements: &NetworkCapabilityRequirements,
    sovereignty: &NetworkSovereigntyRequirements,
    execution_provider_id: &WorkloadExecutionProviderId,
) -> Result<WorkloadOwnerEvidenceDigest, Error> {
    let payload = ForwardedMachineProvisionSourcePlanDigest {
        provider,
        forwarder_authority,
        node_identity,
        connectivity,
        forwarder_config,
        bundle,
        selection,
        requirements,
        sovereignty,
        execution_provider_id,
    };
    serde_json::to_vec(&payload)
        .map(WorkloadOwnerEvidenceDigest::sha256)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to digest forwarded machine provision source plan: {error}"
            ))
        })
}

/// Real forwarded-machine substitution for the narrow provision capabilities.
pub(crate) struct ForwardedMachineProvisionAdapter {
    client: MachineApiClient,
    // Production construction retains the process-composition token. Tests may
    // inject the primitive authority without manufacturing another manager.
    _parent_network: Option<HostMachineNetworkAuthority>,
    port_leases: LocalPortLeaseAuthority,
    publication_journal: ConfirmedMachinePublicationJournal,
    phases: ProviderProvisionPhaseAdapter,
    restart_phases: ProviderRestartPhaseAdapter,
    live: Mutex<BTreeMap<NetworkPlanId, LivePublicationBatch>>,
    machine_name: Arc<str>,
    source_plan: ForwardedMachineProvisionSourcePlan,
}

impl ForwardedMachineProvisionAdapter {
    pub(crate) fn desire_admission_guard(
        &self,
    ) -> Result<Arc<dyn nimbus_compute::workload_saga::WorkloadDesireAdmissionGuard>, Error> {
        Ok(Arc::new(ConfirmedMachineDesireAdmissionGuard::new(
            self.publication_journal.clone(),
            self.machine_name.as_ref(),
            self.source_plan.forwarder_authority.clone(),
            self.source_plan.execution_provider_id.clone(),
        )?))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        client: MachineApiClient,
        port_leases: LocalPortLeaseAuthority,
        source_plan: ForwardedMachineProvisionSourcePlan,
    ) -> Result<Self, Error> {
        source_plan.authenticate_for_activation(&client)?;
        Self::open(client, None, port_leases, source_plan)
    }

    fn open(
        client: MachineApiClient,
        parent_network: Option<HostMachineNetworkAuthority>,
        port_leases: LocalPortLeaseAuthority,
        source_plan: ForwardedMachineProvisionSourcePlan,
    ) -> Result<Self, Error> {
        let publication_journal =
            ConfirmedMachinePublicationJournal::open(port_leases.state_root())?;
        let phase_journal = ProviderCommandAttemptJournal::open(
            port_leases.state_root(),
            PROVIDER_JOURNAL_NAMESPACE,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open forwarded machine provision journal: {error}"
            ))
        })?;
        let restart_phase_journal = ProviderCommandAttemptJournal::open(
            port_leases.state_root(),
            RESTART_PROVIDER_JOURNAL_NAMESPACE,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open forwarded machine restart journal: {error}"
            ))
        })?;
        Ok(Self {
            client,
            _parent_network: parent_network,
            port_leases,
            publication_journal,
            phases: ProviderProvisionPhaseAdapter::new(phase_journal),
            restart_phases: ProviderRestartPhaseAdapter::new(restart_phase_journal),
            live: Mutex::new(BTreeMap::new()),
            machine_name: Arc::from(DEFAULT_MACHINE_NAME),
            source_plan,
        })
    }

    pub(super) fn teardown_client(&self) -> MachineApiClient {
        self.client.clone()
    }

    pub(super) fn teardown_state_root(&self) -> &std::path::Path {
        self.port_leases.state_root()
    }

    pub(super) fn teardown_source_plan(&self) -> &ForwardedMachineProvisionSourcePlan {
        &self.source_plan
    }

    pub(super) fn take_live_publication_batch(
        &self,
        plan_id: &NetworkPlanId,
        expected_members: &[ConfirmedMachinePublicationMember],
    ) -> Result<Option<LivePublicationBatch>, Error> {
        let mut batches = self.live.lock().map_err(|_| {
            Error::Internal("forwarded machine publication runtime registry is poisoned".to_owned())
        })?;
        let Some(batch) = batches.remove(plan_id) else {
            return Ok(None);
        };
        if batch.members != expected_members {
            batches.insert(plan_id.clone(), batch);
            return Err(Error::PreconditionFailed(
                "live parent publication members differ from teardown authority".to_owned(),
            ));
        }
        Ok(Some(batch))
    }

    fn validate_exact_phase(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        expected_step: WorkloadProvisionStep,
        expected_mode: WorkloadProvisionCommandMode,
    ) -> Result<ValidatedForwardedCommand, ProviderProvisionEffectObservation> {
        if command.step() != expected_step || command.mode() != expected_mode {
            return Err(definite_failure(
                "machine_phase_command_mode_mismatch",
                "forwarded machine capability received the wrong step or command mode",
            ));
        }
        let authority = self.client.forwarder_authority().map_err(|error| {
            definite_failure("machine_forwarder_authority_missing", error.to_string())
        })?;
        if command.execution().node_identity() != self.source_plan.node_identity()
            || !provider_target_matches(command, authority, &self.source_plan)
        {
            return Err(definite_failure(
                "machine_phase_provider_mismatch",
                "confirmed command does not target the exact provider owned by this machine adapter",
            ));
        }
        let validated = validate_sandbox_provision_command(command, SandboxBackendKind::Container)?;
        let envelope = MachineApiWorkloadProvisionCommandEnvelope::new(
            command.command_id(),
            command.attempt_id().clone(),
            command.dispatch_epoch(),
            command.provider_target().clone(),
            command.claim().clone(),
            command.confirmed_revision(),
            command.transition_id().clone(),
            command.generation(),
            command.desired_digest(),
            command.source().clone(),
            command.network_plan_digest(),
            command.execution().clone(),
            command.executable().clone(),
            command.compiled_network_plan().clone(),
            authority.generation(),
            command.mode(),
        )
        .map_err(|error| definite_failure("machine_phase_envelope_rejected", error.to_string()))?;
        self.publication_journal
            .authenticate_retirement_witness(&self.machine_name, &envelope, authority)
            .map_err(|error| {
                definite_failure("machine_retirement_witness_rejected", error.to_string())
            })?;
        Ok(ValidatedForwardedCommand {
            envelope,
            authority: authority.clone(),
            plan_id: validated.network_plan().plan_id().clone(),
        })
    }

    fn validate_publication(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        expected_step: WorkloadProvisionStep,
        expected_mode: WorkloadProvisionCommandMode,
    ) -> Result<ValidatedForwardedPublication, ProviderProvisionEffectObservation> {
        let validated = self.validate_exact_phase(command, expected_step, expected_mode)?;
        let members =
            canonical_machine_publication_members(&validated.envelope, &validated.authority)
                .map_err(|error| {
                    definite_failure("machine_parent_publication_invalid", error.to_string())
                })?;
        Ok(ValidatedForwardedPublication {
            envelope: validated.envelope,
            authority: validated.authority,
            plan_id: validated.plan_id,
            members,
        })
    }

    fn forward_exact_phase(
        &self,
        validated: &ValidatedForwardedCommand,
    ) -> ProviderProvisionEffectObservation {
        match self
            .client
            .provision_workload_phase(validated.envelope.clone())
        {
            Ok(response) => machine_observation(response.observation()),
            Err(error) => ambiguous(error.to_string()),
        }
    }

    fn execute_exact_phase(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        step: WorkloadProvisionStep,
    ) -> WorkloadProvisionInspectionResult {
        let validated =
            match self.validate_exact_phase(command, step, WorkloadProvisionCommandMode::Execute) {
                Ok(validated) => validated,
                Err(error) => return direct_result(command, error),
            };
        self.phases
            .execute(command, || self.forward_exact_phase(&validated))
    }

    fn inspect_exact_phase(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        step: WorkloadProvisionStep,
    ) -> WorkloadProvisionInspectionResult {
        let validated =
            match self.validate_exact_phase(command, step, WorkloadProvisionCommandMode::Inspect) {
                Ok(validated) => validated,
                Err(error) => return direct_result(command, error),
            };
        self.phases
            .inspect(command, || self.forward_exact_phase(&validated))
    }

    fn observe_execution(
        &self,
        key: &WorkloadSagaKey,
        execution: &WorkloadExecutionReference,
        source: &WorkloadProvisionSourceEvidence,
        executable: &WorkloadExecutableIntent,
    ) -> WorkloadProviderObservation<nimbus_sandbox::SandboxInspection> {
        let Some(sandbox_id) =
            validate_execution_observation(key, execution, source, executable, &self.source_plan)
        else {
            return WorkloadProviderObservation::Ambiguous;
        };
        match self.client.inspect_service_sandbox(&sandbox_id) {
            Ok(Some(inspection)) if inspection.handle.tenant_id == *key.tenant_id() => {
                WorkloadProviderObservation::Present(inspection)
            }
            Ok(Some(_)) | Err(Error::InvalidInput(_)) => WorkloadProviderObservation::Ambiguous,
            Ok(None) => WorkloadProviderObservation::Absent,
            Err(_) => WorkloadProviderObservation::Ambiguous,
        }
    }

    /// Read only the continuously held parent publication batch.
    ///
    /// This path never forwards a Machine API phase, opens a journal, reclaims
    /// a lifetime, reserves a port, or repairs provider state. Projection can
    /// trust it only when the request, retained source plan, durable records,
    /// live lifetime guards, and concrete bindings all agree exactly.
    fn observe_ingress(
        &self,
        request: &WorkloadIngressObservationRequest,
    ) -> WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>> {
        let Some(query) =
            ForwardedIngressObservationQuery::authenticate(request, &self.source_plan)
        else {
            return WorkloadProviderObservation::Ambiguous;
        };
        let live = match self.live.lock() {
            Ok(live) => live,
            Err(_) => return WorkloadProviderObservation::Ambiguous,
        };
        let Some(batch) = live.get(&query.plan_id) else {
            return WorkloadProviderObservation::InProgress;
        };
        if batch.members.len() != query.listeners.len()
            || batch.lifetimes.len() != batch.members.len()
        {
            return WorkloadProviderObservation::Ambiguous;
        }
        let records = match self.port_leases.list_plan(&query.plan_id) {
            Ok(records) => records,
            Err(_) => return WorkloadProviderObservation::Ambiguous,
        };
        if records.len() != batch.members.len()
            || authenticate_exact_durable_plan(&publication_requests(&batch.members), &records)
                .is_err()
        {
            return WorkloadProviderObservation::Ambiguous;
        }
        if !exact_active_batch(&records, &batch.members) {
            return WorkloadProviderObservation::InProgress;
        }

        let mut seen = BTreeSet::new();
        let mut endpoints = Vec::with_capacity(batch.members.len());
        for member in &batch.members {
            let Some(expected) = query.listeners.get(member.listener_id()) else {
                return WorkloadProviderObservation::Ambiguous;
            };
            if !seen.insert(member.listener_id().clone())
                || &expected.listener_id != member.listener_id()
            {
                return WorkloadProviderObservation::Ambiguous;
            }
            let request = member.request();
            let Some(record) = records
                .iter()
                .find(|record| record.request().lease_id() == request.lease_id())
            else {
                return WorkloadProviderObservation::Ambiguous;
            };
            let Some(lifetime) = batch
                .lifetimes
                .iter()
                .find(|lifetime| lifetime.request().lease_id() == request.lease_id())
            else {
                return WorkloadProviderObservation::Ambiguous;
            };
            let Some(active_lifetime) = record.active_lifetime() else {
                return WorkloadProviderObservation::InProgress;
            };
            let Some(binding) = record.binding() else {
                return WorkloadProviderObservation::InProgress;
            };
            let endpoint = binding.endpoint();
            let Some(published_ip) = endpoint.target().specific_address() else {
                return WorkloadProviderObservation::Ambiguous;
            };
            if lifetime.request() != request
                || record.request() != request
                || active_lifetime != lifetime.lifetime()
                || binding != member.expected_binding()
                || request.lease_id() != &expected.port_lease_id
                || request.owner_id() != &NetworkResourceId::from(expected.listener_id.clone())
                || request.plan_id() != Some(&query.plan_id)
                || request.tenant_id() != Some(&query.tenant_id)
                || request.generation() != query.generation
                || request.accounting() != PortLeaseAccounting::TenantPublished
                || request.publication().host_address() != Some(expected.desired_host_address)
                || request.binding().protocol() != PortProtocol::Tcp
                || request.binding().realm() != &PortBindRealm::Host
                || endpoint.protocol() != PortProtocol::Tcp
                || endpoint.realm() != &PortBindRealm::Host
                || published_ip != expected.desired_host_address
            {
                return WorkloadProviderObservation::Ambiguous;
            }
            endpoints.push(WorkloadObservedIngressEndpoint::new(
                expected.endpoint_id.clone(),
                SocketAddr::new(published_ip, endpoint.port().get()),
                WorkloadIngressBindingWitness::new(
                    query.plan_id.clone(),
                    query.plan_digest,
                    query.generation,
                    expected.listener_id.clone(),
                    expected.port_lease_id.clone(),
                    active_lifetime,
                    lifetime.lifetime(),
                    endpoint.clone(),
                    binding.provenance(),
                ),
            ));
        }
        if seen.len() != query.listeners.len() {
            return WorkloadProviderObservation::Ambiguous;
        }
        endpoints.sort_by(|left, right| left.endpoint_id().cmp(right.endpoint_id()));
        WorkloadProviderObservation::Present(endpoints)
    }

    fn authenticate_parent(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.publication_journal
            .authenticate_or_stage(
                &self.machine_name,
                &validated.envelope,
                &validated.authority,
                &validated.members,
            )
            .map_err(parent_journal_error)
    }

    fn publish(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> ProviderProvisionEffectObservation {
        if let Err(error) = self.reserve_parent_batch(validated) {
            return error;
        }
        if let Err(error) = self.publication_journal.commit_before_machine_api(
            &validated.envelope,
            &validated.authority,
            &validated.members,
        ) {
            return parent_journal_error(error);
        }
        let response = match self
            .client
            .provision_workload_phase(validated.envelope.clone())
        {
            Ok(response) => response,
            Err(error) => {
                let observation = ConfirmedMachinePublicationObservation::Ambiguous;
                let _ = self.publication_journal.record_observation(
                    &validated.envelope,
                    &validated.authority,
                    &validated.members,
                    observation,
                );
                return ProviderProvisionEffectObservation::Ambiguous {
                    evidence: error.to_string().into_bytes(),
                };
            }
        };
        self.translate_response(validated, response.observation())
    }

    fn inspect_remote(
        &self,
        validated: &ValidatedForwardedPublication,
        allow_absence: bool,
    ) -> ProviderProvisionEffectObservation {
        if let Err(error) = self.publication_journal.commit_before_machine_api(
            &validated.envelope,
            &validated.authority,
            &validated.members,
        ) {
            return parent_journal_error(error);
        }
        let response = match self
            .client
            .provision_workload_phase(validated.envelope.clone())
        {
            Ok(response) => response,
            Err(error) => {
                let observation = ConfirmedMachinePublicationObservation::Ambiguous;
                let _ = self.publication_journal.record_observation(
                    &validated.envelope,
                    &validated.authority,
                    &validated.members,
                    observation,
                );
                return ProviderProvisionEffectObservation::Ambiguous {
                    evidence: error.to_string().into_bytes(),
                };
            }
        };
        match response.observation() {
            MachineApiWorkloadProvisionObservation::Absent { evidence } if !allow_absence => {
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::InProgress,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::InProgress {
                    evidence: evidence.clone(),
                }
            }
            observation => self.translate_response(validated, observation),
        }
    }

    fn translate_response(
        &self,
        validated: &ValidatedForwardedPublication,
        observation: &MachineApiWorkloadProvisionObservation,
    ) -> ProviderProvisionEffectObservation {
        match observation {
            MachineApiWorkloadProvisionObservation::Succeeded { evidence } => {
                let parent = match validated.envelope.claim().attempt().step() {
                    WorkloadProvisionStep::Publish => self.activate_parent_batch(validated),
                    WorkloadProvisionStep::ObservePublication => {
                        self.observe_parent_batch(validated)
                    }
                    _ => Err(definite_failure(
                        "machine_publication_step_mismatch",
                        "forwarded ingress response belongs to a non-publication step",
                    )),
                };
                if let Err(error) = parent {
                    return error;
                }
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::Succeeded,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::Succeeded {
                    evidence: evidence.clone(),
                }
            }
            MachineApiWorkloadProvisionObservation::DefiniteFailure { evidence } => {
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::DefiniteFailure,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::DefiniteFailure {
                    code: "machine_publication_rejected".to_owned(),
                    evidence: evidence.clone(),
                }
            }
            MachineApiWorkloadProvisionObservation::Absent { evidence } => {
                if validated.envelope.mode() == WorkloadProvisionCommandMode::Execute {
                    if let Err(error) = self.record_parent_observation(
                        validated,
                        ConfirmedMachinePublicationObservation::DefiniteFailure,
                    ) {
                        return error;
                    }
                    return ProviderProvisionEffectObservation::DefiniteFailure {
                        code: "machine_publication_absent_during_execute".to_owned(),
                        evidence: evidence.clone(),
                    };
                }
                if let Err(error) = self.reconcile_parent_absence(validated) {
                    return error;
                }
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::Absent,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::Absent {
                    evidence: evidence.clone(),
                }
            }
            MachineApiWorkloadProvisionObservation::InProgress { evidence } => {
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::InProgress,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::InProgress {
                    evidence: evidence.clone(),
                }
            }
            MachineApiWorkloadProvisionObservation::Ambiguous { evidence } => {
                if let Err(error) = self.record_parent_observation(
                    validated,
                    ConfirmedMachinePublicationObservation::Ambiguous,
                ) {
                    return error;
                }
                ProviderProvisionEffectObservation::Ambiguous {
                    evidence: evidence.clone(),
                }
            }
        }
    }

    fn record_parent_observation(
        &self,
        validated: &ValidatedForwardedPublication,
        observation: ConfirmedMachinePublicationObservation,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.publication_journal
            .record_observation(
                &validated.envelope,
                &validated.authority,
                &validated.members,
                observation,
            )
            .map_err(parent_journal_error)
    }

    fn reserve_parent_batch(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.reserve_parent_batch_for(&validated.plan_id, &validated.members)
    }

    fn reserve_parent_batch_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), ProviderProvisionEffectObservation> {
        if members.is_empty() {
            return Ok(());
        }
        let claims = members
            .iter()
            .map(|member| (member.request().clone(), member.bind_claim().clone()))
            .collect::<Vec<_>>();
        let reservation = self
            .port_leases
            .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
            .map_err(|error| {
                definite_failure("machine_parent_port_lease_rejected", error.to_string())
            })?;
        let live = LivePublicationBatch {
            members: members.to_vec(),
            lifetimes: reservation.into_parts().1,
        };
        let mut batches = self
            .live
            .lock()
            .map_err(|_| ambiguous("forwarded machine publication runtime registry is poisoned"))?;
        match batches.entry(plan_id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(live);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => Err(definite_failure(
                "machine_parent_publication_live_conflict",
                "the exact network plan already has a live parent publication owner",
            )),
        }
    }

    fn activate_parent_batch(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.activate_parent_batch_for(&validated.plan_id, &validated.members)
    }

    fn activate_parent_batch_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), ProviderProvisionEffectObservation> {
        if members.is_empty() {
            return Ok(());
        }
        let batches = self
            .live
            .lock()
            .map_err(|_| ambiguous("forwarded machine publication runtime registry is poisoned"))?;
        if let Some(live) = batches.get(plan_id) {
            if live.members != members {
                return Err(definite_failure(
                    "machine_parent_publication_live_mismatch",
                    "live parent publication members differ from the canonical command",
                ));
            }
            let records = self
                .port_leases
                .list_plan(plan_id)
                .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
            let requests = publication_requests(members);
            authenticate_exact_durable_plan(&requests, &records).map_err(|error| {
                definite_failure("machine_parent_plan_mismatch", error.to_string())
            })?;
            let result = if exact_active_batch(&records, members) {
                Ok(())
            } else {
                let bindings = activation_batch(members);
                self.port_leases
                    .adopt_claimed_and_activate_batch_with_lifetimes(
                        &bindings,
                        None,
                        &live.lifetimes,
                    )
                    .map(|_| ())
                    .map_err(|error| ambiguous(port_authority_error(error).to_string()))
            };
            return result;
        }
        drop(batches);

        let records = self
            .port_leases
            .list_plan(plan_id)
            .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        let requests = publication_requests(members);
        if records.is_empty() {
            return Err(definite_failure(
                "machine_parent_publication_untracked",
                "guest publication exists without its required parent lease authority",
            ));
        }
        authenticate_exact_durable_plan(&requests, &records)
            .map_err(|error| definite_failure("machine_parent_plan_mismatch", error.to_string()))?;
        if exact_active_batch(&records, members) {
            // The durable binding is exact. A fresh process still needs to
            // reclaim the provider-managed lifetime below.
        } else if !records.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved && record.bind_claim().is_some()
        }) {
            return Err(ambiguous(
                "parent publication leases are neither an exact active batch nor an unadopted batch",
            ));
        }
        let recoveries = recover_dead_batch(&self.port_leases, &requests)
            .map_err(|error| ambiguous(error.to_string()))?;
        let mut lifetimes = Vec::with_capacity(members.len());
        for (member, recovery) in members.iter().zip(recoveries) {
            let lifetime = self
                .port_leases
                .reclaim_provider_managed_binding_after_owner_death(
                    member.request(),
                    member.expected_binding(),
                    recovery,
                )
                .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
            lifetimes.push(lifetime);
        }
        self.live
            .lock()
            .map_err(|_| ambiguous("forwarded machine publication runtime registry is poisoned"))?
            .insert(
                plan_id.clone(),
                LivePublicationBatch {
                    members: members.to_vec(),
                    lifetimes,
                },
            );
        Ok(())
    }

    fn observe_parent_batch(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.observe_parent_batch_for(&validated.plan_id, &validated.members)
    }

    fn observe_parent_batch_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), ProviderProvisionEffectObservation> {
        if members.is_empty() {
            return Ok(());
        }
        let mut batches = self
            .live
            .lock()
            .map_err(|_| ambiguous("forwarded machine publication runtime registry is poisoned"))?;
        if let Some(live) = batches.get(plan_id) {
            return if live.members == members {
                Ok(())
            } else {
                Err(definite_failure(
                    "machine_parent_publication_live_mismatch",
                    "live parent publication members differ from the canonical command",
                ))
            };
        }
        let records = self
            .port_leases
            .list_plan(plan_id)
            .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        let requests = publication_requests(members);
        authenticate_exact_durable_plan(&requests, &records)
            .map_err(|error| definite_failure("machine_parent_plan_mismatch", error.to_string()))?;
        if !exact_active_batch(&records, members) {
            return Err(ProviderProvisionEffectObservation::InProgress {
                evidence: b"parent machine publication leases are not all active".to_vec(),
            });
        }
        let recoveries = recover_dead_batch(&self.port_leases, &requests)
            .map_err(|error| ambiguous(error.to_string()))?;
        let mut lifetimes = Vec::with_capacity(members.len());
        for (member, recovery) in members.iter().zip(recoveries) {
            let lifetime = self
                .port_leases
                .reclaim_provider_managed_binding_after_owner_death(
                    member.request(),
                    member.expected_binding(),
                    recovery,
                )
                .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
            lifetimes.push(lifetime);
        }
        batches.insert(
            plan_id.clone(),
            LivePublicationBatch {
                members: members.to_vec(),
                lifetimes,
            },
        );
        Ok(())
    }

    fn reconcile_parent_absence(
        &self,
        validated: &ValidatedForwardedPublication,
    ) -> Result<(), ProviderProvisionEffectObservation> {
        self.reconcile_parent_absence_for(&validated.plan_id, &validated.members)
    }

    fn reconcile_parent_absence_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<(), ProviderProvisionEffectObservation> {
        if members.is_empty() {
            return Ok(());
        }
        // Dropping the live batch first releases its non-cloneable lifetime
        // locks. Exact guest absence then permits recovery to retain the same
        // stable numeric slots for the next compute-authorized epoch.
        self.live
            .lock()
            .map_err(|_| ambiguous("forwarded machine publication runtime registry is poisoned"))?
            .remove(plan_id);
        let records = self
            .port_leases
            .list_plan(plan_id)
            .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        if records.is_empty() {
            return Ok(());
        }
        let requests = publication_requests(members);
        authenticate_exact_durable_plan(&requests, &records)
            .map_err(|error| definite_failure("machine_parent_plan_mismatch", error.to_string()))?;
        if records.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.bind_claim().is_none()
        }) {
            return Ok(());
        }
        let recoveries = recover_dead_batch(&self.port_leases, &requests)
            .map_err(|error| ambiguous(error.to_string()))?;
        self.port_leases
            .mark_cleanup_pending_batch_after_owner_death(&requests, &recoveries)
            .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        if records.iter().all(|record| record.binding().is_some()) {
            let bindings = members
                .iter()
                .map(|member| (member.request().clone(), member.expected_binding().clone()))
                .collect::<Vec<_>>();
            self.port_leases
                .prepare_rebind_provider_managed_batch_after_confirmed_stop(&bindings, &recoveries)
                .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        } else if records.iter().all(|record| record.binding().is_none()) {
            self.port_leases
                .prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(
                    &requests,
                    &recoveries,
                )
                .map_err(|error| ambiguous(port_authority_error(error).to_string()))?;
        } else {
            return Err(ambiguous(
                "parent publication lease batch mixes adopted and unadopted members",
            ));
        }
        Ok(())
    }
}

macro_rules! impl_forwarded_execute_and_inspect_capability {
    ($capability:ty, $step:expr) => {
        impl $capability for ForwardedMachineProvisionAdapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.execute_exact_phase(command, $step) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.inspect_exact_phase(command, $step) })
            }
        }
    };
}

impl_forwarded_execute_and_inspect_capability!(
    NetworkReservationCapability,
    WorkloadProvisionStep::ReserveNetwork
);
impl_forwarded_execute_and_inspect_capability!(
    WorkloadPreparationCapability,
    WorkloadProvisionStep::PrepareWorkload
);
impl_forwarded_execute_and_inspect_capability!(
    NetworkAttachmentCapability,
    WorkloadProvisionStep::AttachNetwork
);
impl_forwarded_execute_and_inspect_capability!(
    WorkloadActivationCapability,
    WorkloadProvisionStep::ActivateWorkload
);

impl WorkloadActivationPrerequisiteCapability for ForwardedMachineProvisionAdapter {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.inspect_exact_phase(
                command,
                WorkloadProvisionStep::InspectActivationPrerequisites,
            )
        })
    }
}

impl WorkloadReadinessCapability for ForwardedMachineProvisionAdapter {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.inspect_exact_phase(command, WorkloadProvisionStep::InspectWorkloadReadiness)
        })
    }
}

impl WorkloadExecutionObservationCapability for ForwardedMachineProvisionAdapter {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            self.observe_execution(
                request.key(),
                request.execution(),
                request.source(),
                request.executable(),
            )
        })
    }
}

impl WorkloadIngressObservationCapability for ForwardedMachineProvisionAdapter {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async move { self.observe_ingress(request) })
    }
}

impl IngressPublicationCapability for ForwardedMachineProvisionAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = match self.validate_publication(
                command,
                WorkloadProvisionStep::Publish,
                WorkloadProvisionCommandMode::Execute,
            ) {
                Ok(validated) => validated,
                Err(error) => return direct_result(command, error),
            };
            if let Err(error) = self.authenticate_parent(&validated) {
                return direct_result(command, error);
            }
            self.phases.execute(command, || self.publish(&validated))
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = match self.validate_publication(
                command,
                WorkloadProvisionStep::Publish,
                WorkloadProvisionCommandMode::Inspect,
            ) {
                Ok(validated) => validated,
                Err(error) => return direct_result(command, error),
            };
            if let Err(error) = self.authenticate_parent(&validated) {
                return direct_result(command, error);
            }
            self.phases
                .inspect_live(command, || self.inspect_remote(&validated, true))
        })
    }
}

impl IngressPublicationInspectionCapability for ForwardedMachineProvisionAdapter {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            let validated = match self.validate_publication(
                command,
                WorkloadProvisionStep::ObservePublication,
                WorkloadProvisionCommandMode::Inspect,
            ) {
                Ok(validated) => validated,
                Err(error) => return direct_result(command, error),
            };
            if let Err(error) = self.authenticate_parent(&validated) {
                return direct_result(command, error);
            }
            self.phases
                .inspect_live(command, || self.inspect_remote(&validated, false))
        })
    }
}

struct ValidatedForwardedCommand {
    envelope: MachineApiWorkloadProvisionCommandEnvelope,
    authority: MachineForwarderAuthority,
    plan_id: NetworkPlanId,
}

struct ValidatedForwardedPublication {
    envelope: MachineApiWorkloadProvisionCommandEnvelope,
    authority: MachineForwarderAuthority,
    plan_id: NetworkPlanId,
    members: Vec<ConfirmedMachinePublicationMember>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForwardedIngressObservationExpectation {
    endpoint_id: nimbus_network::PublishedEndpointId,
    listener_id: nimbus_network::ListenerId,
    port_lease_id: nimbus_network::PortLeaseId,
    desired_host_address: std::net::IpAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForwardedIngressObservationQuery {
    tenant_id: nimbus_core::TenantId,
    plan_id: NetworkPlanId,
    plan_digest: nimbus_network::NetworkPlanDigest,
    generation: NetworkResourceGeneration,
    listeners: BTreeMap<nimbus_network::ListenerId, ForwardedIngressObservationExpectation>,
}

impl ForwardedIngressObservationQuery {
    fn authenticate(
        request: &WorkloadIngressObservationRequest,
        source_plan: &ForwardedMachineProvisionSourcePlan,
    ) -> Option<Self> {
        let plan = request.compiled_plan();
        let content = plan.content();
        let publication = request.publication();
        let network = publication.network();
        let identity = content.identity();
        if content.publication() != WorkloadPublicationIntent::PublishWhenReady
            || request.key().tenant_id() != identity.tenant_id()
            || request.execution().generation().as_u64() != identity.generation().as_u64()
            || network.plan_id() != plan.plan().plan_id()
            || network.digest() != plan.plan().digest()
            || network.generation() != identity.generation()
            || plan.plan().generation() != identity.generation()
            || content.capability_selection().is_none_or(|selection| {
                selection.ingress_provider_id() != source_plan.selection().ingress_provider_id()
            })
        {
            return None;
        }

        let expected_endpoint_ids = publication
            .endpoints()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if expected_endpoint_ids.is_empty()
            || expected_endpoint_ids.len() != publication.endpoints().len()
            || expected_endpoint_ids.len() != content.listeners().len()
        {
            return None;
        }
        let mut listeners = BTreeMap::new();
        for listener in content.listeners() {
            if !expected_endpoint_ids.contains(listener.endpoint_id())
                || listeners
                    .insert(
                        listener.listener_id().clone(),
                        ForwardedIngressObservationExpectation {
                            endpoint_id: listener.endpoint_id().clone(),
                            listener_id: listener.listener_id().clone(),
                            port_lease_id: listener.port_lease_id().clone(),
                            desired_host_address: listener.desired_host_address(),
                        },
                    )
                    .is_some()
            {
                return None;
            }
        }
        Some(Self {
            tenant_id: request.key().tenant_id().clone(),
            plan_id: plan.plan().plan_id().clone(),
            plan_digest: plan.plan().digest(),
            generation: identity.generation(),
            listeners,
        })
    }
}

pub(super) struct LivePublicationBatch {
    members: Vec<ConfirmedMachinePublicationMember>,
    lifetimes: Vec<PortLeaseLifetimeGuard>,
}

impl LivePublicationBatch {
    pub(super) fn lifetimes(&self) -> &[PortLeaseLifetimeGuard] {
        &self.lifetimes
    }
}

fn activation_batch(
    members: &[ConfirmedMachinePublicationMember],
) -> Vec<(
    nimbus_network::PortLeaseRequest,
    PortBindClaim,
    PortLeaseBinding,
)> {
    members
        .iter()
        .map(|member| {
            (
                member.request().clone(),
                member.bind_claim().clone(),
                member.expected_binding().clone(),
            )
        })
        .collect()
}

pub(super) fn publication_requests(
    members: &[ConfirmedMachinePublicationMember],
) -> Vec<nimbus_network::PortLeaseRequest> {
    members
        .iter()
        .map(|member| member.request().clone())
        .collect()
}

fn exact_active_batch(
    records: &[nimbus_network::PortLeaseRecord],
    members: &[ConfirmedMachinePublicationMember],
) -> bool {
    records.iter().all(|record| {
        record.phase() == PortLeasePhase::Active
            && members.iter().any(|member| {
                member.request().lease_id() == record.request().lease_id()
                    && record.binding() == Some(member.expected_binding())
            })
    })
}

fn provider_target_matches(
    command: &ConfirmedWorkloadProvisionCommand,
    authority: &MachineForwarderAuthority,
    source_plan: &ForwardedMachineProvisionSourcePlan,
) -> bool {
    match (command.step(), command.provider_target()) {
        (
            WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Attachment,
                provider_id,
                provider_source_digest,
            },
        ) => {
            provider_id == source_plan.selection().attachment_provider_id()
                && *provider_source_digest
                    == source_plan.bundle().selection_evidence().source_digest()
        }
        (
            WorkloadProvisionStep::PrepareWorkload
            | WorkloadProvisionStep::InspectActivationPrerequisites
            | WorkloadProvisionStep::ActivateWorkload
            | WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionProviderTarget::Execution { provider_id, .. },
        ) => provider_id == source_plan.execution_provider_id(),
        (
            WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Ingress,
                provider_id,
                provider_source_digest,
            },
        ) => {
            provider_id == authority.provider_instance().provider_id()
                && *provider_source_digest
                    == source_plan.bundle().selection_evidence().source_digest()
        }
        _ => false,
    }
}

fn machine_observation(
    observation: &MachineApiWorkloadProvisionObservation,
) -> ProviderProvisionEffectObservation {
    match observation {
        MachineApiWorkloadProvisionObservation::Succeeded { evidence } => {
            ProviderProvisionEffectObservation::Succeeded {
                evidence: evidence.clone(),
            }
        }
        MachineApiWorkloadProvisionObservation::DefiniteFailure { evidence } => {
            ProviderProvisionEffectObservation::DefiniteFailure {
                code: "machine_phase_rejected".to_owned(),
                evidence: evidence.clone(),
            }
        }
        MachineApiWorkloadProvisionObservation::Absent { evidence } => {
            ProviderProvisionEffectObservation::Absent {
                evidence: evidence.clone(),
            }
        }
        MachineApiWorkloadProvisionObservation::InProgress { evidence } => {
            ProviderProvisionEffectObservation::InProgress {
                evidence: evidence.clone(),
            }
        }
        MachineApiWorkloadProvisionObservation::Ambiguous { evidence } => {
            ProviderProvisionEffectObservation::Ambiguous {
                evidence: evidence.clone(),
            }
        }
    }
}

fn validate_execution_observation(
    key: &WorkloadSagaKey,
    execution: &WorkloadExecutionReference,
    source: &WorkloadProvisionSourceEvidence,
    executable: &WorkloadExecutableIntent,
    source_plan: &ForwardedMachineProvisionSourcePlan,
) -> Option<SandboxId> {
    let spec = decode_sandbox_spec(executable).ok()?;
    if spec.backend != SandboxBackendKind::Container
        || spec.tenant_id != *key.tenant_id()
        || execution.node_identity() != source_plan.node_identity()
        || source.execution_provider_id() != source_plan.execution_provider_id()
    {
        return None;
    }
    Some(SandboxId::new(execution.execution_id().as_str()))
}

fn parent_journal_error(error: Error) -> ProviderProvisionEffectObservation {
    match error {
        error @ (Error::InvalidInput(_)
        | Error::PreconditionFailed(_)
        | Error::AlreadyExists(_)
        | Error::Conflict { .. }) => definite_failure(
            "machine_parent_publication_fence_rejected",
            error.to_string(),
        ),
        error => ambiguous(error.to_string()),
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

fn ambiguous(evidence: impl Into<Vec<u8>>) -> ProviderProvisionEffectObservation {
    ProviderProvisionEffectObservation::Ambiguous {
        evidence: evidence.into(),
    }
}

fn direct_result(
    command: &ConfirmedWorkloadProvisionCommand,
    observation: ProviderProvisionEffectObservation,
) -> WorkloadProvisionInspectionResult {
    match observation {
        ProviderProvisionEffectObservation::DefiniteFailure { code, evidence } => {
            let digest = WorkloadOwnerEvidenceDigest::sha256(evidence);
            let failure = WorkloadFailureEvidence::new(&code, digest).unwrap_or_else(|_| {
                WorkloadFailureEvidence::new("machine_publication_rejected", digest)
                    .expect("the fallback failure code is valid")
            });
            WorkloadProvisionInspectionResult::DefiniteFailure {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                failure,
            }
        }
        _ => WorkloadProvisionInspectionResult::Ambiguous {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
        },
    }
}

#[path = "provision/restart.rs"]
mod restart;

#[cfg(test)]
pub(crate) use restart::tests::{
    confirmed_automatic_restart_command_for_test, confirmed_restart_command_for_test,
};

#[cfg(test)]
pub(crate) fn forwarder_authority_for_test() -> MachineForwarderAuthority {
    tests::forwarder_authority()
}

#[cfg(test)]
#[path = "provision/tests.rs"]
mod tests;
