use std::collections::BTreeMap;

use super::super::OciNetworkConfig;
use super::*;
use crate::backends::oci::network::{SingleNodeSegmentAllocator, direct_test_ipam_authority};
use nimbus_core::TenantId;
use nimbus_network::{
    LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim,
    NetworkSegmentAllocator,
};
use tempfile::tempdir;

use crate::instance::SandboxId;

fn touch_netns(root: &Path, tenant: &str, sandbox: &str) {
    let dir = root
        .join("tenants")
        .join(tenant)
        .join("networks")
        .join("netns");
    std::fs::create_dir_all(&dir).expect("netns dir");
    std::fs::write(dir.join(sandbox), b"").expect("netns file");
}

fn touch_evidence(root: &Path, directory: &str, tenant: &str, sandbox: &str, value: &str) {
    let dir = root
        .join("tenants")
        .join(tenant)
        .join("networks")
        .join(directory);
    std::fs::create_dir_all(&dir).expect("evidence directory");
    std::fs::write(dir.join(format!("{sandbox}.json")), value).expect("evidence file");
}

fn touch_manifest(root: &Path, tenant: &str, sandbox: &str) {
    let dir = root.join("tenants").join(tenant).join("sandboxes");
    std::fs::create_dir_all(&dir).expect("manifest directory");
    std::fs::write(dir.join(format!("{sandbox}.json")), "{}").expect("manifest file");
}

fn evidence_exists(root: &Path, directory: &str, tenant: &str, sandbox: &str) -> bool {
    root.join("tenants")
        .join(tenant)
        .join("networks")
        .join(directory)
        .join(format!("{sandbox}.json"))
        .exists()
}

fn manifest_exists(root: &Path, tenant: &str, sandbox: &str) -> bool {
    root.join("tenants")
        .join(tenant)
        .join("sandboxes")
        .join(format!("{sandbox}.json"))
        .exists()
}

fn reservation_claim(attempt: &str) -> NetworkReservationClaim {
    let provider =
        NetworkProviderId::for_registration_key("nimbus-sandbox.network-launch-coordinator.test");
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider, format!("attempt:{attempt}"))
            .expect("claim fixture should validate"),
    )
}

fn allocator_has_hold(root: &Path, tenant: &str, sandbox: &str) -> bool {
    SingleNodeSegmentAllocator::single_node_default(root).has_hold(tenant, sandbox)
}

#[test]
fn exact_pre_effect_compensation_removes_ipam_before_segment_authority() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-exact-compensation").expect("tenant fixture");
    let sandbox = SandboxId::new("sandbox-exact-compensation");
    let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let claim = reservation_claim("exact-compensation");
    super::super::placement::place_sandbox_on_block(
        &allocator,
        &ipam_authority,
        &tenant,
        &layout,
        &sandbox,
        &claim,
        super::super::placement::OciPlacementProvider::new(
            super::super::provider_locator::OciAttachmentProviderKind::Container,
            |segment, reservation_claim| OciNetworkConfig {
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        ),
    )
    .expect("placement should reserve attachment and IPAM");
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
        "placement must create durable IPAM before port reservation"
    );

    release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            &layout,
            &tenant,
            &sandbox,
            &claim,
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect("exact compensation should complete");

    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_err(),
        "IPAM must be gone before segment authority is reusable"
    );
    assert!(
        allocator
            .inspect_segments(&tenant)
            .expect("segment authority should inspect")
            .is_none(),
        "last exact hold should finalize the tenant segment"
    );
}

