use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use nimbus_core::{Document, TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
    NetworkAttachmentProviderRegistration, NetworkBindRealmKind, NetworkCapabilityBundle,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortProtocol,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadNetworkEndpointSemantics,
    WorkloadNetworkForwardingBehavior, WorkloadNetworkIntent, WorkloadNetworkListenerBlueprint,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadPhaseDetail,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadSagaKey, WorkloadSagaRecord,
};

use super::codec::encode_workload_saga_record;
use super::schema::workload_saga_table;

mod ambiguity;
mod codec;
mod compiled_plan_durability;
mod composition;
mod durability;
mod executable_durability;
mod ingress;
mod provision_driver_process;
mod provision_fixture;
mod recovery;
mod restart;
mod restart_candidates;
mod restart_process;
mod store;
mod tenant_enumeration;

fn engine(root: &tempfile::TempDir) -> Arc<Engine> {
    Arc::new(Engine::new(root.path()).expect("fixture Engine should open"))
}

fn initial_record(label: &str) -> WorkloadSagaRecord {
    initial_record_with_seed(label, "default")
}

fn initial_record_with_seed(label: &str, seed: &str) -> WorkloadSagaRecord {
    initial_record_with_counters_and_seed(label, 1, 1, seed)
}

fn initial_record_with_counters(
    label: &str,
    generation: u64,
    network_generation: u64,
) -> WorkloadSagaRecord {
    initial_record_with_counters_and_seed(label, generation, network_generation, "default")
}

fn initial_record_with_counters_and_seed(
    label: &str,
    generation: u64,
    network_generation: u64,
    seed: &str,
) -> WorkloadSagaRecord {
    let tenant_id = TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid");
    let key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(format!("workload-{label}")).expect("fixture workload is valid"),
    );
    let executable = nimbus_workloads::WorkloadExecutableIntent::new(
        nimbus_workloads::WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"desired-{label}-{seed}"}}"#),
    )
    .expect("fixture executable is valid");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(label, "fixture")
            .expect("fixture source identity is valid"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new(format!("fixture-{seed}"))
            .expect("fixture source version is valid"),
        executable.content_digest(),
        nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment"),
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
    )
    .expect("fixture source evidence is valid");
    let intent = nimbus_workloads::WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        nimbus_workloads::WorkloadGeneration::new(generation),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_network_plan(
            &tenant_id,
            &format!("{label}-{seed}"),
            network_generation,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("fixture decision id is valid"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixture workload uid is valid"),
            NodeIdentity::new(format!("node-{label}")).expect("fixture node is valid"),
        ),
    )
    .expect("fixture intent is valid");
    WorkloadSagaRecord::new(key, intent).expect("initial record is valid")
}

pub(super) fn provision_source(
    executable: &nimbus_workloads::WorkloadExecutableIntent,
    label: &str,
    generation: u64,
    attachment_provider_id: nimbus_network::NetworkProviderId,
) -> WorkloadProvisionSourceEvidence {
    WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(label, "fixture")
            .expect("fixture source identity is valid"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new(format!("fixture-{label}-{generation}"))
            .expect("fixture source version is valid"),
        executable.content_digest(),
        attachment_provider_id,
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
    )
    .expect("fixture source evidence is valid")
}

pub(super) fn compiled_network_plan(
    tenant_id: &TenantId,
    workload_incarnation: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        workload_incarnation,
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture network identity is valid");
    let attachment =
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []);
    let endpoint = NetworkEndpointCapabilitySet::new(
        [NetworkAddressFamily::Ipv4],
        [NetworkBindRealmKind::Host],
        [NetworkExposure::Loopback],
        [PortProtocol::Tcp],
        [NetworkPortAssignmentMode::ProviderAssigned],
    );
    let ingress = NetworkIngressCapabilitySet::new([]);
    let forwarding = NetworkForwardingCapabilitySet::new([]);
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let requirements = NetworkCapabilityRequirements::new(
        attachment.clone(),
        endpoint.clone(),
        ingress.clone(),
        forwarding.clone(),
        nimbus_network::NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle.clone()),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let (selection, selection_evidence, listeners) =
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            let attachment_provider =
                nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment");
            let ingress_provider =
                nimbus_network::NetworkProviderId::for_registration_key("fixture-ingress");
            let bundle = NetworkCapabilityBundle::new(
                NetworkAttachmentProviderRegistration::new(
                    attachment_provider,
                    attachment,
                    [NetworkAddressFamily::Ipv4],
                    lifecycle.clone(),
                    NetworkSovereigntyCapabilities::new(
                        NetworkControlPlaneLocality::LocalOnly,
                        [],
                        true,
                    ),
                ),
                NetworkIngressProviderRegistration::new(
                    ingress_provider,
                    endpoint,
                    ingress,
                    forwarding,
                    lifecycle,
                    NetworkSovereigntyCapabilities::new(
                        NetworkControlPlaneLocality::LocalOnly,
                        [],
                        true,
                    ),
                ),
            );
            let listener = WorkloadNetworkListenerBlueprint::new(
                &identity,
                "api",
                EndpointProtocol::Http,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                nimbus_workloads::WorkloadNetworkPortRequestMode::ProviderAssigned,
                WorkloadNetworkEndpointSemantics::new(
                    WorkloadNetworkForwardingBehavior::None,
                    NetworkTlsBehavior::Disabled,
                ),
                None,
            )
            .expect("fixture listener is valid");
            (
                Some(bundle.selection()),
                Some(bundle.selection_evidence()),
                vec![listener],
            )
        } else {
            (None, None, Vec::new())
        };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        selection,
        selection_evidence,
        None,
        [],
        listeners,
        [],
        activation,
        publication,
    )
    .expect("fixture network content is valid");
    CompiledWorkloadNetworkPlan::from_content(content).expect("fixture network plan is valid")
}

fn valid_successor(current: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_fixture::first_proposed_candidate(current)
}

fn valid_competing_successor(current: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let detail = WorkloadPhaseDetail::teardown(
        nimbus_workloads::WorkloadSagaPhase::WithdrawalCommitted,
        current.active_intent(),
        current.phase(),
        current.phase_detail().references(),
        Vec::new(),
    )
    .expect("fixture teardown detail is valid");
    current
        .advance(
            nimbus_workloads::WorkloadSagaPhase::WithdrawalCommitted,
            detail,
            None,
        )
        .expect("fixture competing successor is valid")
}

fn document_for(record: &WorkloadSagaRecord) -> Document {
    Document::with_id(
        nimbus_core::DocumentId::from_key(record.saga_id().as_str())
            .expect("saga id is a document key"),
        workload_saga_table().expect("private table is valid"),
        encode_workload_saga_record(record).expect("fixture encodes"),
    )
}
