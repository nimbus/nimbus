use std::sync::Arc;

use nimbus_core::{Document, TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_network::{NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadDesiredDigest, WorkloadEffectReferences,
    WorkloadNetworkIntent, WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation,
    WorkloadPhaseDetail, WorkloadPublicationIntent, WorkloadSagaKey, WorkloadSagaRecord,
};

use super::codec::encode_workload_saga_record;
use super::schema::workload_saga_table;

mod ambiguity;
mod codec;
mod composition;
mod durability;
mod recovery;
mod store;

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
        WorkloadDesiredDigest::sha256(format!("desired-{label}-{seed}")),
        WorkloadNetworkIntent::new(
            NetworkPlanId::for_tenant_workload_plan(&tenant_id, label),
            NetworkResourceGeneration::new(network_generation),
            NetworkPlanDigest::from_bytes([0x31; 32]),
        ),
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