#[test]
fn exact_pre_effect_compensation_removes_ipam_while_sibling_hold_remains() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-sibling-compensation").expect("tenant fixture");
    let cancelled = SandboxId::new("sandbox-cancelled");
    let sibling = SandboxId::new("sandbox-sibling");
    let cancelled_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &cancelled);
    let sibling_layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sibling);
    let ipam_authority = direct_test_ipam_authority(&cancelled_layout);
    cancelled_layout
        .ensure_directories()
        .expect("cancelled layout should initialize");
    sibling_layout
        .ensure_directories()
        .expect("sibling layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let cancelled_claim = reservation_claim("cancelled");
    let sibling_claim = reservation_claim("sibling");
    for (layout, sandbox, claim) in [
        (&cancelled_layout, &cancelled, &cancelled_claim),
        (&sibling_layout, &sibling, &sibling_claim),
    ] {
        super::super::placement::place_sandbox_on_block(
            &allocator,
            &ipam_authority,
            &tenant,
            layout,
            sandbox,
            claim,
            super::super::placement::OciPlacementProvider::new(
                super::super::provider_locator::OciAttachmentProviderKind::Container,
                |segment, reservation_claim| OciNetworkConfig {
                    network_name: segment.network_name().to_owned(),
                    network_interface: segment.network_interface().to_owned(),
                    network_subnet: segment.cidr().to_string(),
                    segment_id: segment.segment_id().as_str().to_owned(),
                    reservation_claim: reservation_claim.clone(),
                    network_id: segment.network_id().as_str().to_owned(),
                    ..OciNetworkConfig::default()
                },
            ),
        )
        .expect("placement should reserve attachment and IPAM");
    }

    release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            &cancelled_layout,
            &tenant,
            &cancelled,
            &cancelled_claim,
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect("exact compensation should remove only the cancelled launch");

    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &cancelled_layout, &cancelled,)
            .is_err(),
        "the cancelled launch must not leak IPAM merely because a sibling retains the segment"
    );
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &sibling_layout, &sibling).is_ok(),
        "the live sibling's IPAM allocation must remain intact"
    );
    assert!(
        !allocator.has_hold(tenant.as_str(), cancelled.as_str()),
        "the exact cancelled attachment must be released"
    );
    assert!(
        allocator.has_hold(tenant.as_str(), sibling.as_str()),
        "the sibling attachment must continue to hold the tenant segment"
    );
    assert!(
        allocator
            .inspect_segments(&tenant)
            .expect("segment authority should inspect")
            .is_some(),
        "a sibling attachment must keep the tenant segment live"
    );
}

#[test]
fn foreign_pre_effect_claim_preserves_ipam_and_segment_authority_byte_for_byte() {
    let dir = tempdir().expect("state root");
    let tenant = TenantId::new("tenant-foreign-compensation").expect("tenant fixture");
    let sandbox = SandboxId::new("sandbox-foreign-compensation");
    let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("layout should initialize");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let winner = reservation_claim("winner");
    super::super::placement::place_sandbox_on_block(
        &allocator,
        &ipam_authority,
        &tenant,
        &layout,
        &sandbox,
        &winner,
        super::super::placement::OciPlacementProvider::new(
            super::super::provider_locator::OciAttachmentProviderKind::Container,
            |segment, reservation_claim| OciNetworkConfig {
                network_name: segment.network_name().to_owned(),
                network_interface: segment.network_interface().to_owned(),
                network_subnet: segment.cidr().to_string(),
                segment_id: segment.segment_id().as_str().to_owned(),
                reservation_claim: reservation_claim.clone(),
                network_id: segment.network_id().as_str().to_owned(),
                ..OciNetworkConfig::default()
            },
        ),
    )
    .expect("winner should reserve attachment and IPAM");
    let authority_path = LocalNetworkStateStore::authority_path_for(dir.path());
    let before = std::fs::read(&authority_path).expect("authority should be durable");

    let error = release_reserved_network_launch_after_ports(
        ReservedNetworkLaunchAuthority::new(
            &allocator,
            &ipam_authority,
            &layout,
            &tenant,
            &sandbox,
            &reservation_claim("foreign"),
            super::super::provider_locator::OciAttachmentProviderKind::Container,
        ),
        Ok(()),
    )
    .expect_err("a foreign coordinator must not compensate the winner");
    assert!(
        error
            .to_string()
            .contains("different launch reservation coordinator"),
        "claim rejection should remain explicit: {error}"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("rejected authority should remain readable"),
        before,
        "claim authentication must precede every IPAM or segment mutation"
    );
    assert!(
        super::super::ipam::load_container_ips(&ipam_authority, &layout, &sandbox).is_ok(),
        "the winning coordinator's IPAM evidence must remain intact"
    );
    assert!(
        allocator.has_hold(tenant.as_str(), sandbox.as_str()),
        "the winning coordinator's reserved attachment must remain intact"
    );
}

