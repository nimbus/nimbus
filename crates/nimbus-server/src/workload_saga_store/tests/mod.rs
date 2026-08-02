use std::sync::Arc;

use nimbus_core::{Document, TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkResourceGeneration,
    NetworkSovereigntyRequirements,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadEffectReferences,
    WorkloadNetworkIntent, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
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
mod recovery;
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
    let intent = nimbus_workloads::WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        nimbus_workloads::WorkloadGeneration::new(generation),
        nimbus_workloads::WorkloadExecutableIntent::new(
            nimbus_workloads::WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            format!(r#"{{"fixture":"desired-{label}-{seed}"}}"#),
        )
        .expect("fixture executable is valid"),
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
            Some(NodeIdentity::new(format!("node-{label}")).expect("fixture node is valid")),
        ),
    )
    .expect("fixture intent is valid");
    WorkloadSagaRecord::new(key, intent).expect("initial record is valid")
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
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        [],
        [],
        [],
        activation,
        publication,
    )
    .expect("fixture network content is valid");
    CompiledWorkloadNetworkPlan::from_content(content).expect("fixture network plan is valid")
}

fn valid_successor(current: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let references = WorkloadEffectReferences::provision(current.active_intent(), None)
        .expect("fixture references are valid");
    let observation = WorkloadOwnerObservation::NetworkReserved {
        reference: references
            .network()
            .expect("provision references contain network authority")
            .clone(),
        evidence: WorkloadOwnerEvidenceDigest::sha256("network-reserved"),
    };
    let detail = WorkloadPhaseDetail::provision(
        nimbus_workloads::WorkloadSagaPhase::NetworkReserved,
        current.active_intent(),
        references,
        vec![observation],
    )
    .expect("fixture phase detail is valid");
    current
        .advance(
            nimbus_workloads::WorkloadSagaPhase::NetworkReserved,
            detail,
            None,
        )
        .expect("fixture successor is valid")
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
