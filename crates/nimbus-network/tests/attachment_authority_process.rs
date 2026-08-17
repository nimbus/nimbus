use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkAttachmentAuthority, NetworkAttachmentCapabilitySet, NetworkAttachmentId,
    NetworkAttachmentSegmentAssociation, NetworkCapabilityRequirements,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkLeaseEpoch, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId,
    NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration,
    NetworkSegmentId, NetworkSovereigntyRequirements,
};
use tempfile::TempDir;

const CHILD_STATE_ROOT: &str = "NIMBUS_ATTACHMENT_AUTHORITY_CHILD_STATE_ROOT";
const CHILD_PROOF_FILE: &str = "attachment-authority-child.proof";

fn tenant() -> TenantId {
    TenantId::new("tenant-process").expect("tenant fixture should validate")
}

fn attachment() -> NetworkAttachmentId {
    NetworkAttachmentId::for_workload_attachment("workload-process", "default")
}

fn provider(label: &str) -> NetworkProviderId {
    NetworkProviderId::for_registration_key(label)
}

fn association() -> NetworkAttachmentSegmentAssociation {
    NetworkAttachmentSegmentAssociation::new(
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(
                provider("nimbus.test.attachment-coordinator"),
                "launch-attempt-process",
            )
            .expect("reservation claim should validate"),
        ),
        "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse::<NetworkSegmentId>()
            .expect("segment fixture should parse"),
        NetworkLeaseEpoch::new(73),
    )
}

fn plan(tenant_id: &TenantId) -> NetworkPlan {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::ThirdParty, [], false),
    );
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, "workload-process"),
        NetworkResourceGeneration::new(19),
        NetworkPlanContentDigest::sha256(b"process-desired"),
        requirements,
    )
}

#[test]
fn fresh_process_reopens_exact_attachment_segment_association() {
    let root = TempDir::new().expect("temporary state root should exist");
    let tenant_id = tenant();
    let attachment_id = attachment();
    let authority =
        LocalNetworkAttachmentAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(
            &tenant_id,
            provider("nimbus.test.attachment"),
            &plan(&tenant_id),
            attachment_id,
            association(),
        )
        .expect("attachment should reserve");
    drop(authority);

    let output = Command::new(env::current_exe().expect("test executable should resolve"))
        .arg("--ignored")
        .arg("--exact")
        .arg("fresh_process_child_reopens_exact_attachment_segment_association")
        .arg("--nocapture")
        .env(CHILD_STATE_ROOT, root.path())
        .output()
        .expect("fresh child process should start");

    assert!(
        output.status.success(),
        "fresh process failed to authenticate association\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(root.path().join(CHILD_PROOF_FILE))
            .expect("child must leave an out-of-band execution proof"),
        b"authenticated",
        "a zero-test child invocation must not count as process-boundary proof"
    );
}

#[test]
#[ignore = "subprocess entry point; exercised by the parent test"]
fn fresh_process_child_reopens_exact_attachment_segment_association() {
    let root =
        PathBuf::from(env::var_os(CHILD_STATE_ROOT).expect("parent must provide the state root"));
    let tenant_id = tenant();
    let attachment_id = attachment();
    let authority =
        LocalNetworkAttachmentAuthority::open(&root).expect("child should reopen authority");
    let record = authority
        .get(&tenant_id, &attachment_id)
        .expect("child inspection should succeed")
        .expect("child should observe the attachment");

    assert_eq!(record.association(), &association());
    assert_eq!(
        record.resource().version().lease_epoch(),
        association().lease_epoch(),
        "resource epoch must be derived from the durable association"
    );
    fs::write(root.join(CHILD_PROOF_FILE), b"authenticated")
        .expect("child should publish its out-of-band execution proof");
}