#[test]
fn reconcile_quarantines_holds_whose_netns_is_gone_and_keeps_live_ones() {
    let dir = tempdir().expect("temp dir");
    let root = dir.path();
    let allocator = SingleNodeSegmentAllocator::single_node_default(root);

    // tenant-live (index 0) holds a sandbox that still has a netns.
    allocator
        .acquire(
            &TenantId::new("tenant-live").unwrap(),
            &default_network_attachment_id(&SandboxId::new("sb-live")),
        )
        .expect("acquire live");
    touch_netns(root, "tenant-live", "sb-live");
    // tenant-dead (index 1) holds a sandbox whose netns is gone (crash-leaked).
    allocator
        .acquire(
            &TenantId::new("tenant-dead").unwrap(),
            &default_network_attachment_id(&SandboxId::new("sb-dead")),
        )
        .expect("acquire dead");

    let quarantined = reconcile_network_segment_orphans(root, &allocator).expect("reconcile");
    assert_eq!(quarantined, 1, "only the netns-less tenant is quarantined");
    assert!(
        allocator.has_hold("tenant-dead", "sb-dead")
            && allocator.has_pending_hold("tenant-dead", "sb-dead"),
        "netns absence alone must preserve and quarantine the durable hold"
    );

    // tenant-dead's uncertain index 1 remains unavailable.
    let fresh = allocator
        .acquire(
            &TenantId::new("tenant-new").unwrap(),
            &default_network_attachment_id(&SandboxId::new("sb-new")),
        )
        .expect("acquire new");
    assert_eq!(fresh.cidr().to_string(), "10.0.2.0/24");
    // tenant-live still holds its original index 0.
    let live = allocator
        .acquire(
            &TenantId::new("tenant-live").unwrap(),
            &default_network_attachment_id(&SandboxId::new("sb-live")),
        )
        .expect("re-acquire live");
    assert_eq!(live.cidr().to_string(), "10.0.0.0/24");
}

#[test]
fn startup_reconciliation_reads_live_netns_from_the_workload_root() {
    let dir = tempdir().expect("temp dir");
    let workload_root = dir.path().join("project-state");
    let network_root = dir.path().join("node-network-state");
    let allocator = SingleNodeSegmentAllocator::single_node_default(&network_root);
    let tenant = TenantId::new("tenant-split-root-live").expect("tenant should parse");
    let sandbox = SandboxId::new("sandbox-split-root-live");
    let layout = OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    allocator
        .acquire(&tenant, &default_network_attachment_id(&sandbox))
        .expect("live attachment should allocate");
    touch_netns(&workload_root, tenant.as_str(), sandbox.as_str());

    super::super::reconcile_startup_network_state(&workload_root, &ipam_authority, &allocator)
        .expect("startup reconciliation must retain workload-root netns evidence");

    assert!(
        allocator.has_hold(tenant.as_str(), sandbox.as_str())
            && !allocator.has_pending_hold(tenant.as_str(), sandbox.as_str()),
        "a live workload-root netns must not be quarantined as a network-root orphan"
    );
}

#[test]
fn reconcile_ignores_non_tenant_siblings_without_a_netns_tree() {
    let dir = tempdir().expect("temp dir");
    let root = dir.path();
    let allocator = SingleNodeSegmentAllocator::single_node_default(root);
    let tenant = TenantId::new("tenant-dead").expect("tenant should parse");
    allocator
        .acquire(
            &tenant,
            &default_network_attachment_id(&SandboxId::new("sb-dead")),
        )
        .expect("orphan hold should allocate");
    let tenants_root = root.join("tenants");
    std::fs::create_dir_all(&tenants_root).expect("tenants root should exist");
    std::fs::write(tenants_root.join(".DS_Store"), b"foreign metadata")
        .expect("foreign sibling fixture should write");

    let quarantined = reconcile_network_segment_orphans(root, &allocator)
        .expect("a non-tenant sibling without a netns tree must be ignored");

    assert_eq!(quarantined, 1, "the real orphan must still be quarantined");
    assert!(
        allocator.has_hold(tenant.as_str(), "sb-dead")
            && allocator.has_pending_hold(tenant.as_str(), "sb-dead"),
        "foreign siblings must not suppress durable hold quarantine"
    );
}

#[derive(Clone, Copy)]
struct OrphanEvidenceCase {
    name: &'static str,
    hold: bool,
    desired: bool,
    netns: bool,
    manifest: bool,
    effect: bool,
    desired_generation: u64,
    effect_generation: u64,
    inspection_unknown: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct OrphanObservation {
    reclaimed_segments: usize,
    hold: bool,
    desired: bool,
    netns: bool,
    manifest: bool,
    effect: bool,
    classifier_result: &'static str,
}

fn observe_orphan_case(case: OrphanEvidenceCase) -> OrphanObservation {
    let dir = tempdir().expect("temp dir");
    let root = dir.path();
    let allocator = SingleNodeSegmentAllocator::single_node_default(root);
    let tenant = format!("tenant-{}", case.name);
    let sandbox = format!("sandbox-{}", case.name);
    let tenant_id = TenantId::new(&tenant).expect("tenant id should parse");
    let sandbox_id = SandboxId::new(&sandbox);

    if case.hold {
        allocator
            .acquire(&tenant_id, &default_network_attachment_id(&sandbox_id))
            .expect("hold should persist");
    }
    if case.desired {
        touch_evidence(
            root,
            "attachments",
            &tenant,
            &sandbox,
            &format!(r#"{{"generation":{}}}"#, case.desired_generation),
        );
    }
    if case.netns {
        touch_netns(root, &tenant, &sandbox);
    }
    if case.manifest {
        touch_manifest(root, &tenant, &sandbox);
    }
    if case.effect {
        touch_evidence(
            root,
            "provider-effects",
            &tenant,
            &sandbox,
            &format!(
                r#"{{"generation":{},"inspection":"{}"}}"#,
                case.effect_generation,
                if case.inspection_unknown {
                    "unknown"
                } else {
                    "present"
                }
            ),
        );
    }

    let reclaimed_segments =
        reconcile_network_segment_orphans(root, &allocator).expect("reconcile should run");
    let hold = allocator_has_hold(root, &tenant, &sandbox);
    let desired = evidence_exists(root, "attachments", &tenant, &sandbox);
    let netns = root
        .join("tenants")
        .join(&tenant)
        .join("networks")
        .join("netns")
        .join(&sandbox)
        .exists();
    let manifest = manifest_exists(root, &tenant, &sandbox);
    let effect = evidence_exists(root, "provider-effects", &tenant, &sandbox);
    let classifier_result = if hold {
        "retained-by-netns-filename"
    } else if desired || netns || manifest || effect {
        "unowned-evidence-left-behind"
    } else {
        "fully-removed"
    };

    OrphanObservation {
        reclaimed_segments,
        hold,
        desired,
        netns,
        manifest,
        effect,
        classifier_result,
    }
}

#[test]
// This is the NNC0.7 fail-before executable baseline for the exact
// `provider effect -> allocator hold` crash window in both OCI-family
// backends. NNC0.1b already proves exact-boundary process killing and
// same-root recovery; this test materializes the durable recovery image
// left by that cut without duplicating the upper-layer subprocess harness
// (which cannot be a dependency of this low-level crate).
#[ignore = "NNC0.7 expected red until provider attempts precede effects and reconcile removes or quarantines unowned effects"]
fn nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect() {
    let observed = observe_orphan_case(OrphanEvidenceCase {
        name: "crash-after-effect-before-hold",
        hold: false,
        desired: false,
        netns: true,
        manifest: true,
        effect: true,
        desired_generation: 0,
        effect_generation: 7,
        inspection_unknown: false,
    });

    assert!(!observed.hold, "the crash cut precedes allocator acquire");
    assert!(
        observed.netns && observed.effect,
        "the exact provider-effect boundary must be present before the safety assertion"
    );
    assert_eq!(
        observed.classifier_result, "fully-removed",
        "NNCF8: recovery must remove or durably quarantine the provider effect and netns \
             when no desired attachment/provider attempt owns them"
    );
}

#[test]
// This is the complete NNC0.7 fail-before evidence matrix for NNCF8. It
// intentionally uses durable desired/effect/generation/inspection markers
// that the current filename-only reaper cannot read. NNC5.2a owns the
// classifier and must turn this green by adopting, removing, or
// quarantining every row; NNC8.3 owns restart convergence.
#[ignore = "NNC0.7 expected red until orphan recovery classifies durable intent, provider attempts, generations, and unknown inspection"]
fn nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix() {
    let cases = [
        (
            OrphanEvidenceCase {
                name: "hold-desired-effect",
                hold: true,
                desired: true,
                netns: true,
                manifest: true,
                effect: true,
                desired_generation: 7,
                effect_generation: 7,
                inspection_unknown: false,
            },
            "adopted",
        ),
        (
            OrphanEvidenceCase {
                name: "hold-no-desired-effect",
                hold: true,
                desired: false,
                netns: true,
                manifest: true,
                effect: true,
                desired_generation: 0,
                effect_generation: 7,
                inspection_unknown: false,
            },
            "removed-or-quarantined",
        ),
        (
            OrphanEvidenceCase {
                name: "hold-no-netns",
                hold: true,
                desired: true,
                netns: false,
                manifest: true,
                effect: false,
                desired_generation: 7,
                effect_generation: 0,
                inspection_unknown: false,
            },
            "removed-or-quarantined",
        ),
        (
            OrphanEvidenceCase {
                name: "effect-no-hold",
                hold: false,
                desired: false,
                netns: true,
                manifest: true,
                effect: true,
                desired_generation: 0,
                effect_generation: 7,
                inspection_unknown: false,
            },
            "removed-or-quarantined",
        ),
        (
            OrphanEvidenceCase {
                name: "manifest-no-hold",
                hold: false,
                desired: false,
                netns: false,
                manifest: true,
                effect: false,
                desired_generation: 0,
                effect_generation: 0,
                inspection_unknown: false,
            },
            "removed-or-quarantined",
        ),
        (
            OrphanEvidenceCase {
                name: "hold-netns-no-manifest",
                hold: true,
                desired: true,
                netns: true,
                manifest: false,
                effect: true,
                desired_generation: 7,
                effect_generation: 7,
                inspection_unknown: false,
            },
            "adopted",
        ),
        (
            OrphanEvidenceCase {
                name: "stale-generation",
                hold: true,
                desired: true,
                netns: true,
                manifest: true,
                effect: true,
                desired_generation: 8,
                effect_generation: 7,
                inspection_unknown: false,
            },
            "removed-or-quarantined",
        ),
        (
            OrphanEvidenceCase {
                name: "unknown-inspection",
                hold: true,
                desired: true,
                netns: true,
                manifest: true,
                effect: true,
                desired_generation: 7,
                effect_generation: 7,
                inspection_unknown: true,
            },
            "cleanup-pending",
        ),
    ];

    let observed: BTreeMap<&str, (&str, OrphanObservation)> = cases
        .into_iter()
        .map(|(case, expected)| (case.name, (expected, observe_orphan_case(case))))
        .collect();
    assert_eq!(observed.len(), 8, "every required evidence arm must run");
    assert!(
        observed.values().all(|(_, observation)| {
            observation.classifier_result == "retained-by-netns-filename"
                || observation.classifier_result == "unowned-evidence-left-behind"
        }),
        "precondition: the current reaper must expose its filename/hold-only behavior"
    );

    let mismatches: BTreeMap<&str, (&str, &str)> = observed
        .iter()
        .filter_map(|(name, (expected, observation))| {
            (*expected != observation.classifier_result)
                .then_some((*name, (*expected, observation.classifier_result)))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "NNCF8: every restart state must be adopted, removed, quarantined, or held \
             cleanup-pending from durable ownership evidence; current mismatches: {mismatches:#?}; \
             full observations: {observed:#?}"
    );
}

#[test]
// NNC0.3 pass-after: bridge cleanup failure leaves the exact allocation
// cleanup-pending; a later successful retry finalizes it exactly once.
fn failed_bridge_cleanup_must_fence_segment_from_reuse() {
    let dir = tempdir().expect("temp dir");
    let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
    let original_tenant = TenantId::new("tenant-original").expect("tenant should parse");
    let original_sandbox = SandboxId::new("sandbox-original");
    let original = allocator
        .acquire(
            &original_tenant,
            &default_network_attachment_id(&original_sandbox),
        )
        .expect("original segment should allocate");

    let mut surviving_bridges = Vec::new();
    let cleanup_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_sandbox,
        None,
        |segment| {
            surviving_bridges.push(segment.network_interface().to_owned());
            Err(SandboxError::OperationFailed {
                message: "forced bridge provider cleanup failure".to_owned(),
            })
        },
    );
    assert_eq!(cleanup_errors.len(), 1);
    assert!(
        cleanup_errors[0]
            .to_string()
            .contains("forced bridge provider cleanup failure")
    );
    assert_eq!(
        surviving_bridges,
        [original.network_interface().to_owned()],
        "the failed provider cleanup leaves the original bridge effect present"
    );

    let replacement = allocator
        .acquire(
            &TenantId::new("tenant-replacement").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-replacement")),
        )
        .expect("replacement segment should allocate");
    assert_ne!(
        replacement.cidr(),
        original.cidr(),
        "a segment with a surviving provider effect must remain fenced from reuse"
    );

    let mut successful_reaps = 0usize;
    let retry_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_sandbox,
        None,
        |segment| {
            successful_reaps += 1;
            assert_eq!(segment.segment_id(), original.segment_id());
            Ok(())
        },
    );
    assert!(retry_errors.is_empty(), "cleanup retry should succeed");
    assert_eq!(successful_reaps, 1, "the bridge should be deleted once");

    let recovered = allocator
        .acquire(
            &TenantId::new("tenant-recovered").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-recovered")),
        )
        .expect("confirmed cleanup should make the slot reusable");
    assert_eq!(recovered.cidr(), original.cidr());
    assert_ne!(
        recovered.segment_id(),
        original.segment_id(),
        "reused location must receive a new stable identity"
    );

    let repeated_errors = release_network_segment_hold_with(
        &allocator,
        &original_tenant,
        &original_sandbox,
        None,
        |_| {
            successful_reaps += 1;
            Ok(())
        },
    );
    assert!(
        repeated_errors.is_empty(),
        "repeated finalization should be idempotent"
    );
    assert_eq!(
        successful_reaps, 1,
        "already-finalized cleanup must not repeat provider deletion"
    );
    let next = allocator
        .acquire(
            &TenantId::new("tenant-next").expect("tenant should parse"),
            &default_network_attachment_id(&SandboxId::new("sandbox-next")),
        )
        .expect("next segment should allocate");
    assert_eq!(
        next.cidr().to_string(),
        "10.0.2.0/24",
        "the recovered slot is owned exactly once, not handed out twice"
    );
}
